use super::*;

/// An admitted WM presentation, qualified by the WM connection epoch that
/// admitted it (t244). Admission, validation and the policy reducer belong to
/// the protocol seam (t243); the renderer draws only what it is handed, and
/// resolves every source's committed generation itself at frame capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivePolicyPresentation {
    pub owner_epoch: u64,
    pub presentation: PolicyPresentation,
}

/// Why an admitted presentation was not installed. The previous one stays.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LivePolicyPresentationRefusal {
    /// An instance names a source with no committed content in the scene
    /// being drawn. The candidate is refused whole; readiness settles it.
    MissingSource { source: SurfaceId },
    /// An output the candidate covers has no enabled head to draw it on.
    MissingHeads { output: OutputId },
    /// A target would draw no clipped pixel on some head of its output, by
    /// the head plan's own arithmetic: presenting it would attest a draw
    /// that did not happen. `id` is the instance or region id.
    UndrawnTarget { output: OutputId, id: u64 },
}

impl std::fmt::Display for LivePolicyPresentationRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LivePolicyPresentationRefusal {}

/// A presentation withdrawn because a source it samples left the scene. The
/// whole publication is revoked, never one instance; its owner is told.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LivePolicyPresentationRevocation {
    pub owner_epoch: u64,
    pub generation: u64,
    pub source: SurfaceId,
}

/// The publication an output's retired frame actually presents, read from
/// that frame alone: distinct from the requested [`LivePolicyPresentation`],
/// which may have moved on. A presented `(id, generation)` fixes the admitted
/// record's geometry, clip, z order and action, since any change to those
/// takes a new target generation; a source repaint changes neither.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivePresentedPolicyPublication {
    pub owner_epoch: u64,
    pub generation: u64,
    pub output: OutputId,
    pub output_generation: u64,
    /// `(id, generation)` of each instance drawn, back to front.
    pub instances: Vec<(u64, u64)>,
    /// `(id, generation)` of each region drawn, back to front.
    pub regions: Vec<(u64, u64)>,
}

impl LivePresentedPolicyPublication {
    /// What a whole output presents, from the frame each of its heads last
    /// retired (primary first, as the native target reports them): a
    /// publication only once every head has retired a frame with the same
    /// stamp identity (owner epoch, publication generation, output, output
    /// generation). A lagging mirror head, or one still showing an earlier
    /// publication or none, leaves the output without a completed
    /// publication. A matching stamp does not prove the same draw: the
    /// target lists are those `(id, generation)` every head drew, in the
    /// primary's order, so a target any head crops away is not listed and
    /// the session's completeness check fails closed (t244).
    pub fn from_presented_heads(frames: &[Option<&OutputFrameDamageSnapshot>]) -> Option<Self> {
        let mut heads = frames.iter();
        let primary = Self::from_presented_frame((*heads.next()?)?)?;
        let identity = |publication: &Self| {
            (
                publication.owner_epoch,
                publication.generation,
                publication.output,
                publication.output_generation,
            )
        };
        let mut all = primary.clone();
        for frame in heads {
            let head = frame.and_then(Self::from_presented_frame)?;
            if identity(&head) != identity(&primary) {
                return None;
            }
            all.instances
                .retain(|target| head.instances.contains(target));
            all.regions.retain(|target| head.regions.contains(target));
        }
        Some(all)
    }

    /// None when the frame presents no WM publication.
    pub fn from_presented_frame(frame: &OutputFrameDamageSnapshot) -> Option<Self> {
        let list = &frame.compositor_display_list;
        let stamp = list.presentation_stamp()?;
        let owned = |node: CompositorNodeId| match node {
            CompositorNodeId::PolicyRegion { owner_epoch, id }
                if owner_epoch == stamp.owner_epoch =>
            {
                Some(id)
            }
            _ => None,
        };
        let mut regions = Vec::new();
        for command in &list.commands {
            let region = match command {
                CompositorDisplayCommand::Rect(rect) => {
                    owned(rect.node).map(|id| (id, rect.generation))
                }
                CompositorDisplayCommand::Border(border) => {
                    owned(border.node).map(|id| (id, border.generation))
                }
                _ => None,
            };
            if let Some(region) = region {
                regions.push(region);
            }
        }
        Some(Self {
            owner_epoch: stamp.owner_epoch,
            generation: stamp.publication_generation,
            output: stamp.output,
            output_generation: stamp.output_generation,
            instances: list
                .surface_instances()
                .filter(|instance| instance.owner_epoch == stamp.owner_epoch)
                .map(|instance| (instance.id, instance.generation))
                .collect(),
            regions,
        })
    }
}

impl LivePolicyPresentation {
    /// Whether this output shows the presentation in place of its ordinary
    /// application presentation.
    pub(super) fn replaces_applications(&self, output: OutputId) -> bool {
        self.presentation.outputs.iter().any(|record| {
            record.output == output && record.mode == PolicyPresentationMode::ReplaceApplications
        })
    }

    /// This output's tier: the stamp naming the publication, then regions
    /// and instances in their one z order. Regions are Engine chrome in its
    /// own palette: Backdrop the frame colour fully opaque, Frame the frame
    /// stroke, Emphasis the focus-ring stroke, all within the region's
    /// clipped allocation. An instance's source generation is left for
    /// [`resolve_surface_instance_sources`], so a value the WM could
    /// influence never reaches a frame. Nothing when this output has no
    /// output record.
    pub(super) fn tier_commands(
        &self,
        output: OutputId,
        style: SurfaceChromeStyle,
    ) -> Vec<CompositorDisplayCommand> {
        let Some(record) = self
            .presentation
            .outputs
            .iter()
            .find(|record| record.output == output)
        else {
            return Vec::new();
        };
        let mut layered = Vec::new();
        for instance in self
            .presentation
            .instances
            .iter()
            .filter(|instance| instance.output == output)
        {
            layered.push((
                instance.z_index,
                CompositorDisplayCommand::SurfaceInstance(CompositorSurfaceInstance {
                    owner_epoch: self.owner_epoch,
                    id: instance.id,
                    generation: instance.generation,
                    source: instance.source,
                    source_generation: 0,
                    destination: instance.destination,
                    clip: instance.clip,
                    opacity_millis: instance.opacity_millis,
                }),
            ));
        }
        for region in self
            .presentation
            .regions
            .iter()
            .filter(|region| region.output == output)
        {
            if let Some(command) = region_command(self.owner_epoch, region, style) {
                layered.push((region.z_index, command));
            }
        }
        layered.sort_by_key(|(z_index, _)| *z_index);
        std::iter::once(CompositorDisplayCommand::PresentationStamp(
            CompositorPresentationStamp {
                owner_epoch: self.owner_epoch,
                publication_generation: self.presentation.generation,
                output,
                output_generation: record.generation,
                coverage: record.coverage,
            },
        ))
        .chain(layered.into_iter().map(|(_, command)| command))
        .collect()
    }

    fn sources(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.presentation
            .instances
            .iter()
            .map(|instance| instance.source)
    }
}

/// One region as Engine chrome within its clipped allocation.
fn region_command(
    owner_epoch: u64,
    region: &PolicyPresentationRegion,
    style: SurfaceChromeStyle,
) -> Option<CompositorDisplayCommand> {
    let allocation = clipped(region.geometry, region.clip)?;
    let node = CompositorNodeId::PolicyRegion {
        owner_epoch,
        id: region.id,
    };
    let stroke = |width: i32, color: CompositorRgb8| {
        let width = width.max(1);
        CompositorDisplayCommand::Border(CompositorBorder {
            node,
            generation: region.generation,
            outer: allocation,
            inner: Rect {
                x: allocation.x.saturating_add(width),
                y: allocation.y.saturating_add(width),
                width: allocation
                    .width
                    .saturating_sub(width.saturating_mul(2))
                    .max(0),
                height: allocation
                    .height
                    .saturating_sub(width.saturating_mul(2))
                    .max(0),
            },
            color,
        })
    };
    Some(match region.role {
        PolicyPresentationRegionRole::Backdrop => CompositorDisplayCommand::Rect(CompositorRect {
            opacity: u8::MAX,
            node,
            generation: region.generation,
            geometry: allocation,
            color: style.frame.unfocused_color,
        }),
        PolicyPresentationRegionRole::Frame => {
            stroke(style.frame.width, style.frame.unfocused_color)
        }
        PolicyPresentationRegionRole::Emphasis => {
            stroke(style.focus_ring.width, style.focus_ring.color)
        }
    })
}

fn clipped(geometry: Rect, clip: Rect) -> Option<Rect> {
    let x = geometry.x.max(clip.x);
    let y = geometry.y.max(clip.y);
    let right = geometry
        .x
        .saturating_add(geometry.width)
        .min(clip.x.saturating_add(clip.width));
    let bottom = geometry
        .y
        .saturating_add(geometry.height)
        .min(clip.y.saturating_add(clip.height));
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

impl LiveProductionVisualRuntime {
    /// Read-only: whether `candidate` could be installed now. Every source
    /// must have committed content both in the scene now displayed and in
    /// the committed set the next frame draws; otherwise the whole
    /// candidate is refused, and the installed presentation and layout stay
    /// valid. The session checks this before preparing a proposal and
    /// installs with [`Self::set_policy_presentation`] at commit, which
    /// checks again.
    pub fn validate_policy_presentation(
        &self,
        candidate: &LivePolicyPresentation,
    ) -> Result<(), LivePolicyPresentationRefusal> {
        match candidate.sources().find(|source| {
            ![self.displayed_surface_view(), self.committed_surfaces()]
                .iter()
                .all(|scene| scene.iter().any(|state| state.surface == *source))
        }) {
            Some(source) => Err(LivePolicyPresentationRefusal::MissingSource { source }),
            None => Ok(()),
        }
    }

    /// Read-only whole-candidate geometry admission against the heads given
    /// (normally every enabled head of the covered outputs, from
    /// [`Self::validate_policy_presentation_on_native`]): the source checks
    /// of [`Self::validate_policy_presentation`], then every covered output
    /// must have at least one head, and every instance and region must draw
    /// at least one clipped pixel on every one of them, borders by their
    /// clipped bands. It decides with `head_draws_policy_command`, the head
    /// plan's own arithmetic, so a refused candidate is exactly one some
    /// head would have presented without that target. The caller refuses the
    /// candidate before commit, so the WM keeps its prior state (t244).
    pub fn validate_policy_presentation_on_heads(
        &self,
        candidate: &LivePolicyPresentation,
        heads: &[HeadRenderTarget],
    ) -> Result<(), LivePolicyPresentationRefusal> {
        self.validate_policy_presentation(candidate)?;
        for record in &candidate.presentation.outputs {
            let output = record.output;
            let viewport = self.outputs.logical_viewport(output);
            let covering = heads
                .iter()
                .filter(|head| head.output == output)
                .collect::<Vec<_>>();
            let Some(viewport) = viewport.filter(|_| !covering.is_empty()) else {
                return Err(LivePolicyPresentationRefusal::MissingHeads { output });
            };
            for command in candidate.tier_commands(output, self.surface_chrome_style) {
                let id = match &command {
                    CompositorDisplayCommand::SurfaceInstance(instance) => instance.id,
                    CompositorDisplayCommand::Rect(CompositorRect {
                        node: CompositorNodeId::PolicyRegion { id, .. },
                        ..
                    })
                    | CompositorDisplayCommand::Border(CompositorBorder {
                        node: CompositorNodeId::PolicyRegion { id, .. },
                        ..
                    }) => *id,
                    _ => continue,
                };
                if covering.iter().any(|head| {
                    !sophia_engine::head_draws_policy_command(&command, viewport, **head)
                }) {
                    return Err(LivePolicyPresentationRefusal::UndrawnTarget { output, id });
                }
            }
        }
        Ok(())
    }

    /// [`Self::validate_policy_presentation_on_heads`] against every enabled
    /// head the native target currently has for the covered outputs.
    pub fn validate_policy_presentation_on_native(
        &self,
        candidate: &LivePolicyPresentation,
        native: &LiveProductionNativeScanout,
    ) -> Result<(), LivePolicyPresentationRefusal> {
        let heads = candidate
            .presentation
            .outputs
            .iter()
            .flat_map(|record| native.head_render_targets(record.output))
            .collect::<Vec<_>>();
        self.validate_policy_presentation_on_heads(candidate, &heads)
    }

    /// Installs or withdraws the admitted WM presentation and queues a
    /// retained repaint when native scanout owns presentation. The source
    /// surfaces keep their allocation, placement and output ownership.
    pub fn set_policy_presentation(
        &mut self,
        presentation: Option<LivePolicyPresentation>,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.policy_presentation == presentation {
            return Ok(false);
        }
        if let Some(candidate) = &presentation {
            self.validate_policy_presentation(candidate)?;
        }
        let previous = std::mem::replace(&mut self.policy_presentation, presentation);
        if let Some(native_scanout) = native_scanout
            && let Err(error) = self.queue_retained_projection(scene, native_scanout)
        {
            self.policy_presentation = previous;
            return Err(error);
        }
        Ok(true)
    }

    /// How a Present's composition must treat its own surface: under a
    /// presentation that replaces applications on every one of the Present's
    /// outputs, the surface may be sampled only by a preview, or not at all
    /// (t246). The tier may still be withheld for a missing source, in which
    /// case the ordinary draw returns and samples it anyway.
    pub(super) fn present_sampling(&self, outputs: &[OutputId]) -> LivePresentSampling {
        match &self.policy_presentation {
            Some(presentation)
                if !outputs.is_empty()
                    && outputs
                        .iter()
                        .all(|output| presentation.replaces_applications(*output)) =>
            {
                LivePresentSampling::ReplacedByPolicy
            }
            _ => LivePresentSampling::Required,
        }
    }

    /// Whether a WM presentation hides `surface` on `output`: it replaces
    /// applications there and draws no preview of the surface. A first
    /// Present parked outside the head frames is not released by the
    /// surface's own geometry on such an output, or it would be released and
    /// re-parked every service pass without its budget ever expiring (t246).
    pub(super) fn surface_hidden_by_policy(&self, surface: SurfaceId, output: OutputId) -> bool {
        self.policy_presentation
            .as_ref()
            .is_some_and(|presentation| {
                presentation.replaces_applications(output)
                    && !presentation
                        .presentation
                        .instances
                        .iter()
                        .any(|instance| instance.output == output && instance.source == surface)
            })
    }

    pub fn policy_presentation(&self) -> Option<&LivePolicyPresentation> {
        self.policy_presentation.as_ref()
    }

    /// Revokes the whole presentation when a source it samples was removed
    /// from the scene, and records the revocation for its owner.
    pub(super) fn revoke_policy_presentation_for_removed(&mut self, removed: &[SurfaceId]) {
        let Some(source) = self.policy_presentation.as_ref().and_then(|presentation| {
            presentation
                .sources()
                .find(|source| removed.contains(source))
        }) else {
            return;
        };
        let revoked = self
            .policy_presentation
            .take()
            .expect("a presentation named the removed source");
        self.policy_presentation_revocation = Some(LivePolicyPresentationRevocation {
            owner_epoch: revoked.owner_epoch,
            generation: revoked.presentation.generation,
            source,
        });
    }

    /// The last revocation, once: what presented input and the WM are told.
    pub fn take_policy_presentation_revocation(
        &mut self,
    ) -> Option<LivePolicyPresentationRevocation> {
        self.policy_presentation_revocation.take()
    }

    /// The surfaces a frame samples: the presentation order, then every
    /// instance source it does not already name. A preview-only source is
    /// sampled like any other; it is not added to the presentation order, so
    /// it is neither presented at its own placement nor an input layer.
    pub(super) fn sampled_surface_order(&self) -> Vec<SurfaceId> {
        let mut order = self.presentation_order.clone();
        if let Some(presentation) = &self.policy_presentation {
            for source in presentation.sources() {
                if !order.contains(&source) {
                    order.push(source);
                }
            }
        }
        order
    }
}

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

impl LivePolicyPresentation {
    /// This output's instances as display commands in z order. The source
    /// generation is left for [`resolve_surface_instance_sources`], so a
    /// value the WM could influence never reaches a frame.
    pub(super) fn instance_commands(&self, output: OutputId) -> Vec<CompositorDisplayCommand> {
        let mut instances = self
            .presentation
            .instances
            .iter()
            .filter(|instance| instance.output == output)
            .collect::<Vec<_>>();
        instances.sort_by_key(|instance| instance.z_index);
        instances
            .into_iter()
            .map(|instance| {
                CompositorDisplayCommand::SurfaceInstance(CompositorSurfaceInstance {
                    owner_epoch: self.owner_epoch,
                    id: instance.id,
                    generation: instance.generation,
                    source: instance.source,
                    source_generation: 0,
                    destination: instance.destination,
                    clip: instance.clip,
                    opacity_millis: instance.opacity_millis,
                })
            })
            .collect()
    }

    fn sources(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.presentation
            .instances
            .iter()
            .map(|instance| instance.source)
    }
}

impl LiveProductionVisualRuntime {
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
        // Every source must be committed both in the scene now displayed and
        // in the committed set the next frame draws; otherwise the whole
        // candidate is refused and the last valid presentation stays.
        if let Some(candidate) = &presentation
            && let Some(source) = candidate.sources().find(|source| {
                ![self.displayed_surface_view(), self.committed_surfaces()]
                    .iter()
                    .all(|scene| scene.iter().any(|state| state.surface == *source))
            })
        {
            return Err(Box::new(LivePolicyPresentationRefusal::MissingSource {
                source,
            }));
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

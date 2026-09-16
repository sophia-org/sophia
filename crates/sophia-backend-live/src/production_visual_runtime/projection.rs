use super::*;

mod content;
mod retirement;
pub(super) use content::presented_content_list_matches;
use content::{
    content_binding_from_frame, presented_content_matches, retain_presented_content_binding,
    same_content_binding,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct LiveSurfaceProjectionMetadata {
    namespace: Option<NamespaceId>,
}

impl LiveProductionVisualRuntime {
    /// Blocks ordinary and retained repaints while another frame owns retirement.
    /// Replacing that frame, even with identical pixels, would strand its proof.
    pub(super) fn native_publication_blocked(&self) -> bool {
        self.native_suspended
            || self.present_scheduler.has_in_flight()
            || !self.software_present_frames_bound.is_empty()
    }

    pub(super) fn observe_surface_metadata(
        &mut self,
        transactions: &[SurfaceTransaction],
        removed_surfaces: &[SurfaceId],
    ) {
        self.surface_metadata
            .retain(|surface, _| !removed_surfaces.contains(surface));
        for transaction in transactions {
            self.surface_metadata.insert(
                transaction.surface,
                LiveSurfaceProjectionMetadata {
                    namespace: transaction.namespace,
                },
            );
        }
    }

    /// Drops input layers for surfaces that have left the presentation order.
    ///
    /// The published projection is a record of retired pixels, and on the
    /// native path nothing rebuilds it until the next accepted page flip. A
    /// window unmapped between two flips would therefore keep answering the
    /// pointer until one arrives. This removes it on the cycle its layout
    /// changed instead, and bumps the epoch so consumers holding a route see
    /// the projection move.
    ///
    /// Removal only. Nothing is added, so this can never route input to pixels
    /// that have not reached scanout.
    pub(super) fn prune_input_projections_to_presentation_order(&mut self) {
        let eligible: BTreeSet<SurfaceId> = self.presentation_order.iter().copied().collect();
        for projection in &mut self.input_projections {
            let before = projection.layers.len();
            projection
                .layers
                .retain(|layer| eligible.contains(&layer.surface));
            if projection.layers.len() != before {
                projection.epoch = projection
                    .epoch
                    .checked_add(1)
                    .expect("presented input epoch exhausted");
            }
        }
    }

    /// Publishes the committed scene for runtimes whose output tick is also
    /// their presentation boundary (currently the non-native/headless path).
    pub(super) fn publish_committed_input_layers(&mut self) {
        if self.native_suspended {
            return;
        }
        let input_layers = input_layer_snapshots(
            self.production.committed_surfaces(),
            &self.presentation_order,
            &self.surface_metadata,
        );
        for index in 0..self.input_projections.len() {
            let Some(output) = self.outputs.output_id(index) else {
                continue;
            };
            if let Some(bounds) = self.outputs.logical_viewport(output)
                && let Ok(list) = self.display_list_for_output(
                    output,
                    bounds,
                    self.production.committed_surfaces(),
                    &self.presentation_order,
                )
            {
                self.tab_frames.insert(output, list.into());
            }
            let (descriptor_targets, descriptor_occlusion) = direct_descriptor_projection(
                self.descriptor_overlay.as_ref(),
                output,
                self.descriptor_overlay_interactive,
            );
            self.replace_presented_input_projection(
                index,
                input_layers.clone(),
                Vec::new(),
                None,
                descriptor_targets,
                descriptor_occlusion,
                self.shell_content.get(&output).map(|frame| {
                    content_binding_from_frame(
                        frame,
                        self.outputs.logical_viewport(output).unwrap_or_default(),
                        self.content_layout_generation,
                    )
                }),
            );
        }
        tracing::trace!(
            committed_scene_surfaces = self.production.committed_surfaces().len(),
            input_layers = input_layers.len(),
            "rebuilt input projection from committed scene"
        );
    }

    /// Publishes only pixels whose native frame has crossed an accepted page
    /// flip. Each output keeps its own snapshot and semantic epoch so one
    /// head's retirement cannot publish or invalidate another head's input.
    pub(super) fn publish_presented_input_layers<T: NativeCompositionTarget>(
        &mut self,
        native_scanout: &T,
    ) {
        for index in 0..self.outputs.output_count() {
            let Some(output) = self.outputs.output_id(index) else {
                continue;
            };
            if let Some(frame) = native_scanout.presented_frame(output) {
                self.tab_frames
                    .insert(output, frame.compositor_display_list.clone());
            } else {
                self.tab_frames.remove(&output);
            }
            let (
                input_layers,
                chrome_targets,
                chrome_occlusion,
                descriptor_targets,
                descriptor_occlusion,
                content,
                presented_scene_surfaces,
            ) = native_scanout.presented_frame(output).map_or_else(
                || (Vec::new(), Vec::new(), None, Vec::new(), None, None, 0),
                |presented| {
                    let logical_viewport =
                        self.outputs.logical_viewport(output).unwrap_or_default();
                    let (chrome_targets, chrome_occlusion) =
                        presented_chrome_projection(presented, logical_viewport);
                    let (descriptor_targets, descriptor_occlusion) =
                        presented_descriptor_projection(
                            presented,
                            self.descriptor_overlay.as_ref(),
                            output,
                            self.descriptor_overlay_interactive,
                        );
                    (
                        presented_input_layer_snapshots(
                            presented,
                            &self.surface_metadata,
                            &self.presentation_order,
                        ),
                        chrome_targets,
                        chrome_occlusion,
                        descriptor_targets,
                        descriptor_occlusion,
                        self.shell_content.get(&output).and_then(|frame| {
                            if presented_content_matches(presented, &frame.frame) {
                                Some(content_binding_from_frame(
                                    frame,
                                    logical_viewport,
                                    self.content_layout_generation,
                                ))
                            } else {
                                retain_presented_content_binding(
                                    presented,
                                    frame,
                                    self.input_projections[index].content.as_ref(),
                                    logical_viewport,
                                    self.content_layout_generation,
                                )
                            }
                        }),
                        presented.surfaces.len(),
                    )
                },
            );
            self.replace_presented_input_projection(
                index,
                input_layers,
                chrome_targets,
                chrome_occlusion,
                descriptor_targets,
                descriptor_occlusion,
                content,
            );
            tracing::trace!(
                output = output.raw(),
                presented_scene_surfaces,
                input_layers = self.input_projections[index].layers.len(),
                "published output-local input projection from retired native frame"
            );
        }
    }

    fn replace_presented_input_projection(
        &mut self,
        index: usize,
        input_layers: Vec<LayerSnapshot>,
        chrome_targets: Vec<sophia_engine::IndicatorChromeHitTarget>,
        chrome_occlusion: Option<Rect>,
        descriptor_targets: Vec<sophia_engine::PresentedChromeTarget>,
        descriptor_occlusion: Option<Rect>,
        mut content: Option<sophia_engine::PresentedContentBinding>,
    ) {
        let Some(output) = self.input_projections.get(index).map(|p| p.output) else {
            return;
        };
        let descriptor_projection = self.tab_frames.get(&output).and_then(|frame| {
            frame.rects().find_map(|rect| match rect.node {
                sophia_engine::CompositorNodeId::DescriptorOverlay {
                    projection,
                    slot: u16::MAX,
                    role: sophia_engine::DescriptorOverlayNodeRole::Panel,
                } => Some(projection),
                _ => None,
            })
        });
        let mut descriptor_targets = descriptor_targets;
        let mut tab_occlusions = Vec::new();
        if let Some(frame) = self.tab_frames.get(&output) {
            for rect in frame.rects() {
                if let sophia_engine::CompositorNodeId::TabBar { group, .. } = rect.node {
                    tab_occlusions.push(rect.geometry);
                    if let Some(bar) = self.tab_bars.iter().find(|b| {
                        b.output == output
                            && b.group == group
                            && b.generation == rect.generation
                            && self
                                .indicator_publication
                                .as_ref()
                                .is_some_and(|p| p.tab_groups.contains(&b.policy))
                    }) {
                        descriptor_targets.extend(
                            bar.targets
                                .iter()
                                .filter(|t| {
                                    t.geometry == rect.geometry
                                        && !input_layers.iter().any(|l| {
                                            sophia_engine::tab_rects_overlap(l.geometry, t.geometry)
                                        })
                                })
                                .cloned(),
                        );
                    }
                }
            }
        }
        let Some(projection) = self.input_projections.get_mut(index) else {
            return;
        };
        if !same_interaction_projection(&projection.layers, &input_layers)
            || projection.chrome_targets != chrome_targets
            || projection.chrome_occlusion != chrome_occlusion
            || projection.descriptor_targets != descriptor_targets
            || projection.descriptor_occlusion != descriptor_occlusion
            || projection.descriptor_projection != descriptor_projection
            || projection.tab_occlusions != tab_occlusions
            || !same_content_binding(projection.content.as_ref(), content.as_ref())
        {
            projection.epoch = projection
                .epoch
                .checked_add(1)
                .expect("presented input epoch exhausted");
        }
        projection.layers = input_layers;
        projection.chrome_targets = chrome_targets;
        projection.chrome_occlusion = chrome_occlusion;
        projection.descriptor_targets = descriptor_targets;
        projection.descriptor_occlusion = descriptor_occlusion;
        projection.descriptor_projection = descriptor_projection;
        projection.tab_occlusions = tab_occlusions;
        if let Some(binding) = &mut content {
            sophia_engine::reconcile_content_continuity(projection.content.as_ref(), binding);
            binding.presentation_epoch = projection.epoch.max(1);
            for target in &mut binding.targets {
                target.presentation_epoch = binding.presentation_epoch;
            }
        }
        projection.content = content;
    }

    pub(super) fn compositor_layer_templates(&self) -> Vec<LayerSnapshot> {
        committed_layer_snapshots(self.production.committed_surfaces(), &self.surface_metadata)
    }

    /// Both views of the scene, from one read.
    ///
    /// The engine pairs a tick's committed surfaces against its layer templates
    /// by `SurfaceId` and fails closed on a committed surface with no template.
    /// Templates travel in the tick input while the committed list is cloned from
    /// the per-output assembly, so any site that captures them at different
    /// moments -- or from different coordinators -- can violate that invariant.
    /// Building the templates FROM the committed slice makes the two set-equal by
    /// construction, which turns a discipline every tick site had to remember
    /// into a property none of them can break.
    pub(super) fn scene_views(&self) -> (Vec<LayerSnapshot>, Vec<CommittedSurfaceState>) {
        let committed = self.production.committed_surfaces().to_vec();
        let templates = committed_layer_snapshots(&committed, &self.surface_metadata);
        (templates, committed)
    }
}

fn direct_descriptor_projection(
    overlay: Option<&sophia_engine::DescriptorOverlayProjection>,
    output: OutputId,
    interactive: bool,
) -> (Vec<sophia_engine::PresentedChromeTarget>, Option<Rect>) {
    overlay
        .filter(|overlay| overlay.output == output)
        .map_or_else(
            || (Vec::new(), None),
            |overlay| {
                (
                    if interactive {
                        overlay.targets.clone()
                    } else {
                        Default::default()
                    },
                    Some(overlay.geometry),
                )
            },
        )
}

fn presented_descriptor_projection(
    presented: &OutputFrameDamageSnapshot,
    overlay: Option<&sophia_engine::DescriptorOverlayProjection>,
    output: OutputId,
    interactive: bool,
) -> (Vec<sophia_engine::PresentedChromeTarget>, Option<Rect>) {
    let presented_identity =
        presented
            .compositor_display_list
            .commands
            .iter()
            .find_map(|command| match command {
                sophia_engine::CompositorDisplayCommand::Rect(rect) => match rect.node {
                    sophia_engine::CompositorNodeId::DescriptorOverlay {
                        projection,
                        slot: u16::MAX,
                        role: sophia_engine::DescriptorOverlayNodeRole::Panel,
                    } => Some((projection, rect.geometry)),
                    _ => None,
                },
                _ => None,
            });
    let Some((projection, geometry)) = presented_identity else {
        return (Vec::new(), None);
    };
    let targets = overlay
        .filter(|overlay| {
            interactive
                && overlay.output == output
                && overlay.commands.iter().any(|command| {
                    matches!(
                        command,
                        sophia_engine::CompositorDisplayCommand::Rect(rect)
                            if matches!(
                                rect.node,
                                sophia_engine::CompositorNodeId::DescriptorOverlay {
                                    projection: candidate,
                                    slot: u16::MAX,
                                    role: sophia_engine::DescriptorOverlayNodeRole::Panel,
                                } if candidate == projection
                            )
                    )
                })
        })
        .map(|overlay| overlay.targets.clone())
        .unwrap_or_default();
    (targets, Some(geometry))
}

fn presented_chrome_projection(
    presented: &OutputFrameDamageSnapshot,
    logical_viewport: Rect,
) -> (Vec<sophia_engine::IndicatorChromeHitTarget>, Option<Rect>) {
    let Some(strip) = presented.compositor_display_list.indicator_strips().last() else {
        return (Vec::new(), None);
    };
    if strip.strip.geometry.is_empty() || logical_viewport.is_empty() {
        return (Vec::new(), None);
    }
    let logical_strip = Rect {
        x: logical_viewport.x,
        y: logical_viewport.y,
        width: logical_viewport.width,
        height: sophia_engine::INDICATOR_STRIP_HEIGHT.min(logical_viewport.height),
    };
    let targets = sophia_engine::project_indicator_chrome_targets(
        &strip.strip.hit_targets,
        strip.strip.geometry,
        logical_strip,
    );
    (targets, Some(logical_strip))
}

fn same_interaction_projection(previous: &[LayerSnapshot], next: &[LayerSnapshot]) -> bool {
    previous.len() == next.len()
        && previous.iter().zip(next).all(|(previous, next)| {
            previous.surface == next.surface
                && previous.namespace == next.namespace
                && previous.stack_rank == next.stack_rank
                && previous.geometry == next.geometry
                && previous.transform == next.transform
                && (previous.opacity > 0.0) == (next.opacity > 0.0)
                && (previous.source != BufferSource::None) == (next.source != BufferSource::None)
        })
}

pub(super) fn committed_layer_snapshots(
    committed: &[CommittedSurfaceState],
    metadata: &BTreeMap<SurfaceId, LiveSurfaceProjectionMetadata>,
) -> Vec<LayerSnapshot> {
    committed
        .iter()
        .enumerate()
        .map(|(index, state)| layer_snapshot(index, state, metadata.get(&state.surface)))
        .collect()
}

fn input_layer_snapshots(
    committed: &[CommittedSurfaceState],
    presentation_order: &[SurfaceId],
    metadata: &BTreeMap<SurfaceId, LiveSurfaceProjectionMetadata>,
) -> Vec<LayerSnapshot> {
    presentation_order
        .iter()
        .enumerate()
        .filter_map(|(index, surface)| {
            let state = committed.iter().find(|state| state.surface == *surface)?;
            Some(layer_snapshot(index, state, metadata.get(surface)))
        })
        .collect()
}

/// The layers a retired frame's pixels may route pointer input to.
///
/// Two conditions, both required. The surface's pixels must have reached
/// scanout, which is what this projection is for -- input goes to what is
/// actually on screen, never to something composed but not yet flipped. And
/// the surface must still be in the presentation order the session last
/// published, because a retired frame is a record of the past: a window
/// unmapped this cycle keeps its pixels on screen until the next flip retires,
/// and without this filter it keeps answering the pointer for that whole
/// interval.
///
/// The filter only ever removes. Nothing absent from the retired frame is
/// added, so this cannot route input to pixels a viewer cannot see. That
/// direction is the one `committed_layer_snapshots` must never filter, and
/// this is a different projection with the opposite obligation.
fn presented_input_layer_snapshots(
    presented: &OutputFrameDamageSnapshot,
    metadata: &BTreeMap<SurfaceId, LiveSurfaceProjectionMetadata>,
    presentation_order: &[SurfaceId],
) -> Vec<LayerSnapshot> {
    // Membership is built once rather than scanned per surface: both sides are
    // bounded by MAX_OUTPUT_FRAME_SURFACES (1024), so a linear scan per entry
    // is a million comparisons on a full frame. Retirement is not the pointer
    // path, but it is not the place to add that either.
    let eligible: BTreeSet<SurfaceId> = presentation_order.iter().copied().collect();
    presented
        .surfaces
        .iter()
        .filter(|state| eligible.contains(&state.surface))
        // Ranks are assigned after filtering, so they stay contiguous and keep
        // the retired frame's relative order. This matches the committed path,
        // which also enumerates what survives its own filter.
        .enumerate()
        .map(|(index, state)| LayerSnapshot {
            translation: None,
            // NOTE: this is wrong and the comment that used to sit here said
            // the opposite -- that input routing reads layers carrying the
            // region. It does not: physical_input_phase.rs takes
            // `runtime.input_layers()`, which is exactly this projection, so a
            // shaped window's input region never reaches the hit test and
            // SHAPE input shapes cannot take effect on this path. Left as it
            // was rather than widened here, because carrying the region needs
            // a source for it in the retired frame and that is its own change.
            input_region: None,
            surface: state.surface,
            authority_local_id: None,
            // Rebuilt from what the Engine committed, which records pixels
            // rather than which policy placed them. Ownership travels with the
            // proposal, not with a read-back.
            output: None,
            namespace: metadata
                .get(&state.surface)
                .and_then(|metadata| metadata.namespace),
            stack_rank: u32::try_from(index).unwrap_or(u32::MAX),
            // Pointer coordinates are desktop-wide. Native damage geometry
            // loses the output origin and scale and cannot serve as input geometry.
            geometry: state.logical_geometry,
            source: state.buffer,
            source_size: state.source_size,
            damage: Region::default(),
            opacity: 1.0,
            crop: None,
            transform: Transform::IDENTITY,
            generation: state.committed_generation,
            resize_sync: ResizeSyncCapability::ImplicitOnly,
        })
        .collect()
}

fn layer_snapshot(
    index: usize,
    state: &CommittedSurfaceState,
    metadata: Option<&LiveSurfaceProjectionMetadata>,
) -> LayerSnapshot {
    LayerSnapshot {
        translation: None,
        // Same correction as the presented constructor: these layers are what
        // physical_input_phase.rs hands the hit test, so this projection does
        // answer the pointer and dropping the region means SHAPE input shapes
        // do not take effect through it.
        input_region: None,
        surface: state.surface,
        authority_local_id: None,
        // As above: a read-back of committed pixels names no owner.
        output: None,
        namespace: metadata.and_then(|metadata| metadata.namespace),
        stack_rank: u32::try_from(index).unwrap_or(u32::MAX),
        geometry: state.geometry,
        // The committed record measured this raster; the geometry is only
        // where it was placed, and the two part company during a resize.
        source: state.buffer(),
        source_size: state.content.canonical_variant().pixel_size,
        damage: state.damage.clone(),
        opacity: 1.0,
        crop: None,
        transform: Transform::IDENTITY,
        generation: state.committed_generation,
        resize_sync: ResizeSyncCapability::ImplicitOnly,
    }
}

#[cfg(test)]
#[path = "../../tests/support/presented_projection.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/support/reference_presentation.rs"]
mod reference_presentation_tests;

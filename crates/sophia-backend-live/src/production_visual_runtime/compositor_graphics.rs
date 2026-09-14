use super::*;

/// Head composition frames grouped by the output that will scan them out.
type HeadCompositionFramesByOutput =
    Vec<(OutputId, Vec<crate::LiveProductionHeadCompositionFrame>)>;

pub(super) struct LiveProductionRetainedCompositionSourceSet {
    pub committed: Vec<CommittedSurfaceState>,
    pub presentation_order: Vec<SurfaceId>,
    pub scene_generation: u64,
    pub sources: Vec<sophia_renderer_live::LiveOwnedHeadCompositionSource>,
}

fn cpu_variant_sources(
    surface: SurfaceId,
    cpu_layers: &[LiveCpuPresentationLayer],
) -> Vec<sophia_renderer_live::LiveOwnedHeadCompositionSource> {
    cpu_layers
        .iter()
        .filter(|layer| layer.surface == surface)
        .map(
            |layer| sophia_renderer_live::LiveOwnedHeadCompositionSource {
                surface,
                source: BufferSource::CpuBuffer {
                    handle: layer.buffer.handle,
                },
                kind: sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::Cpu(
                    layer.buffer.clone().into(),
                ),
            },
        )
        .collect()
}

/// One retained layer as a composition source.
///
/// A composed frame's layer names a renderer image, because the flip that
/// displayed it promoted a compositor-owned snapshot. A *directly* scanned
/// frame has no snapshot -- the renderer never saw its buffer; that copy is
/// what direct scanout skips -- so its source must be the client's still-held
/// planes, which the compose then imports and captures. Emitting the image id
/// anyway asked the renderer for a picture nobody ever took, and the first
/// overlay over a direct frame died on it.
fn retained_layer_source(
    surface: SurfaceId,
    committed_source: BufferSource,
    displayed: &crate::LiveRetainedRendererImageLayer,
    direct: Option<sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame>,
) -> sophia_renderer_live::LiveOwnedHeadCompositionSource {
    let kind = match direct {
        Some(frame) => sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::DmaBuf {
            image_id: displayed.image_id,
            frame,
        },
        None => sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::RendererImage {
            image_id: displayed.image_id,
            size: displayed.size,
            format: displayed.format,
        },
    };
    sophia_renderer_live::LiveOwnedHeadCompositionSource {
        surface,
        source: committed_source,
        kind,
    }
}

pub(crate) fn retained_surface_sources(
    surface: SurfaceId,
    committed_source: BufferSource,
    cpu_layers: &[LiveCpuPresentationLayer],
    in_flight: Option<&crate::LiveRetainedRendererImageLayer>,
    in_flight_direct: Option<sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame>,
    retained: Option<&crate::LiveRetainedRendererImageLayer>,
    retained_direct: Option<sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame>,
) -> Result<Vec<sophia_renderer_live::LiveOwnedHeadCompositionSource>, &'static str> {
    let mut sources = Vec::new();
    if let Some(displayed) = in_flight {
        sources.push(retained_layer_source(
            surface,
            committed_source,
            displayed,
            in_flight_direct,
        ));
    }
    sources.extend(cpu_variant_sources(surface, cpu_layers));
    if !sources.is_empty() {
        return Ok(sources);
    }
    if let Some(displayed) = retained {
        if !matches!(committed_source, BufferSource::DmaBuf { .. }) {
            return Err("retained renderer image lost its DMA-BUF identity");
        }
        sources.push(retained_layer_source(
            surface,
            committed_source,
            displayed,
            retained_direct,
        ));
        return Ok(sources);
    }
    Err("retained head plan has no authority-owned source")
}

/// The sources a queued Present's plan requires, read from the candidate it plans.
///
/// The caller passes the same `candidate` slice it builds its display lists from, so
/// plan and sources cannot disagree about which surfaces exist or which buffer each
/// one commits. A Present used to carry CPU layers captured when it was enqueued; a
/// Present parked behind a layout epoch then composed a rebased scene against a
/// snapshot predating every surface admitted while it waited, and the missing source
/// surfaced as a lowering failure rather than as the skew it was.
///
/// The presenting surface contributes the frame just composed for it plus any CPU
/// variants the candidate still carries, because a head may select a retained raster
/// for it rather than the DMA-BUF being presented.
/// Sources cover the union of the output display lists that will be lowered. Using
/// only the primary output's list loses every surface owned by another output.
pub fn live_present_head_composition_sources<'a, 'b>(
    presenting_surface: SurfaceId,
    current_source: sophia_renderer_live::LiveOwnedHeadCompositionSource,
    candidate: &[CommittedSurfaceState],
    display_lists: impl IntoIterator<Item = &'b CompositorDisplayList>,
    cpu_layers: &[LiveCpuPresentationLayer],
    retained: impl Fn(SurfaceId) -> Option<&'a crate::LiveRetainedRendererImageLayer>,
    retained_direct: impl Fn(SurfaceId) -> Option<sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame>,
) -> Result<Vec<sophia_renderer_live::LiveOwnedHeadCompositionSource>, Box<dyn std::error::Error>> {
    let mut current_source = Some(current_source);
    let mut sources = Vec::new();
    let mut seen = BTreeSet::new();
    for command in display_lists.into_iter().flat_map(|list| &list.commands) {
        let CompositorDisplayCommand::Surface { surface } = command else {
            continue;
        };
        // Heads share retained sources; resolve each surface once across outputs.
        if !seen.insert(*surface) {
            continue;
        }
        // Presentation order names policy's surfaces; the planner keeps only those the
        // candidate has committed. A surface policy ordered before its pixels arrived
        // is dropped there, so it owes no source here.
        let Some(committed_source) = candidate
            .iter()
            .find(|state| state.surface == *surface)
            .map(CommittedSurfaceState::buffer)
        else {
            continue;
        };
        if *surface == presenting_surface {
            sources.push(
                current_source
                    .take()
                    .ok_or("current Present appeared twice in the layout")?,
            );
            sources.extend(cpu_variant_sources(*surface, cpu_layers));
            continue;
        }
        sources.extend(retained_surface_sources(
            *surface,
            committed_source,
            cpu_layers,
            // A Present reaches submission only while no other one is in flight, so
            // this candidate is the only one that can own a renderer image here.
            None,
            None,
            retained(*surface),
            retained_direct(*surface),
        )?);
    }
    if current_source.is_some() {
        return Err("visible Present surface is missing from the presentation order".into());
    }
    Ok(sources)
}

/// Policy ownership takes precedence over frontend geometry routing. An unknown
/// surface is never made visible merely because it overlaps an output.
pub fn live_surface_routes_to_output(
    surface: SurfaceId,
    surface_outputs: &BTreeMap<SurfaceId, OutputId>,
    geometry_routed: &BTreeSet<SurfaceId>,
    output: OutputId,
) -> bool {
    match surface_outputs.get(&surface) {
        Some(owner) => *owner == output,
        None => geometry_routed.contains(&surface),
    }
}

/// The surfaces one head composites: those whose projection placed them on it.
///
/// Geometry still decides how much of a surface a head shows -- a column half
/// past the edge shows its visible half -- but it does not decide which head
/// shows it. Selecting by rectangle instead drew one display's window on the
/// display beside it, because a scrolling strip runs past its own edge on
/// purpose and the neighbour's rectangle starts there.
///
/// This selector covers policy-managed surfaces. Frontend-positioned surfaces
/// use the explicit geometry route in `live_surface_routes_to_output`; they
/// intentionally have no WM placement.
pub fn live_surfaces_owned_by_output(
    presentation_order: &[SurfaceId],
    surface_outputs: &BTreeMap<SurfaceId, OutputId>,
    output: OutputId,
) -> Vec<SurfaceId> {
    presentation_order
        .iter()
        .copied()
        .filter(|surface| {
            surface_outputs
                .get(surface)
                .is_some_and(|owner| *owner == output)
        })
        .collect()
}

impl LiveProductionVisualRuntime {
    pub(super) fn cpu_output_head_composition_frames_from_layers(
        &self,
        native_scanout: &LiveProductionNativeScanout,
        cpu_layers: &[LiveCpuPresentationLayer],
        scene_generation: u64,
    ) -> Result<HeadCompositionFramesByOutput, Box<dyn std::error::Error>> {
        let committed = self.production.committed_surfaces();
        // One source per layer per head, and each carries the client's pixels.
        // The clone is a refcount bump: the registry, this frame, and every
        // other head share one allocation, and the client's next patch copies
        // it only while these still read it. It used to copy the whole buffer
        // here, then again inside the conversion.
        let sources = cpu_layers
            .iter()
            .map(
                |source| sophia_renderer_live::LiveOwnedHeadCompositionSource {
                    surface: source.surface,
                    source: BufferSource::CpuBuffer {
                        handle: source.buffer.handle,
                    },
                    kind: sophia_renderer_live::LiveOwnedHeadCompositionSourceKind::Cpu(
                        source.buffer.clone().into(),
                    ),
                },
            )
            .collect::<Vec<_>>();
        self.outputs
            .logical_viewports()
            .map(|(output, logical_viewport)| {
                let display_list = self.display_list_for_output(
                    output,
                    logical_viewport,
                    committed,
                    &self.presentation_order,
                )?;
                Ok((
                    output,
                    self.compose_native_head_frames_from_sources(
                        native_scanout,
                        output,
                        committed,
                        display_list,
                        scene_generation.max(1),
                        &sources,
                    )?,
                ))
            })
            .collect()
    }

    pub(super) fn compose_native_head_frames_from_sources(
        &self,
        native_scanout: &LiveProductionNativeScanout,
        output: OutputId,
        committed: &[CommittedSurfaceState],
        display_list: CompositorDisplayList,
        scene_generation: u64,
        sources: &[sophia_renderer_live::LiveOwnedHeadCompositionSource],
    ) -> Result<Vec<crate::LiveProductionHeadCompositionFrame>, Box<dyn std::error::Error>> {
        let logical_viewport = self
            .outputs
            .logical_viewport(output)
            .ok_or("head composition targets an unknown logical output")?;
        let (presented, display_list) =
            self.translations
                .project(output, committed, display_list, self.translation_time());
        let snapshot = sophia_engine::output_scene_snapshot_from_committed_in_view(
            output,
            scene_generation.max(1),
            logical_viewport,
            &presented,
            display_list,
            None,
        )?;
        let targets = native_scanout.head_render_targets(output);
        let plans = sophia_engine::build_output_head_plans(&snapshot, &targets)?;
        if plans.len() != targets.len() {
            return Err("head composition planner returned partial target coverage".into());
        }
        for plan in &plans {
            trace_live_head_composition_plan(plan);
        }
        plans
            .iter()
            .map(|plan| {
                Ok(crate::LiveProductionHeadCompositionFrame {
                    head: plan.head,
                    scene_generation: plan.scene_generation,
                    target_generation: plan.target_generation,
                    mapping: plan.mapping,
                    logical_content_checksum: plan.logical_content_checksum,
                    frame: sophia_renderer_live::lower_head_composition_plan_with_caches(
                        plan,
                        sources,
                        &mut self.indicator_strip_cache.borrow_mut(),
                        &mut self.text_cache.borrow_mut(),
                    )?,
                })
            })
            .collect()
    }

    /// The in-flight submission's transaction, when that submission put a
    /// client's buffer on the plane directly.
    fn in_flight_direct(
        &self,
        native_scanout: &LiveProductionNativeScanout,
    ) -> Option<TransactionId> {
        native_scanout
            .heads
            .iter()
            .any(|head| head.submitted_direct)
            .then(|| self.present_scheduler.in_flight_transaction())
            .flatten()
    }

    /// The client's still-held planes for a *displayed* direct present, keyed
    /// by the image id its retained layer carries.
    ///
    /// `None` for every composed frame, whose retained image the renderer
    /// really holds. A direct frame's image was never imported -- the copy is
    /// what direct scanout skips -- so composing over it must source the
    /// client's buffer, which is still owed to the client and therefore still
    /// held here.
    pub(super) fn displayed_direct_frame(
        &self,
        image_id: sophia_renderer_live::LiveRendererImageId,
    ) -> Option<sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame> {
        let transaction = crate::presentation::present_for_renderer_image(image_id);
        if !self
            .displayed_direct_presents
            .values()
            .any(|displayed| *displayed == transaction)
        {
            return None;
        }
        self.cloned_direct_frame(transaction)
    }

    fn cloned_direct_frame(
        &self,
        transaction: TransactionId,
    ) -> Option<sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame> {
        match self
            .presentation_feedback
            .resources()
            .try_clone_submitted_dma_buf(transaction)
        {
            Ok(frame) => Some(frame),
            Err(error) => {
                // The compose that follows will refuse the layer, which is the
                // same failure this path exists to prevent -- but with the
                // reason on record instead of an image id nobody imported.
                tracing::warn!(
                    transaction = transaction.raw(),
                    ?error,
                    "a displayed direct present could not re-offer its planes"
                );
                None
            }
        }
    }

    pub(super) fn retained_composition_source_set(
        &self,
        scene: &LiveProductionCpuScene,
        in_flight_direct_transaction: Option<TransactionId>,
    ) -> Result<LiveProductionRetainedCompositionSourceSet, Box<dyn std::error::Error>> {
        let committed = self
            .present_scheduler
            .in_flight_candidate()
            .unwrap_or_else(|| self.production.committed_surfaces())
            .to_vec();
        let retained_order =
            live_production_retained_surface_order(&self.presentation_order, &committed);
        let display_lists = self
            .outputs
            .logical_viewports()
            .map(|(output, viewport)| {
                self.display_list_for_output(output, viewport, &committed, &retained_order)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let cpu_layers = scene.presentation_variant_layers(&committed, &retained_order);
        let in_flight = self.present_scheduler.in_flight_displayed_layer();
        let mut sources = Vec::new();
        let mut seen = BTreeSet::new();
        for command in display_lists.iter().flat_map(|list| &list.commands) {
            let CompositorDisplayCommand::Surface { surface } = command else {
                continue;
            };
            if !seen.insert(*surface) {
                continue;
            }
            let committed_source = committed
                .iter()
                .find(|state| state.surface == *surface)
                .map(CommittedSurfaceState::buffer)
                .ok_or("retained display list escaped committed Engine membership")?;
            let in_flight = in_flight
                .filter(|(in_flight_surface, _)| *in_flight_surface == *surface)
                .map(|(_, displayed)| displayed);
            let retained = self
                .displayed_surfaces
                .get(surface)
                .map(|displayed| &displayed.layer);
            let in_flight_direct = in_flight
                .filter(|_| in_flight_direct_transaction.is_some())
                .and_then(|_| {
                    self.cloned_direct_frame(in_flight_direct_transaction.expect("filtered above"))
                });
            let retained_direct =
                retained.and_then(|displayed| self.displayed_direct_frame(displayed.image_id));
            sources.extend(retained_surface_sources(
                *surface,
                committed_source,
                &cpu_layers,
                in_flight,
                in_flight_direct,
                retained,
                retained_direct,
            )?);
        }
        let scene_generation = committed
            .iter()
            .map(|state| state.committed_generation)
            .max()
            .unwrap_or(1);
        Ok(LiveProductionRetainedCompositionSourceSet {
            committed,
            presentation_order: retained_order,
            scene_generation,
            sources,
        })
    }

    pub(super) fn retained_output_head_composition_frames_from_sources(
        &self,
        native_scanout: &LiveProductionNativeScanout,
        source_set: &LiveProductionRetainedCompositionSourceSet,
    ) -> Result<HeadCompositionFramesByOutput, Box<dyn std::error::Error>> {
        self.outputs
            .logical_viewports()
            .map(|(output, logical_viewport)| {
                let display_list = self.display_list_for_output(
                    output,
                    logical_viewport,
                    &source_set.committed,
                    &source_set.presentation_order,
                )?;
                Ok((
                    output,
                    self.compose_native_head_frames_from_sources(
                        native_scanout,
                        output,
                        &source_set.committed,
                        display_list,
                        source_set.scene_generation,
                        &source_set.sources,
                    )?,
                ))
            })
            .collect()
    }

    pub(super) fn retained_output_head_composition_frames(
        &self,
        scene: &LiveProductionCpuScene,
        native_scanout: &LiveProductionNativeScanout,
    ) -> Result<HeadCompositionFramesByOutput, Box<dyn std::error::Error>> {
        let source_set =
            self.retained_composition_source_set(scene, self.in_flight_direct(native_scanout))?;
        self.retained_output_head_composition_frames_from_sources(native_scanout, &source_set)
    }

    /// Lowers one immutable committed scene into candidate native-size frames
    /// for a provisional topology. CPU buffers and retained renderer images
    /// come from the ordinary authority-owned source set; committed DMA-BUF
    /// identities are not independently importable sources. This is read-only
    /// with respect to the live runtime: the caller must not publish or install
    /// the candidate until its KMS transaction and first-presentation barrier
    /// complete.
    pub fn compose_output_topology_head_frames(
        &self,
        scene: &LiveProductionCpuScene,
        resolved: &crate::LiveResolvedOutputTopology,
        scene_generation: u64,
    ) -> Result<Vec<crate::LiveProductionHeadCompositionFrame>, Box<dyn std::error::Error>> {
        if scene_generation == 0 {
            return Err("topology composition requires a valid scene generation".into());
        }
        // No submission accompanies a provisional topology, so nothing here can
        // be an in-flight direct frame; retained direct frames still resolve.
        let source_set = self.retained_composition_source_set(scene, None)?;
        let targets = resolved.head_render_targets();
        if targets.len() != resolved.targets.len() {
            return Err("topology render-target projection is incomplete".into());
        }
        let mut frames = Vec::with_capacity(targets.len());
        for viewport in &resolved.logical_viewports {
            let display_list = self.display_list_for_output(
                viewport.output,
                viewport.logical,
                &source_set.committed,
                &source_set.presentation_order,
            )?;
            let snapshot = sophia_engine::output_scene_snapshot_from_committed_in_view(
                viewport.output,
                scene_generation,
                viewport.logical,
                &source_set.committed,
                display_list,
                None,
            )?;
            let output_targets = targets
                .iter()
                .copied()
                .filter(|target| target.output == viewport.output)
                .collect::<Vec<_>>();
            let plans = sophia_engine::build_output_head_plans(&snapshot, &output_targets)?;
            for plan in &plans {
                frames.push(crate::LiveProductionHeadCompositionFrame {
                    head: plan.head,
                    scene_generation: plan.scene_generation,
                    target_generation: plan.target_generation,
                    mapping: plan.mapping,
                    logical_content_checksum: plan.logical_content_checksum,
                    frame: sophia_renderer_live::lower_head_composition_plan_with_caches(
                        plan,
                        &source_set.sources,
                        &mut self.indicator_strip_cache.borrow_mut(),
                        &mut self.text_cache.borrow_mut(),
                    )?,
                });
            }
        }
        if frames.len() != targets.len() {
            return Err("topology composition omitted an enabled head".into());
        }
        let actual = frames
            .iter()
            .map(|frame| frame.head)
            .collect::<BTreeSet<_>>();
        let expected = targets
            .iter()
            .map(|target| target.head)
            .collect::<BTreeSet<_>>();
        if actual != expected || actual.len() != frames.len() {
            return Err("topology composition repeated or targeted an unknown head".into());
        }
        Ok(frames)
    }

    pub(super) fn display_list(
        &self,
        committed_surfaces: &[CommittedSurfaceState],
        presentation_order: &[SurfaceId],
    ) -> Result<CompositorDisplayList, CompositorDisplayListError> {
        let output = self
            .outputs
            .primary_output()
            .ok_or(CompositorDisplayListError::InvalidOutput)?;
        let bounds = self
            .outputs
            .logical_viewport(output)
            .ok_or(CompositorDisplayListError::InvalidOutput)?;
        self.display_list_for_output(output, bounds, committed_surfaces, presentation_order)
    }

    pub(super) fn presentation_orders_by_output(&self) -> BTreeMap<OutputId, Vec<SurfaceId>> {
        self.outputs
            .logical_viewports()
            .map(|(output, _)| {
                (
                    output,
                    self.surface_order_for_output(output, &self.presentation_order),
                )
            })
            .collect()
    }

    fn surface_order_for_output(
        &self,
        output: OutputId,
        presentation_order: &[SurfaceId],
    ) -> Vec<SurfaceId> {
        // Frontend-positioned layers are clipped by the head plan. Managed
        // scrolling columns remain confined to their policy-assigned output.
        presentation_order
            .iter()
            .copied()
            .filter(|surface| {
                live_surface_routes_to_output(
                    *surface,
                    &self.surface_outputs,
                    &self.geometry_routed_surfaces,
                    output,
                )
            })
            .collect()
    }

    pub(super) fn display_list_for_output(
        &self,
        output: OutputId,
        _bounds: Rect,
        committed_surfaces: &[CommittedSurfaceState],
        presentation_order: &[SurfaceId],
    ) -> Result<CompositorDisplayList, CompositorDisplayListError> {
        let owned = self.surface_order_for_output(output, presentation_order);
        let mut display_list = surface_chrome_display_list_for_surfaces(
            output,
            &owned,
            &self.chrome_surfaces,
            committed_surfaces,
            self.focused_surface,
            self.surface_chrome_style,
        )?;
        if let Some(publication) = self.indicator_publication.as_ref() {
            sophia_engine::append_tab_bars(
                &mut display_list.commands,
                &publication.tab_groups,
                publication.generation,
                &self.tab_bars,
                output,
            );
        }
        if let Some(outline) = self.floating_outline {
            if display_list.commands.len() >= MAX_COMPOSITOR_DISPLAY_COMMANDS {
                return Err(CompositorDisplayListError::CapacityExceeded);
            }
            let border = compositor_floating_outline(
                outline.surface,
                outline.geometry,
                self.surface_chrome_style.focus_ring.width.max(2),
                self.surface_chrome_style.focus_ring.color,
            )
            .ok_or(CompositorDisplayListError::InvalidSurface)?;
            display_list
                .commands
                .push(CompositorDisplayCommand::Border(border));
        }
        if let Some(content) = self.shell_content.get(&output) {
            if display_list
                .commands
                .len()
                .saturating_add(content.images.len())
                > MAX_COMPOSITOR_DISPLAY_COMMANDS
            {
                return Err(CompositorDisplayListError::CapacityExceeded);
            }
            display_list.commands.extend(
                content
                    .images
                    .iter()
                    .cloned()
                    .map(CompositorDisplayCommand::ContentImage),
            );
        }
        if let Some(overlay) = self
            .descriptor_overlay
            .as_ref()
            .filter(|overlay| overlay.output == output)
        {
            if display_list
                .commands
                .len()
                .saturating_add(overlay.commands.len())
                > MAX_COMPOSITOR_DISPLAY_COMMANDS
            {
                return Err(CompositorDisplayListError::CapacityExceeded);
            }
            display_list
                .commands
                .extend(overlay.commands.iter().cloned());
        }
        Ok(display_list)
    }

    /// Installs one Engine-validated shell projection and queues a retained
    /// compositor repaint when native scanout owns presentation.
    pub fn set_tab_bars(
        &mut self,
        bars: Vec<sophia_engine::TabBarProjection>,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.tab_bars == bars {
            return Ok(false);
        }
        let previous = std::mem::replace(&mut self.tab_bars, bars);
        if let Some(native) = native_scanout {
            let queued = self.queue_retained_projection(scene, native);
            if let Err(e) = queued {
                self.tab_bars = previous;
                return Err(e);
            }
        } else {
            self.publish_committed_input_layers();
        }
        Ok(true)
    }

    pub fn tab_bars_presented(&self, bars: &[sophia_engine::TabBarProjection]) -> bool {
        if bars.is_empty() {
            return self.tab_frames.values().all(|f| {
                !f.rects()
                    .any(|r| matches!(r.node, sophia_engine::CompositorNodeId::TabBar { .. }))
            });
        }
        bars.iter().all(|bar| {
            self.tab_frames
                .get(&bar.output)
                .is_some_and(|frame| bar.commands.iter().all(|c| frame.commands.contains(c)))
        })
    }

    pub fn revoke_tab_interaction(&mut self) {
        for bar in &mut self.tab_bars {
            bar.targets.clear();
        }
        for p in &mut self.input_projections {
            let before = p.descriptor_targets.len();
            p.descriptor_targets
                .retain(|t| t.id.generation & (1 << 63) == 0);
            if before != p.descriptor_targets.len() {
                p.epoch = p.epoch.checked_add(1).expect("input epoch exhausted");
            }
        }
    }

    pub fn set_descriptor_overlay(
        &mut self,
        overlay: Option<sophia_engine::DescriptorOverlayProjection>,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.descriptor_overlay == overlay {
            return Ok(false);
        }
        let previous_overlay = self.descriptor_overlay.clone();
        let previous_interactive = self.descriptor_overlay_interactive;
        self.descriptor_overlay = overlay;
        self.descriptor_overlay_interactive = self.descriptor_overlay.is_some();
        if let Some(native_scanout) = native_scanout {
            let queued = self.queue_retained_projection(scene, native_scanout);
            if let Err(error) = queued {
                self.descriptor_overlay = previous_overlay;
                self.descriptor_overlay_interactive = previous_interactive;
                return Err(error);
            }
        }
        Ok(true)
    }

    /// Revokes input immediately without withdrawing already presented pixels.
    pub fn revoke_descriptor_overlay_interaction(&mut self) -> usize {
        self.descriptor_overlay_interactive = false;
        for bar in &mut self.tab_bars {
            bar.targets.clear();
        }
        let mut revoked = 0usize;
        for projection in &mut self.input_projections {
            revoked = revoked.saturating_add(projection.descriptor_targets.len());
            if !projection.descriptor_targets.is_empty() {
                projection.epoch = projection
                    .epoch
                    .checked_add(1)
                    .expect("presented input epoch exhausted");
                projection.descriptor_targets.clear();
            }
        }
        revoked
    }

    /// Returns the output-local presentation epoch only after the requested
    /// visible or withdrawn candidate has crossed the presentation boundary.
    pub fn descriptor_overlay_presentation_epoch(
        &self,
        output: OutputId,
        generation: u64,
        visible: bool,
    ) -> Option<u64> {
        let projection = self
            .input_projections
            .iter()
            .find(|projection| projection.output == output)?;
        let presented = if visible {
            self.descriptor_overlay.as_ref().is_some_and(|overlay| {
                overlay.output == output
                    && overlay.generation == generation
                    && overlay.commands.iter().any(|c| matches!(c,
                        sophia_engine::CompositorDisplayCommand::Rect(r) if matches!(r.node,
                            sophia_engine::CompositorNodeId::DescriptorOverlay { projection: id, slot: u16::MAX, role: sophia_engine::DescriptorOverlayNodeRole::Panel } if Some(id) == projection.descriptor_projection)))
                    && overlay
                        .targets
                        .iter()
                        .all(|t| projection.descriptor_targets.contains(t))
            })
        } else {
            self.descriptor_overlay.is_none() && projection.descriptor_occlusion.is_none()
        };
        presented.then_some(projection.epoch.max(1))
    }

    pub fn set_floating_outline(
        &mut self,
        outline: Option<LiveFloatingOutline>,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.floating_outline == outline {
            return Ok(false);
        }
        self.floating_outline = outline;
        if let Some(native_scanout) = native_scanout {
            self.queue_retained_projection(scene, native_scanout)?;
        }
        Ok(true)
    }

    pub(super) fn record_focus_ring_observation(
        &mut self,
        committed_surfaces: &[CommittedSurfaceState],
        force: bool,
    ) -> Result<(), CompositorDisplayListError> {
        let display_list = self.display_list(committed_surfaces, &self.presentation_order)?;
        if let Some(surface) = self.focused_surface {
            if let Some(border) = display_list.borders().find(|border| {
                matches!(
                    border.node,
                    CompositorNodeId::SurfaceChrome {
                        surface: border_surface,
                        role: SurfaceChromeRole::FocusRing,
                    } if border_surface == surface
                )
            }) {
                let observation = LiveFocusRingObservation {
                    surface,
                    generation: border.generation,
                    primitives: compositor_border_bands(border)
                        .into_iter()
                        .filter(|band| !band.geometry.is_empty())
                        .count(),
                };
                if observation.primitives > 0
                    && (force || self.last_focus_ring_observation != Some(observation))
                {
                    self.last_focus_ring_observation = Some(observation);
                    self.pending_focus_ring_observation = Some(observation);
                }
            } else {
                self.last_focus_ring_observation = None;
            }
        } else {
            self.last_focus_ring_observation = None;
        }
        let summary = compositor_chrome_summary(&display_list, self.focused_surface);
        let observation = LiveChromeSetObservation {
            generation: summary.generation,
            eligible_surfaces: self
                .chrome_surfaces
                .iter()
                .filter(|surface| self.presentation_order.contains(surface))
                .count(),
            frames: summary.frames,
            focused_frames: summary.focused_frames,
            unfocused_frames: summary.unfocused_frames,
            focus_rings: summary.focus_rings,
            primitives: summary.primitives,
            clearance: summary.clearance,
        };
        if self.last_chrome_set_observation != Some(observation) {
            self.last_chrome_set_observation = Some(observation);
            self.pending_chrome_set_observation = Some(observation);
        }
        Ok(())
    }

    pub fn take_focus_ring_observation(&mut self) -> Option<LiveFocusRingObservation> {
        self.pending_focus_ring_observation.take()
    }

    pub fn take_chrome_set_observation(&mut self) -> Option<LiveChromeSetObservation> {
        self.pending_chrome_set_observation.take()
    }
}

mod shell_content;
mod tests;

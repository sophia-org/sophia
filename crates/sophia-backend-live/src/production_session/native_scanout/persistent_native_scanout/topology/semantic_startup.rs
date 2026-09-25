impl LiveProductionNativeScanout {
    /// Establishes the first displayed generation of a logical output from
    /// independently lowered semantic frames.
    ///
    /// Renderer workers are enabled before any export. Every framebuffer and
    /// modeset property owner is then prepared without KMS mutation. Because a
    /// mirror group is card-local, one blocking card-scoped atomic commit makes
    /// the complete set visible; no head can expose a prepared prefix.
    pub(super) fn initialize_semantic_head_transaction(
        &mut self,
        output: OutputId,
        runtime: &mut crate::LiveBackendRuntimeAssembly,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
        let indices = self.head_indices(output);
        if indices.is_empty() {
            return Err("semantic startup requires at least one head".into());
        }
        if indices.iter().any(|index| {
            let custody = &self.heads[*index].scanout_custody;
            !custody.can_adopt_displayed() || custody.cleanup_pending()
        }) {
            return Err("semantic startup found pre-existing native head custody".into());
        }
        let singleton = indices.len() == 1;
        if singleton
            && (runtime.rendered_primary_plane_scanout_displayed()
                || runtime.rendered_primary_plane_scanout_in_flight()
                || runtime.rendered_primary_plane_scanout_cleanup_pending())
        {
            return Err("semantic singleton startup found pre-existing runtime ownership".into());
        }
        let group = self.heads[indices[0]].group;
        if indices
            .iter()
            .any(|index| self.heads[*index].group != group)
        {
            return Err("one mirrored logical output cannot span DRM cards".into());
        }
        let identities = frames
            .iter()
            .map(|frame| {
                (
                    frame.head,
                    (
                        frame.scene_generation,
                        frame.target_generation,
                        frame.mapping,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let frame = self.queue_initial_head_composition_frames(output, frames)?;
        let required = indices
            .iter()
            .map(|index| self.heads[*index].head)
            .collect::<Vec<_>>();
        let mut workers = BTreeSet::new();
        for index in indices.iter().copied() {
            self.enable_head_renderer_worker(index)?;
            if !self.exporters[index].worker_enabled() {
                return Err("semantic startup renderer worker was not established".into());
            }
            // `workers` counts renderer threads this head can reach, which is
            // one whether it owns the thread or shares its group's. How many
            // threads the session actually runs is a session-wide fact and is
            // reported as one, in the resource record.
            tracing::info!(
                "sophia_live_head_bootstrap schema=1 status=worker_ready output={} head={} workers=1",
                output.raw(),
                self.heads[index].head.raw(),
            );
            workers.insert(self.heads[index].head);
        }

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut prepared =
            BTreeMap::<sophia_engine::RenderHeadId, LiveProductionPreparedTopologyHead>::new();
        loop {
            for index in indices.iter().copied() {
                let head = self.heads[index].head;
                if prepared.contains_key(&head) {
                    continue;
                }
                let selection = self.heads[index].selection;
                let vrr_enabled = topology_vrr_enabled(self.heads[index].vrr);
                match self.prepare_output_topology_head(head, selection, vrr_enabled) {
                    Ok(Some(owner)) => {
                        prepared.insert(head, owner);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        self.cancel_semantic_startup_resources(group, prepared);
                        return Err(error);
                    }
                }
            }
            if prepared.len() == indices.len() {
                break;
            }
            if Instant::now() >= deadline {
                self.cancel_semantic_startup_resources(group, prepared);
                return Err("semantic multi-head startup renderer deadline expired".into());
            }
            std::thread::yield_now();
        }

        let prepared_heads = prepared.keys().copied().collect::<BTreeSet<_>>();
        if reduce_live_production_semantic_startup_barrier(&required, &workers, &prepared_heads)
            != LiveProductionSemanticStartupBarrier::Ready
        {
            self.cancel_semantic_startup_resources(group, prepared);
            return Err("semantic multi-head startup prepare barrier was incomplete".into());
        }

        let changes = indices
            .iter()
            .map(|index| {
                let owner = prepared
                    .get(&self.heads[*index].head)
                    .expect("semantic startup prepared complete head coverage");
                crate::LibdrmNativeAtomicTopologyChange::Enabled(owner.atomic_head())
            })
            .collect::<Vec<_>>();
        loop {
            match crate::submit_native_topology_change_on_device(
                self.groups[group].session.card(),
                &changes,
            ) {
                crate::NativeTopologySubmitOutcome::Accepted => break,
                crate::NativeTopologySubmitOutcome::Busy if Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                outcome => {
                    self.cancel_semantic_startup_resources(group, prepared);
                    return Err(format!(
                        "semantic multi-head startup atomic commit failed: {outcome:?}"
                    )
                    .into());
                }
            }
        }

        let mut adoption_errors = Vec::new();
        for index in indices {
            let head_id = self.heads[index].head;
            let owner = prepared
                .remove(&head_id)
                .expect("accepted semantic startup retained every head owner");
            let displayed = crate::adopt_prepared_rendered_topology_head_after_commit(owner);
            if singleton {
                if let Err(displayed) =
                    runtime.try_adopt_presented_rendered_primary_plane_scanout(displayed)
                {
                    self.heads[index]
                        .scanout_custody
                        .adopt_displayed(
                            displayed.map_scanout_buffer(|owner| {
                                Box::new(owner) as Box<dyn std::any::Any>
                            }),
                        )
                        .expect("startup fallback custody prevalidated before commit");
                    adoption_errors.push(
                        "semantic singleton startup runtime rejected its displayed owner"
                            .to_owned(),
                    );
                }
            } else {
                self.heads[index]
                    .scanout_custody
                    .adopt_displayed(
                        displayed
                            .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>),
                    )
                    .expect("startup head custody prevalidated before commit");
            }
            let exported_nonzero = self.exporters[index].composition_nonzero_rgb_pixels() > 0;
            if exported_nonzero {
                self.nonzero_exports = self.nonzero_exports.saturating_add(1);
                self.heads[index].nonzero_exports =
                    self.heads[index].nonzero_exports.saturating_add(1);
            }
            self.submissions = self.submissions.saturating_add(1);
            trace_live_native_lifecycle("initial_modeset_complete");
            let head = &mut self.heads[index];
            head.submissions = head.submissions.saturating_add(1);
            head.presented_logical_checksum = head.last_checksum;
            head.presented_submissions = head.submissions;
            head.presented_content = head.pending_content.take();
            if head.output_frames.pending().is_some() {
                match head.output_frames.mark_initial_presented() {
                    Ok(presented) => {
                        trace_presented_output_damage(
                            "initial_presented",
                            head.output.id,
                            &presented,
                        );
                    }
                    Err(error) => adoption_errors.push(format!(
                        "initial compositor display-list transition failed for head {}: {error}",
                        head_id.raw(),
                    )),
                }
            }
            head.initial_modeset_submission = Some(head.submissions);
            let transition = self
                .output_lifecycles
                .get_mut(&output)
                .expect("a registered output has a head lifecycle")
                .mark_initialized(head_id);
            if !matches!(
                transition,
                LiveProductionMirrorHeadTransition::Accepted
                    | LiveProductionMirrorHeadTransition::GroupReady
            ) {
                adoption_errors.push(format!(
                    "semantic startup lifecycle rejected initialized head {}: {transition:?}",
                    head_id.raw(),
                ));
            }
            let (scene_generation, target_generation, mapping) = identities
                .get(&head_id)
                .copied()
                .expect("semantic startup retained head plan identity");
            tracing::info!(
                "sophia_live_head_bootstrap schema=1 status=worker_composed output={} head={} frame={} scene_generation={} target_generation={} mapping={} exports=1",
                output.raw(),
                head_id.raw(),
                frame.raw(),
                scene_generation,
                target_generation,
                mapping.reduced_name(),
            );
        }
        if !prepared.is_empty() {
            return Err("semantic startup retained an unadopted head owner".into());
        }
        if !adoption_errors.is_empty() {
            return Err(format!(
                "semantic startup adoption failed after retaining every KMS owner: {}",
                adoption_errors.join("; "),
            )
            .into());
        }
        Ok(frame)
    }

    fn cancel_semantic_startup_resources(
        &mut self,
        group: usize,
        prepared: BTreeMap<sophia_engine::RenderHeadId, LiveProductionPreparedTopologyHead>,
    ) {
        for (head, owner) in prepared {
            let cancelled = crate::cancel_prepared_rendered_topology_head(
                self.groups[group].session.card(),
                owner,
            );
            if let Some(cleanup) = cancelled.cleanup {
                let cleanup =
                    cleanup.map_scanout_buffer(|owner| Box::new(owner) as Box<dyn std::any::Any>);
                if let Some(index) = self.head_index_for_head(head) {
                    if let Err(cleanup) = self.heads[index].scanout_custody.accept_cleanup(cleanup)
                    {
                        self.output_topology_cleanup.push((head, cleanup));
                    }
                } else {
                    self.output_topology_cleanup.push((head, cleanup));
                }
            }
            if cancelled.destroy != crate::LibdrmNativePrimaryPlaneResourceDestroyStatus::Destroyed
            {
                self.retire_failures = self.retire_failures.saturating_add(1);
            }
        }
    }

    /// Releases semantic startup work that never reached KMS.
    ///
    /// A worker command is affine even before framebuffer preparation. It must
    /// be polled to a terminal owner and dropped; merely clearing the head's
    /// passive bookkeeping would detach that renderer lease from cleanup.
    pub(super) fn abort_semantic_startup_head_work(
        &mut self,
        output: OutputId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let indices = self.head_indices(output);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let mut pending = false;
            for index in indices.iter().copied() {
                if !self.exporters[index].pending_frame() {
                    continue;
                }
                if !self.exporters[index].worker_in_flight() {
                    self.exporters[index].discard_pending_frame();
                    continue;
                }
                let selection = self.heads[index].selection;
                let export =
                    crate::LiveRenderedScanoutBufferExporter::export_rendered_scanout_buffer(
                        &mut self.exporters[index],
                        crate::LiveGbmEglFrameTargetRecord::new(selection.size()),
                    );
                pending |= export.status == crate::LiveRendererScanoutBufferExportStatus::Pending;
                // A terminal worker export has not acquired DRM ownership yet;
                // dropping it here returns the renderer lease to its worker.
                drop(export);
            }
            if !pending
                && indices
                    .iter()
                    .all(|index| !self.exporters[*index].pending_frame())
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("semantic startup renderer abort deadline expired".into());
            }
            std::thread::yield_now();
        }
        for index in indices {
            let head = &mut self.heads[index];
            head.pending_content = None;
            head.rendering_content = None;
            head.output_frames.discard_pending();
            tracing::info!(
                "sophia_live_head_bootstrap schema=1 status=aborted output={} head={} renderer_pending=0",
                output.raw(),
                head.head.raw(),
            );
        }
        Ok(())
    }
}

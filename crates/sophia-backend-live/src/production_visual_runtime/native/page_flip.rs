// Retirement of a GPU page flip: the commit point of a Present that reached
// a plane. Split from native.rs, which holds the scanout's attach, drain and
// rebind paths; this is the one place a flip becomes committed Engine state
// or is recorded as discarded.

use super::*;

impl LiveProductionVisualRuntime {
    pub fn finalize_gpu_page_flip(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        output: OutputId,
        retirement: LiveProductionNativeFrameRetirement,
    ) -> Result<Option<LiveProductionRetiredPresent>, Box<dyn std::error::Error>> {
        if reduce_live_production_native_retirement_owner(
            retirement.frame,
            retirement.content,
            self.present_scheduler.submitted_frame(output),
            self.present_scheduler.owns_frame(output, retirement.frame),
        ) != LiveProductionNativeRetirementOwner::SubmittedDmaPresent
        {
            return Err("GPU retirement does not own the selected output frame".into());
        }
        if !matches!(
            retirement.content,
            LiveProductionScanoutContent::MixedPresent { transaction, .. }
                if Some(transaction) == self.present_scheduler.in_flight_transaction()
        ) {
            return Err(self
                .ownership_mismatch(output, retirement)
                .to_string()
                .into());
        }
        let terminal =
            self.present_scheduler
                .mark_output_retired(LiveProductionPageFlipRetirement {
                    output,
                    ust: retirement.ust,
                    msc: retirement.msc,
                })?;
        let Some(sophia_engine::TransactionPresentationTerminal::Presented { .. }) = terminal
        else {
            return Ok(None);
        };
        let (surface, layer) = self
            .present_scheduler
            .in_flight_displayed_layer()
            .ok_or("joined native retirement lost its renderer image owner")?;
        let image = layer.image_id;
        let transaction = self
            .present_scheduler
            .in_flight_transaction()
            .ok_or("joined native retirement lost its transaction owner")?;
        // The page flip is the commit point for the compositor copy. Promote
        // its staged image before releasing the client source or emitting any
        // protocol feedback.
        //
        // A direct frame has no such image. Nothing was composed, so nothing
        // was staged: the buffer on the plane is the client's own, and the
        // renderer never saw it. Demanding a snapshot here failed the first
        // frame that ever reached a plane directly -- after it had already
        // been displayed, which made a working flip look like a lost one.
        if !retirement.direct && native_scanout.promote_renderer_image(image)? == 0 {
            return Err(format!(
                "retired Present lost its staged renderer snapshot: transaction={} surface={} image={} output={} frame={}",
                transaction.raw(), surface.index(), image.raw(), output.raw(), retirement.frame.raw(),
            ).into());
        }
        let submitted = self
            .present_scheduler
            .take_submitted()
            .ok_or("joined native retirement lost its submitted DMA Present")?;
        let layout_identity = layout_witness::SubmittedLayoutIdentity::from_submitted(&submitted);
        let clock = submitted
            .presentation_clock()
            .ok_or("joined native retirement retained no physical presentation clock")?;
        let ust = clock.ust;
        let msc = clock.msc;
        let outputs = submitted.frames().map(|(output, _)| output).collect();
        let direct = retirement.direct;
        // Read before the settlement consumes the prepared commit: if the
        // Engine refuses the candidate, this and the current generation are
        // what say whether an intake landed between prepare and retire.
        let baseline_generation = submitted
            .prepared
            .baseline()
            .iter()
            .find(|state| state.surface == submitted.surface)
            .map_or(0, |state| state.committed_generation);
        let (production, presentation_feedback) =
            (&mut self.production, &mut self.presentation_feedback);
        let mut completion = production
            .settle_prepared_retirement(submitted.prepared, |commit| match commit.outcome {
                // A direct frame completes without idling: the buffer the
                // client handed over is the buffer the screen is scanning, and
                // releasing it here would let the client draw into displayed
                // pixels. Its successor idles it, above, on the next flip.
                TransactionOutcome::Committed if direct => presentation_feedback
                    .complete_flip_without_idle(submitted.transaction, ust, msc),
                TransactionOutcome::Committed => {
                    presentation_feedback.complete_copy(submitted.transaction, ust, msc)
                }
                TransactionOutcome::RejectedStaleSurface
                | TransactionOutcome::RejectedInvalidSurface
                | TransactionOutcome::TimedOut => {
                    presentation_feedback.reject_skip(submitted.transaction, ust, msc)
                }
            })
            .map_err(|error| format!("page flip protocol settlement failed: {error:?}"))?;
        let layout_witness = layout_identity.and_then(|identity| {
            identity.settle_feedback(retirement, &completion.commit, &mut completion.evidence)
        });
        self.outputs
            .project_committed(&completion.committed_surfaces);
        self.route_present_feedback(completion.evidence);
        if completion.commit.outcome != TransactionOutcome::Committed {
            self.present_rejections = self.present_rejections.saturating_add(1);
        }
        if direct && completion.commit.outcome == TransactionOutcome::Committed {
            self.displayed_direct_presents
                .insert(output, submitted.transaction);
        }
        let deferred_groups = self.finish_surface_content_owner(submitted.candidate)?;
        if deferred_groups != 0 {
            tracing::debug!(
                transaction = submitted.transaction.raw(),
                surface = submitted.surface.index(),
                deferred_groups,
                "retired Present released its ordered surface authority backlog"
            );
        }
        if completion.commit.outcome != TransactionOutcome::Committed {
            // Nothing to evict for a direct frame, for the same reason nothing
            // was promoted; eviction of an image no exporter staged is a
            // no-op, so this is stated rather than branched.
            native_scanout.evict_renderer_image(submitted.displayed_layer.image_id)?;
            // The flip showed this candidate and the Engine kept the older
            // state, so the screen and the committed set now disagree until the
            // surface's next Present lands. The session reads that from its
            // service report; the warn alone left the frame that follows
            // unexplained.
            if self.discarded_presents.len() < DISCARDED_PRESENT_CAPACITY {
                let current_generation = self
                    .production
                    .committed_surfaces()
                    .iter()
                    .find(|state| state.surface == submitted.surface)
                    .map_or(0, |state| state.committed_generation);
                self.discarded_presents
                    .push(crate::LiveProductionDiscardedPresent {
                        transaction: submitted.transaction,
                        surface: submitted.surface,
                        outcome: completion.commit.outcome,
                        baseline_generation,
                        current_generation,
                    });
            }
            tracing::warn!(
                transaction = completion.commit.transaction.raw(),
                outcome = ?completion.commit.outcome,
                "settled retired Present without applying its stale Engine candidate"
            );
            return Ok(None);
        }
        let source_size = submitted.displayed_layer.size;
        let target = submitted.displayed_layer.placement.target;
        let clip = submitted.displayed_layer.placement.clip;
        let replaced = replace_displayed_surface(
            &mut self.displayed_surfaces,
            submitted.surface,
            submitted.displayed_layer,
        );
        if let Some(replaced) = replaced {
            native_scanout.evict_renderer_image(replaced.layer.image_id)?;
        }
        Ok(Some(LiveProductionRetiredPresent {
            candidate: submitted.candidate,
            transaction: submitted.transaction,
            surface: submitted.surface,
            outputs,
            source_size,
            target,
            clip,
            ust_usec: ust,
            msc,
            layout_witness,
        }))
    }
}

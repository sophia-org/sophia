impl LiveProductionNativeScanout {
        /// The stalled head's index and how long its flip has been outstanding.
        ///
        /// The index rather than the output alone, because attributing a stall
        /// needs the head's own history and the output cannot reach it: a mirror
        /// group's heads share an output.
        pub fn page_flip_hard_stall(&self) -> Option<(usize, Duration)> {
            self.heads.iter().enumerate().find_map(|(index, head)| {
                let age = head.submitted_at.map(|submitted| submitted.elapsed());
                (reduce_live_production_page_flip_watchdog(
                    age,
                    LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL,
                ) == LiveProductionPageFlipWatchdogStatus::HardStall)
                    .then_some((index, age.unwrap_or_default()))
            })
        }

        pub fn ensure_page_flip_progress(&self) -> Result<(), Box<dyn std::error::Error>> {
            let Some((index, age)) = self.page_flip_hard_stall() else {
                return Ok(());
            };
            let head = &self.heads[index];
            // Its own record rather than another `sophia_live_native_page_flip`
            // status: that name's schema=1 is pinned by matchers reading retained
            // evidence, and a stall's fields have nothing in common with a flip's
            // lifecycle transitions. Widening the shared name would have meant
            // bumping every status under it and invalidating runs already on disk.
            //
            // Two of these terminated a session before any input and neither
            // could be attributed, because `output` and `age` do not distinguish
            // a head that never received its first vblank from one that stopped
            // receiving them, nor a stalled head from a stalled group. The
            // fields below are the ones that separate those cases, and they are
            // already on the head at the moment the stall is declared.
            //
            // The other heads' ages come along because a stall shared across a
            // mirror group is a different fault from a stall on one connector,
            // and the first head to cross the boundary is not necessarily the
            // one that caused it.
            let peers = self
                .heads
                .iter()
                .enumerate()
                .filter(|(peer, _)| *peer != index)
                .map(|(peer, head)| {
                    format!(
                        "{peer}:{}",
                        head.submitted_at.map_or(-1i64, |submitted| i64::try_from(
                            submitted.elapsed().as_millis()
                        )
                        .unwrap_or(i64::MAX))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            // Instantaneous state explains what can retire now; cumulative
            // counters explain whether this process ever decoded, rejected, or
            // emitted evidence. An empty final read is not an attribution.
            let diagnostics = self.groups[head.group]
                .session
                .page_flip_poller_diagnostics();
            let cumulative = self.groups[head.group]
                .session
                .page_flip_poller_cumulative_diagnostics();
            tracing::error!(
                "sophia_live_native_page_flip_stall schema=3 status=hard_stall output={} head={} index={} group={} age_ms={} generation={} submissions={} retirements={} callbacks={} ever_retired={} callback_serial={} in_flight_ticks={} submitted_sequence={} peer_age_ms=[{}] completion_mode={:?} completion_fence={:?} completion_ledger_pending={} out_fence_retirements={} late_page_flip_events={} completion_fence_errors={} poller_pending={} poller_routes={} poller_last_read={:?} poller_last_decoded={} poller_last_rejected={} poller_read_calls={} poller_would_block_reads={} poller_read_failures={} poller_decoded_total={} poller_rejected_total={} poller_emitted_total={} action=terminate_session",
                head.output.id.raw(),
                head.head.raw(),
                index,
                head.group,
                age.as_millis(),
                head.target_generation,
                head.submissions,
                head.retirements,
                head.callback_accepted,
                head.retirements != 0,
                head.last_callback_serial
                    .map_or_else(|| "none".to_owned(), |serial| serial.to_string()),
                head.scanout_in_flight_ticks,
                head.submitted_sequence
                    .map_or_else(|| "none".to_owned(), |sequence| sequence.to_string()),
                peers,
                head.completion_mode,
                head.completion_fence_status,
                head.pending_callback.is_some(),
                head.out_fence_retirements,
                head.late_page_flip_events,
                head.completion_fence_errors,
                diagnostics.pending_callbacks,
                diagnostics.route_count,
                diagnostics.last_read_loop.status,
                diagnostics.last_read_loop.decoded_callbacks,
                diagnostics.last_read_loop.rejected_callbacks,
                cumulative.read_calls,
                cumulative.would_block_reads,
                cumulative.read_failures,
                cumulative.decoded_callbacks,
                cumulative.rejected_callbacks,
                cumulative.emitted_callbacks,
            );
            Err(format!(
                "native page flip exceeded the {} ms hard-stall boundary on head {} after {} retirements",
                LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL.as_millis(),
                head.head.raw(),
                head.retirements,
            )
            .into())
        }
}

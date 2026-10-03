// Bounded owner wait while frame service or topology recovery owns the turn.
{
                let preparation_wake = output_topology_quarantined.then(|| {
                    native_scanout.as_ref().and_then(|native| native.output_topology_preparation_next_service())
                }).flatten();
                // A rollback waiting for candidate presentation ownership is
                // paced the same way; the backend reports no preparation wake then.
                let rollback_wake = active_output_topology_preparation
                    .as_ref()
                    .filter(|execution| execution.phase == LiveOutputTopologyExecutionPhase::RollingBack)
                    .and_then(|execution| execution.rollback_quiescence.as_ref())
                    .and_then(OutputTopologyRollbackQuiescence::next_wake);
                let preparation_wake = match (preparation_wake, rollback_wake) {
                    (Some(preparation), Some(rollback)) => Some(preparation.min(rollback)),
                    (preparation, rollback) => preparation.or(rollback),
                };
                // Preserve topology quarantine and alternate bounded frame
                // service with authority turns, including after frontend EOF.
                if (native_frame_service_preemption && !output_topology_quarantined)
                    || authority_ingress == AuthorityIngressState::Disconnected
                    || preparation_wake.is_some()
                {
                    // A closed receiver cannot provide the usual bounded wait.
                    // Yield until the earliest frame/drain deadline instead of
                    // spinning while a renderer or policy response is pending.
                    let now = Instant::now();
                    // Ordinary frames cannot run during preparation. Their
                    // already-due deadlines must not turn this wait into a
                    // zero-length spin while an exporter is deferring.
                    let mut wait = if let Some(wake) = preparation_wake {
                        wake.saturating_duration_since(now).min(OWNER_SHORT_SERVICE_INTERVAL)
                    } else {
                        let wait = paced_repaint_wait_cap(primary_frame_pacer, paced_repaint_runnable, now, OWNER_SHORT_SERVICE_INTERVAL);
                        runtime.as_ref().map_or(wait, |runtime| runtime.frame_deadline_cap_wait(now, wait))
                    };
                    if let Some(quiescence) = session_quiescence.as_ref() {
                        wait = wait.min(quiescence.deadline.saturating_duration_since(now));
                    }
                    // A due completion service turn runs immediately; fd-less
                    // work keeps its bounded short retry, interrupted by rings.
                    if native_wait_only && native_service_due {
                        wait = Duration::ZERO;
                    }
                    if !wait.is_zero() {
                        let wait = native_wait.deadline.map_or(wait, |deadline| {
                            wait.min(deadline.saturating_duration_since(now))
                        });
                        owner_wake.record_native_wait(true, !native_wait.descriptors.is_empty());
                        owner_wake.wait_for_service(
                            wait,
                            native_wait.descriptors.into_iter()
                                .map(|fd| rustix::event::PollFd::from_borrowed_fd(fd, rustix::event::PollFlags::IN))
                                .collect(),
                            native_scanout.as_ref().map(LiveProductionNativeScanout::completion_progress),
                        )?;
                    }
                }
}

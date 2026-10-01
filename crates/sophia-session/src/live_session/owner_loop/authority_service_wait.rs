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
                        wake.saturating_duration_since(now).min(Duration::from_millis(1))
                    } else {
                        let wait = paced_repaint_wait_cap(primary_frame_pacer, paced_repaint_runnable, now, Duration::from_millis(1));
                        runtime.as_ref().map_or(wait, |runtime| runtime.frame_deadline_cap_wait(now, wait))
                    };
                    if let Some(quiescence) = session_quiescence.as_ref() {
                        wait = wait.min(quiescence.deadline.saturating_duration_since(now));
                    }
                    if !wait.is_zero() {
                        std::thread::sleep(wait);
                    }
                }
}

// Finish or refuse topology restoration before ordinary native teardown.
{
    let mut topology_rollback_established = false;
    if let Some(native_scanout) = native_scanout.as_mut()
        && native_scanout.output_topology_preparation_active()
    {
        // Submitted candidate first frames are not suspended here: that would
        // drop the displayed owner before restoration. The rollback branch
        // below retires them in place before its blocking reverse apply.
        native_scanout.request_abort_output_topology_preparation(
            "session completion cancelled topology preparation",
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut rollback_quiescence =
            OutputTopologyRollbackQuiescence::until(Instant::now(), deadline);
        loop {
            use sophia_backend_live::LiveProductionNativeTopologyPreparationPhase as Phase;
            match native_scanout.output_topology_preparation_phase() {
                Some(Phase::Failed) => {
                    let published_preserved =
                        native_scanout.output_topology_failed_without_mutation();
                    match native_scanout.finish_failed_output_topology_preparation() {
                        Ok((plan, reason)) => crate::session_println!(
                            "sophia_live_output_topology schema=2 status=completion_cancelled heads={} reason={reason:?}",
                            plan.heads.len(),
                        ),
                        Err(error) => cleanup_failures.push(format!(
                            "topology preparation completion failed: {error}"
                        )),
                    }
                    topology_rollback_established = published_preserved;
                    break;
                }
                Some(Phase::RolledBack) => {
                    match native_scanout.install_rolled_back_output_topology() {
                        Ok((plan, reason)) => {
                            let rollback_outputs = native_scanout.outputs();
                            let rollback_viewports = plan
                                .logical_viewports
                                .iter()
                                .map(|viewport| (viewport.output, viewport.logical))
                                .collect::<Vec<_>>();
                            if let Some(runtime) = runtime.as_mut()
                                && let Err(error) = runtime.rebind_applied_native_topology(
                                    native_scanout,
                                    &rollback_outputs,
                                    &rollback_viewports,
                                )
                            {
                                cleanup_failures.push(format!(
                                    "topology rollback runtime rebind failed: {error}"
                                ));
                            }
                            crate::session_println!(
                                "sophia_live_output_topology schema=2 status=completion_rolled_back heads={} reason={reason:?}",
                                plan.heads.len(),
                            );
                            topology_rollback_established = true;
                        }
                        Err(error) => cleanup_failures.push(format!(
                            "topology rollback installation failed: {error}"
                        )),
                    }
                    break;
                }
                Some(Phase::RollingBack) => {
                    // Without the visual runtime nothing can retire candidate
                    // presentation ownership, so restoration cannot be proven
                    // safe to start.
                    let Some(runtime) = runtime.as_mut() else {
                        cleanup_failures
                            .push("topology completion rollback lost the visual runtime".to_owned());
                        break;
                    };
                    let mut owners = (runtime, &mut *native_scanout);
                    if let Err(error) = rollback_quiescence.turn(
                        Instant::now(),
                        &mut owners,
                        |(runtime, native)| runtime.service_output_topology_rollback_quiescence(native),
                        |(_, native)| native.service_prepared_output_topology_apply(),
                    ) {
                        cleanup_failures
                            .push(format!("topology completion rollback failed: {error}"));
                        break;
                    }
                }
                Some(
                    Phase::PreparingCandidate
                    | Phase::PreparingRollback
                    | Phase::Prepared
                    | Phase::Aborting,
                ) => {
                    if let Err(error) = native_scanout.service_output_topology_preparation() {
                        cleanup_failures.push(format!(
                            "topology renderer preparation abort failed: {error}"
                        ));
                        break;
                    }
                }
                Some(Phase::Applying | Phase::Applied | Phase::CandidateInstalled | Phase::FirstFramesQueued) => {
                    cleanup_failures.push(
                        "topology completion abort did not enter a safe rollback phase".to_owned(),
                    );
                    break;
                }
                None => break,
            }
            if Instant::now() >= deadline {
                    cleanup_failures.push(
                        "topology transaction did not abort within two seconds".to_owned(),
                    );
                break;
            }
            // A rollback still waiting for presentation ownership idles until
            // its next turn, never past the abort deadline.
            let now = Instant::now();
            match rollback_quiescence.next_wake() {
                Some(wake) if native_scanout.output_topology_preparation_phase() == Some(Phase::RollingBack) => {
                    let wait = wake
                        .saturating_duration_since(now)
                        .min(deadline.saturating_duration_since(now))
                        .min(Duration::from_millis(1));
                    if wait.is_zero() {
                        std::thread::yield_now();
                    } else {
                        std::thread::sleep(wait);
                    }
                }
                _ => std::thread::yield_now(),
            }
        }
        while native_scanout.output_topology_cleanup_pending() && Instant::now() < deadline {
            native_scanout.retry_output_topology_cleanup();
            std::thread::yield_now();
        }
        if native_scanout.output_topology_cleanup_pending() {
            cleanup_failures
                .push("topology resource cleanup remained pending at native suspension".to_owned());
        }
    }
    topology_rollback_established
}

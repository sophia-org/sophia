{
        let child_reap_started = Instant::now();
        if !primary_child_exited
            && let Some(primary_child) = child.as_deref_mut()
            && let Some(status) = primary_child.try_wait()?
        {
            crate::diagnostics::application::exited(primary_diagnostic, status);
            primary_exit_status = Some(status);
            if terminal_exit_is_session_failure(
                status.success(),
                config.normal_session,
                session_quiescence.is_some(),
            ) {
                let error =
                    format!("session client exited during live session with status {status}");
                terminal_client_error = Some(("primary", error));
                if let Err(error) = stop_frontend_intake(
                    frontend_service_sender,
                    &mut terminal_client_intake_stopped,
                ) {
                    terminal_client_cleanup_failures
                        .push(format!("frontend intake stop failed: {error}"));
                }
                crate::session_println!(
                    "sophia_live_session_client_fatal schema=1 status=detected source=primary exit_status={status} action=bounded_cleanup"
                );
                break 'session;
            }
            if status.success()
                && config.expect_physical_pointer
                && metrics.physical_pointer_buttons_routed == 0
            {
                return Err(
                    "session client exited before the required physical pointer selection".into(),
                );
            }
            if config.application_proof_requested() {
                client_stdout = client_stdout_capture
                    .ok_or("application stdout capture is missing")?
                    .read_bounded()?;
                if client_stdout.len() > 4_096 {
                    return Err("application stdout exceeded the 4096-byte evidence bound".into());
                }
                if let (Some(text), Some(expected)) = (
                    config.inject_text.as_deref(),
                    config.expect_client_stdout.as_deref(),
                ) && client_stdout == expected.as_bytes()
                {
                    input_text_match = true;
                    crate::session_println!(
                        "sophia_live_session_input schema=3 status=semantic_complete source=synthetic text_match=true bytes={}",
                        text.len()
                    );
                }
            }
            if config.normal_session {
                if let Some(id) = config.applications.startup.first() {
                    crate::session_println!(
                        "sophia_session_app schema=1 status=exited id={id} source=startup exit_status={status}",
                    );
                }
                primary_child_exited = true;
                if config.exit_when_startup_exits {
                    begin_session_quiescence!("startup_application_exit");
                }
            } else {
                if status.success()
                    && successful_primary_exit_ends_session(config.input_proof_requested())
                {
                    begin_session_quiescence!("successful_primary_exit");
                }
                // The proof helper intentionally exits after displaying its
                // received text. Keep the session and secondary terminal alive so
                // the final native frame can retire and pointer evidence can run.
                primary_child_exited = true;
            }
        }
        if config.application_proof_requested()
            && !input_text_match
            && physical_text_proof
                .as_ref()
                .is_some_and(PhysicalTextProof::is_complete)
        {
            input_text_match = true;
            crate::session_println!(
                "sophia_live_session_input schema=3 status=semantic_complete source=physical text_match=true bytes={}",
                config.expect_physical_text.as_ref().map_or(0, String::len)
            );
        }
        let mut secondary_index = 0;
        while secondary_index < secondary_children.len() {
            let observed = secondary_children[secondary_index].poll(
                authenticated_launches, owner_wake.notifier(), Instant::now());
            if let Some(crate::namespace_launch::ApplicationProcessEvent::Failed(reason)) = &observed {
                let managed = &mut secondary_children[secondary_index];
                if managed.matches_admission(session_launches) {
                    session_launches.fail_current();
                    launch_admission_started_at = None;
                }
                crate::session_eprintln!(
                    "sophia_session_app schema=3 status=failed id={} source=managed reason={reason}",
                    managed.id.as_deref().unwrap_or("untracked"));
                managed.terminate(true)?;
                secondary_children.remove(secondary_index);
                continue;
            }
            if let Some(crate::namespace_launch::ApplicationProcessEvent::Exited(status)) = observed {
                crate::diagnostics::application::exited(secondary_children[secondary_index].diagnostic, status);
                // OR THE SESSION IS ALREADY SHUTTING DOWN, for the reason
                // `terminal_exit_is_session_failure` gives: quiescence closes
                // the X server these clients are connected to, so their exit
                // is the shutdown working rather than a fault. The primary
                // path is where the QEMU gate reproduced it; this is the same
                // bug one branch away.
                if managed_child_exit_is_nonfatal(
                    config.normal_session,
                    secondary_children[secondary_index].launch_transaction,
                ) || session_quiescence.is_some()
                {
                    secondary_children[secondary_index].terminate(true)?;
                    let launch_transaction =
                        secondary_children[secondary_index].launch_transaction;
                    let id = secondary_children[secondary_index]
                        .id
                        .as_deref()
                        .unwrap_or("untracked");
                    crate::session_println!(
                        "sophia_session_app schema=1 status=exited id={id} source=managed exit_status={status}",
                    );
                    let exiting_admission = secondary_children[secondary_index]
                        .matches_admission(session_launches);
                    if exiting_admission
                        && status.success()
                        && let Some(admission) = session_launches.complete_successful_exit(
                            launch_transaction.expect("exiting admission has a transaction"),
                            secondary_children[secondary_index].catalog_launch,
                        )
                    {
                        launch_admission_started_at = None;
                        crate::session_println!(
                            "sophia_session_app schema=2 status=completed id={id} source=action transaction={} reason=normal_exit exit_status={status}",
                            admission.intent.transaction.raw(),
                        );
                    } else if exiting_admission
                        && let Some(admission) = session_launches.fail_current()
                    {
                        launch_admission_started_at = None;
                        crate::session_eprintln!(
                            "sophia_session_app schema=2 status=failed id={id} source=action transaction={} application={} reason=exit_before_admission exit_status={status}",
                            admission.intent.transaction.raw(),
                            admission.intent.application.raw(),
                        );
                    }
                    if let Some(launch) = &secondary_children[secondary_index].native_catalog {
                        let grant = launch.cause.grant();
                        crate::session_println!(
                            "sophia_catalog_launch schema=1 status=process_exited transaction={} connection_epoch={} content_grant_epoch={} success={}",
                            launch.transaction.raw(), grant.connection_epoch, grant.content_grant_epoch,
                            status.success(),
                        );
                    }
                    secondary_children.remove(secondary_index);
                } else {
                    let error = format!(
                        "secondary xterm {} exited during live session with status {status}",
                        secondary_index + 1
                    );
                    terminal_client_error = Some(("secondary", error));
                    if let Err(error) = stop_frontend_intake(
                        frontend_service_sender,
                        &mut terminal_client_intake_stopped,
                    ) {
                        terminal_client_cleanup_failures
                            .push(format!("frontend intake stop failed: {error}"));
                    }
                    crate::session_println!(
                        "sophia_live_session_client_fatal schema=1 status=detected source=secondary index={} exit_status={status} action=bounded_cleanup",
                        secondary_index + 1,
                    );
                    break 'session;
                }
            } else {
                secondary_index += 1;
            }
        }
        metrics.max_child_reap = metrics.max_child_reap.max(child_reap_started.elapsed());
}

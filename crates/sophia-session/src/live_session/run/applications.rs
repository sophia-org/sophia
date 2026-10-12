// Startup applications and the first-frame ordering barrier.
{
    let normal_primary = config
        .normal_session
        .then(|| {
            config.applications.startup.first().map(|id| {
                config
                    .applications
                    .applications
                    .get(id)
                    .expect("normal session startup application was validated")
            })
        })
        .flatten();
    let mut terminal_command = match (normal_primary, config.client.as_deref()) {
        (Some(app), _) => Some(std::process::Command::new(&app.executable)),
        (None, _) if config.normal_session => None,
        (None, Some(client)) => Some(application_client_command(client)),
        (None, None) => Some(std::process::Command::new(
            terminal.as_deref().expect("xterm executable is resolved"),
        )),
    };
    let (client_stdout_capture, client_stdout_file) = if config.client.is_some() {
        let (capture, file) = LiveClientStdoutCapture::create(display_number)?;
        (Some(capture), Some(file))
    } else {
        (None, None)
    };
    if let Some(terminal_command) = terminal_command.as_mut() {
        crate::application_catalog::configure_host_application_environment(
            terminal_command,
            config.control_socket.as_deref(),
            config.inspection_socket.as_deref(),
        );
        terminal_command
            .env("DISPLAY", &config.display)
            .env("XAUTHORITY", xauthority.path())
            .env_remove("ENV")
            .env_remove("BASH_ENV")
            .stdin(Stdio::null())
            .stderr(Stdio::inherit());
        if let Some(app) = normal_primary {
            terminal_command
                .args(&app.arguments)
                .process_group(0)
                .stdout(Stdio::inherit());
        } else if config.client.is_some() {
            terminal_command
                .env("GDK_BACKEND", "x11")
                .env("GTK_USE_PORTAL", "0")
                .env_remove("WAYLAND_DISPLAY")
                .args(&config.client_args)
                .stdout(Stdio::from(
                    client_stdout_file.expect("application stdout file was created"),
                ));
        } else {
            terminal_command
                .args([
                    "-cm",
                    "-dc",
                    "-geometry",
                    "120x36+80+60",
                    "-title",
                    "Sophia Terminal",
                ])
                .stdout(Stdio::inherit());
        }
        if let Some(result) = input_proof_result.as_ref() {
            terminal_command.env("SOPHIA_INPUT_PROOF_RESULT", result.path());
        }
        if config.client.is_none()
            && let Some(proof_text) = config
                .inject_text
                .as_deref()
                .or(config.expect_physical_text.as_deref())
        {
            terminal_command
                .args([
                    "-e",
                    "sh",
                    "-c",
                    PRIMARY_INPUT_PROOF_SCRIPT,
                    "sophia-input-proof",
                ])
                .arg(proof_text)
                .arg(
                    input_proof_result
                        .as_ref()
                        .expect("input proof result exists with proof text")
                        .path(),
                );
        } else if let Some(program) = config.terminal_exec.as_deref() {
            terminal_command
                .env_remove("ENV")
                .env_remove("BASH_ENV")
                .arg("-e")
                .arg(program)
                .args(&config.terminal_exec_args);
        }
    }
    let child = match terminal_command
        .map(|mut command| {
            if let Some(app) = normal_primary
                && config.core_config_state.active().session.namespace_policy.select_application(&format!("registered:{}", app.id), None)
                    .map_err(std::io::Error::other)?.is_some()
            {
                let effective = crate::application_catalog::ApplicationLaunchCommand {
                    executable: command.get_program().into(),
                    arguments: command.get_args().map(|arg| arg.to_str().map(str::to_owned)
                        .ok_or_else(|| std::io::Error::other("application argument is not UTF-8"))).collect::<Result<_, _>>()?,
                    working_directory: command.get_current_dir().map(std::path::Path::to_path_buf), namespace_recipe: None,
                };
                return (ApplicationLaunchTurn { applications: &mut application_launches,
                    authentication: &mut authenticated_launches, wake: owner_wake.notifier() }).spawn(
                    &config, xauthority.path(), &format!("registered:{}", app.id), &effective,
                    crate::diagnostics::application::LaunchContext { source: crate::diagnostics::application::LaunchSource::Startup, transaction: None });
            }
            crate::diagnostics::application::spawn(
                &mut command,
                crate::diagnostics::application::LaunchContext {
                    source: crate::diagnostics::application::LaunchSource::Startup,
                    transaction: None,
                },
            ).map(crate::namespace_launch::ApplicationProcess::from)
        })
        .transpose()
    {
        Ok(child) => child,
        Err(error) if !config.startup_proof_requested() => {
            crate::session_eprintln!(
                "sophia_session_app schema=2 status=failed source=startup reason=spawn error={error}"
            );
            None
        }
        Err(error) => return Err(error.into()),
    };
    if child.is_some()
        && let Some(app) = normal_primary
    {
        crate::session_println!(
            "sophia_session_app schema=1 status={} id={} source=startup",
            if child.as_ref().is_some_and(|child| child.process_id().is_some()) { "started" } else { "preparing" }, app.id
        );
    }
    let mut process = SessionProcessGuard::new(
        child,
        Vec::new(),
        config.socket_path.clone(),
        config.normal_session,
    );
    let initial_authority_batch =
        startup_barrier::await_primary_startup_frame(&config, &authority_receiver, || {
            let _ = authenticated_launches.poll(Instant::now());
            if let Some(child) = process.child.as_mut() {
                child.pump(&mut authenticated_launches, owner_wake.notifier(), Instant::now());
            }
        })?;
    if config.secondary_terminal {
        process.add_secondary_child(
            None,
            spawn_secondary_xterm(
                terminal
                    .as_deref()
                    .expect("secondary terminal requires xterm"),
                &config.display,
                xauthority.path(),
                config.control_socket.as_deref(),
                config.inspection_socket.as_deref(),
                config
                    .inject_text
                    .as_deref()
                    .or(config.expect_physical_text.as_deref()),
            )?,
        );
    }
    for id in config.applications.startup.iter().skip(1) {
        let app = config
            .applications
            .applications
            .get(id)
            .expect("normal session startup application was validated");
        match (ApplicationLaunchTurn { applications: &mut application_launches,
            authentication: &mut authenticated_launches, wake: owner_wake.notifier() }).spawn_registered(
            &config, xauthority.path(), app,
            crate::diagnostics::application::LaunchContext {
                source: crate::diagnostics::application::LaunchSource::Startup,
                transaction: None,
            },
        ) {
            Ok(child) => {
                let status = if child.process_id().is_some() { "started" } else { "preparing" };
                process.add_secondary_child(Some(app.id.clone()), child);
                crate::session_println!(
                    "sophia_session_app schema=1 status={status} id={} source=startup",
                    app.id
                );
            }
            Err(error) if !config.startup_proof_requested() => {
                crate::session_eprintln!(
                    "sophia_session_app schema=2 status=failed id={} source=startup reason=spawn error={error}",
                    app.id
                );
            }
            Err(error) => return Err(error.into()),
        }
    }

    (process, client_stdout_capture, initial_authority_batch)
}

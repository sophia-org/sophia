use super::*;
use sophia_engine::PolicyProjectionReducer;
use sophia_protocol::{PolicyOutputSnapshot, SOPHIA_WM_CAPABILITY_POINTER_FOCUS};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};

struct TestPolicyChild(Child);
impl Drop for TestPolicyChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn hagia_pointer_focus_real_socket_commits_rejects_and_retries() {
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    for enabled in [false, true] {
        let directory = std::env::temp_dir().join(format!(
            "sophia-hagia-pointer-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = PolicyWmSessionTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let profile = directory.join("desktop.kdl");
        std::fs::write(
            &profile,
            format!("schema 1\npolicy {{ focus-follows-mouse #{}; }}\n", enabled),
        )
        .unwrap();
        std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut command = Command::new(&binary);
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("SOPHIA_")
                || name.to_string_lossy().starts_with("HAGIA_")
            {
                command.env_remove(name);
            }
        }
        command
            .arg(format!("--config={}", profile.display()))
            .arg(format!("--socket={}", transport.socket_path().display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        let child = TestPolicyChild(command.spawn().unwrap());
        transport.authorize_supervised_pid(child.0.id()).unwrap();
        transport
            .accept_and_negotiate(1, Duration::from_secs(4))
            .unwrap();
        assert_eq!(
            transport.selected_capabilities() & SOPHIA_WM_CAPABILITY_POINTER_FOCUS != 0,
            enabled
        );
        let sophia_runtime::PolicyClientEvent::Configuration {
            transaction,
            configuration,
        } = transport
            .receive_client_event_within(Duration::from_secs(4))
            .unwrap()
        else {
            panic!("missing configuration");
        };
        transport
            .send_configuration_outcome(
                transaction,
                configuration.generation,
                PolicyProjectionOutcome::Committed,
            )
            .unwrap();
        let mut initial = scene();
        let left = initial.outputs[0].output;
        let target = initial.surfaces[0].surface;
        let right = OutputId::from_raw(2);
        let mut bounds = initial.outputs[0].bounds;
        bounds.x = bounds.width;
        initial.outputs.push(PolicyOutputSnapshot {
            policy_key: None,
            output: right,
            generation: 1,
            focus: None,
            bounds,
            work_area: bounds,
        });
        let mut reducer = PolicyProjectionReducer::new(initial).unwrap();
        reducer.connect(1).unwrap();
        let scenarios = if enabled {
            vec![
                (PolicyRequestCause::SceneChanged, left, true),
                (
                    PolicyRequestCause::PointerFocus {
                        output: right,
                        target: None,
                    },
                    right,
                    true,
                ),
                (
                    PolicyRequestCause::PointerFocus {
                        output: left,
                        target: Some(target),
                    },
                    left,
                    false,
                ),
                (PolicyRequestCause::SceneChanged, right, true),
                (
                    PolicyRequestCause::PointerFocus {
                        output: left,
                        target: Some(target),
                    },
                    left,
                    true,
                ),
            ]
        } else {
            vec![(PolicyRequestCause::SceneChanged, left, true)]
        };
        for (index, (cause, expected, commit)) in scenarios.into_iter().enumerate() {
            let request = reducer
                .issue_request_with_cause(vec![left, right], cause)
                .unwrap();
            let snapshot = encode_wm_v1_policy_snapshot(
                TransactionId::from_raw(100 + index as u64 * 2),
                1,
                reducer.scene(),
                &[],
                &[],
                transport.selected_capabilities(),
            )
            .unwrap();
            transport
                .send_snapshot(
                    snapshot.transaction,
                    &snapshot.begin,
                    &snapshot.chunks,
                    &snapshot.end,
                )
                .unwrap();
            transport
                .send_projection_request(TransactionId::from_raw(101 + index as u64 * 2), &request)
                .unwrap();
            let proposal = loop {
                match transport
                    .receive_client_event_within(Duration::from_secs(4))
                    .unwrap()
                {
                    sophia_runtime::PolicyClientEvent::Projection(
                        QueuedPolicyProjection::Admitted(transfer),
                    ) => {
                        break decode_wm_v1_policy_projection(&transfer.into_wire_transfer())
                            .unwrap();
                    }
                    sophia_runtime::PolicyClientEvent::ProjectionPending => {}
                    _ => panic!("unexpected client event"),
                }
            };
            assert_eq!(proposal.active_output, expected);
            let before = reducer.scene().active_output;
            let staged = reducer.stage_proposal(&proposal).unwrap();
            assert_eq!(reducer.scene().active_output, before);
            let outcome = if commit {
                reducer.commit_staged(staged)
            } else {
                reducer.timeout(request.request_id)
            };
            assert_eq!(
                outcome,
                if commit {
                    PolicyProjectionOutcome::Committed
                } else {
                    PolicyProjectionOutcome::TimedOut
                }
            );
            assert_eq!(
                reducer.scene().active_output,
                if commit { expected } else { before }
            );
            transport
                .send_projection_outcome(
                    proposal.transaction,
                    request.request_id,
                    reducer.scene().generation,
                    outcome,
                )
                .unwrap();
        }
        transport.disconnect().unwrap();
        drop(child);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn hagia_pointer_focus_old_server_reports_the_setting_before_admission() {
    use sophia_protocol::{
        WmV1ServerWelcome, decode_wm_v1_client_hello_frame, encode_wm_v1_server_welcome_frame,
    };
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    let directory = std::env::temp_dir().join(format!(
        "sophia-hagia-old-pointer-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.join("socket");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    let profile = directory.join("desktop.kdl");
    std::fs::write(
        &profile,
        "schema 1\npolicy { focus-follows-mouse #true; }\n",
    )
    .unwrap();
    std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut command = Command::new(binary);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("SOPHIA_")
            || name.to_string_lossy().starts_with("HAGIA_")
        {
            command.env_remove(name);
        }
    }
    let mut child = TestPolicyChild(
        command
            .arg(format!("--config={}", profile.display()))
            .arg(format!("--socket={}", socket.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(error) => panic!("policy never connected: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let hello = decode_wm_v1_client_hello_frame(&read_frame(&mut stream)).unwrap();
    assert_ne!(hello.capabilities & SOPHIA_WM_CAPABILITY_POINTER_FOCUS, 0);
    let welcome = WmV1ServerWelcome {
        selected_revision: 3,
        capabilities: hello.capabilities & !SOPHIA_WM_CAPABILITY_POINTER_FOCUS,
        connection_epoch: 1,
        max_outputs: 16,
        max_bindings: 256,
        max_surfaces: 1024,
        max_chunk_bytes: 65500,
    };
    stream
        .write_all(&encode_wm_v1_server_welcome_frame(&welcome).unwrap())
        .unwrap();
    let mut extra = [0; 1];
    assert_eq!(
        stream.read(&mut extra).unwrap(),
        0,
        "unsupported profile must stop before sending configuration"
    );
    assert!(!child.0.wait().unwrap().success());
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(stderr.contains("focus-follows-mouse"), "{stderr}");
    drop(child);
    drop(listener);
    std::fs::remove_dir_all(directory).unwrap();
}

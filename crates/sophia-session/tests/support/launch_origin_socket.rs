//! Real X connectors exercise the production admission policy; no forged PID
//! property or synthetic admission is used to establish parentage.
use crate::launch_origin::LaunchOriginRegistry;
use crate::live_session::x_frontend::LiveXAdmissionPolicy;
use sophia_protocol::*;
use sophia_runtime::NamespaceRegistry;
use sophia_x_authority::*;
use std::collections::BTreeSet;
use std::io::{BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, WindowClass};
use x11rb::rust_connection::{DefaultStream, RustConnection};

const WAIT: Duration = Duration::from_secs(8);

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn connect(path: &std::path::Path) -> RustConnection<DefaultStream> {
    let socket = UnixStream::connect(path).unwrap();
    socket.set_read_timeout(Some(WAIT)).unwrap();
    socket.set_write_timeout(Some(WAIT)).unwrap();
    let (stream, _) = DefaultStream::from_unix_stream(socket).unwrap();
    RustConnection::connect_to_stream(stream, 0).unwrap()
}

fn map(connection: &RustConnection<DefaultStream>) {
    let window = connection.generate_id().unwrap();
    connection
        .create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            connection.setup().roots[0].root,
            0,
            0,
            300,
            180,
            0,
            WindowClass::INPUT_OUTPUT,
            x11rb::COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .unwrap()
        .check()
        .unwrap();
    connection.map_window(window).unwrap().check().unwrap();
    connection.get_input_focus().unwrap().reply().unwrap();
}

#[test]
fn x_child_fixture() {
    let Some(path) = std::env::var_os("SOPHIA_TEST_ORIGIN_X_SOCKET") else {
        return;
    };
    println!("ORIGIN_READY");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
    let connection = connect(std::path::Path::new(&path));
    println!("ORIGIN_CONNECTED");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
    map(&connection);
    println!("ORIGIN_MAPPED");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_exact(&mut [0]).unwrap();
}

#[test]
fn real_x_child_connection_freezes_authenticated_origin_before_delayed_map() {
    exercise_x_origin(None, false, false);
}

#[test]
fn hagia_real_x_child_origin_survives_monitor_switch_and_rejection() {
    let Some(binary) = std::env::var_os("SOPHIA_HAGIA_BIN") else {
        return;
    };
    for switch_before_connect in [false, true] {
        for hidden_workspace in [false, true] {
            exercise_x_origin(
                Some(binary.clone()),
                switch_before_connect,
                hidden_workspace,
            );
        }
    }
}

fn exercise_x_origin(
    hagia: Option<std::ffi::OsString>,
    switch_before_connect: bool,
    hidden_workspace: bool,
) {
    let directory = std::env::temp_dir().join(format!(
        "sophia-origin-x-{}-{}",
        std::process::id(),
        if hagia.is_some() { "hagia" } else { "unit" }
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let socket = directory.join("x");
    let mut namespaces = NamespaceRegistry::new(1).unwrap();
    let namespace =
        namespaces.create_namespace(NamespaceProfile::ClassicShared, NamespaceCapabilities::NONE);
    let origins = Arc::new(Mutex::new(LaunchOriginRegistry::default()));
    origins.lock().unwrap().set_epoch(1);
    let admission = Arc::new(LiveXAdmissionPolicy {
        launch_origins: origins.clone(),
        registry: Arc::new(Mutex::new(namespaces)),
        namespace: namespace.id,
        session_user_id: rustix::process::geteuid().as_raw(),
    });
    let mut frontend = XServerFrontend::bind(
        XServerFrontendConfig::new_with_namespace_context(&socket, namespace)
            .unwrap()
            .with_admission_policy(admission),
    )
    .unwrap();
    let (sender, receiver) = mpsc::channel();
    let registry = origins.clone();
    let seen = Mutex::new(BTreeSet::new());
    let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
        if let Some(batch) = XAuthorityObservedTransactionBatch::from_dispatch_observation(&trace) {
            for fact in &batch.surface_presentations {
                if fact.mapped
                    && fact.kind == LayoutNodeKind::Toplevel
                    && fact.role == SurfacePresentationRole::PolicyManaged
                    && let Some(route) = batch
                        .surface_routes
                        .iter()
                        .find(|r| r.surface == fact.surface)
                    && seen.lock().unwrap().insert(fact.surface)
                {
                    assert_eq!(
                        fact.owner, None,
                        "ordinary launch must not become a transient"
                    );
                    registry
                        .lock()
                        .unwrap()
                        .observe_toplevel(fact.surface, route.admission.unwrap());
                    sender.send(fact.surface).unwrap();
                }
            }
        }
        Ok(None)
    });
    let server = std::thread::spawn(move || {
        let broker = XServerFrontendRouteBroker::new(std::num::NonZeroUsize::new(4).unwrap());
        frontend
            .serve_next_concurrently_routed_traced(&broker, observer.clone())
            .unwrap();
        frontend
            .serve_next_concurrently_routed_traced(&broker, observer)
            .unwrap();
        frontend.wait_for_clients().unwrap();
    });
    let parent_connection = connect(&socket);
    map(&parent_connection);
    let parent = receiver.recv_timeout(WAIT).unwrap();
    let mut policy = hagia.map(|binary| PolicyFixture::new(binary, &directory, parent));
    let context = if let Some(policy) = &mut policy {
        policy
            .cycle(&origins, PolicyRequestCause::SceneChanged, true)
            .launch_contexts
            .into_iter()
            .find(|c| c.surface == parent)
            .unwrap()
    } else {
        PolicyLaunchContext {
            surface: parent,
            epoch: 1,
            token: 71,
        }
    };
    origins.lock().unwrap().publish(1, &[context]);
    origins.lock().unwrap().focused(Some(parent));
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "live_session::tests::launch_origin_socket::x_child_fixture",
                "--nocapture",
            ])
            .env("SOPHIA_TEST_ORIGIN_X_SOCKET", &socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (lines_tx, lines_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            if lines_tx.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let marker = |expected: &str| {
        loop {
            let line = lines_rx.recv_timeout(WAIT).unwrap();
            if line.contains(expected) {
                break;
            }
        }
    };
    marker("ORIGIN_READY");
    if switch_before_connect && let Some(policy) = &mut policy {
        policy.switch_away(&origins, hidden_workspace);
    }
    child.0.stdin.as_mut().unwrap().write_all(b"c").unwrap();
    marker("ORIGIN_CONNECTED");
    // The process is authenticated and its bookmark frozen, but it has not
    // created a window yet. A subsequent focus/output change cannot rewrite it.
    origins.lock().unwrap().focused(None);
    if !switch_before_connect && let Some(policy) = &mut policy {
        policy.switch_away(&origins, hidden_workspace);
    }
    child.0.stdin.as_mut().unwrap().write_all(b"m").unwrap();
    marker("ORIGIN_MAPPED");
    let surface = receiver.recv_timeout(WAIT).unwrap();
    assert_ne!(surface, parent);
    let frozen = origins.lock().unwrap().origins([surface]);
    assert_eq!(frozen, vec![PolicyLaunchContext { surface, ..context }]);
    if let Some(policy) = &mut policy {
        let mut scene = policy.reducer.scene().clone();
        scene.generation += 1;
        let mut child_surface = scene.surfaces[0];
        child_surface.surface = surface;
        child_surface.current_output = None;
        scene.surfaces.push(child_surface);
        policy.reducer.observe_scene(scene).unwrap();
        for commit in [false, true] {
            let proposal = policy.cycle(&origins, PolicyRequestCause::SceneChanged, commit);
            assert_eq!(proposal.active_output, OutputId::from_raw(2));
            for expected in [parent, surface] {
                assert_eq!(
                    proposal
                        .outputs
                        .iter()
                        .any(|o| o.output == OutputId::from_raw(1)
                            && o.placements.iter().any(|p| p.surface == expected)),
                    !hidden_workspace
                );
            }
            assert_eq!(
                origins.lock().unwrap().origins([surface]).is_empty(),
                commit
            );
        }
        if hidden_workspace {
            policy.cycle(
                &origins,
                PolicyRequestCause::PointerFocus {
                    output: OutputId::from_raw(1),
                    target: None,
                },
                true,
            );
            let action = policy.action("focus-workspace 1");
            let proposal = policy.cycle(
                &origins,
                PolicyRequestCause::Action {
                    activation_serial: 2,
                    action,
                },
                true,
            );
            for expected in [parent, surface] {
                assert!(
                    proposal
                        .outputs
                        .iter()
                        .any(|o| o.output == OutputId::from_raw(1)
                            && o.placements.iter().any(|p| p.surface == expected))
                );
            }
        }
    }
    child.0.stdin.as_mut().unwrap().write_all(b"q").unwrap();
    assert!(child.0.wait().unwrap().success());
    drop(parent_connection);
    server.join().unwrap();
    drop(policy);
    std::fs::remove_dir_all(directory).unwrap();
}

struct PolicyFixture {
    child: ChildGuard,
    transport: sophia_runtime::PolicyWmSessionTransport,
    reducer: sophia_engine::PolicyProjectionReducer,
    transaction: u64,
    actions: Vec<PolicyActionRegistration>,
}
impl PolicyFixture {
    fn new(binary: std::ffi::OsString, directory: &std::path::Path, parent: SurfaceId) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let mut transport = sophia_runtime::PolicyWmSessionTransport::bind_for_supervised_uid(
            directory.join("policy"),
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
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
        let child = ChildGuard(
            command
                .arg(format!("--config={}", profile.display()))
                .arg(format!("--socket={}", transport.socket_path().display()))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        transport.authorize_supervised_pid(child.0.id()).unwrap();
        transport.accept_and_negotiate(1, WAIT).unwrap();
        assert_ne!(
            transport.selected_capabilities() & SOPHIA_WM_CAPABILITY_LAUNCH_ORIGIN,
            0
        );
        let sophia_runtime::PolicyClientEvent::Configuration {
            transaction,
            configuration,
        } = transport.receive_client_event_within(WAIT).unwrap()
        else {
            panic!("missing configuration")
        };
        transport
            .send_configuration_outcome(
                transaction,
                configuration.generation,
                PolicyProjectionOutcome::Committed,
            )
            .unwrap();
        let bounds = Rect {
            x: 0,
            y: 0,
            width: 1280,
            height: 960,
        };
        let mut right = bounds;
        right.x = 1280;
        let scene = PolicySceneSnapshot {
            generation: 1,
            active_output: OutputId::from_raw(1),
            outputs: vec![
                PolicyOutputSnapshot {
                    policy_key: None,
                    output: OutputId::from_raw(1),
                    generation: 1,
                    focus: Some(parent),
                    bounds,
                    work_area: bounds,
                },
                PolicyOutputSnapshot {
                    policy_key: None,
                    output: OutputId::from_raw(2),
                    generation: 1,
                    focus: None,
                    bounds: right,
                    work_area: right,
                },
            ],
            surfaces: vec![PolicySurfaceSnapshot {
                surface: parent,
                generation: 1,
                current_output: Some(OutputId::from_raw(1)),
                kind: PolicySurfaceKind::Toplevel,
                capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                exact_size: None,
                requested_state: PolicyPresentationState::default(),
                current_state: PolicyPresentationState::default(),
                transient_owner: None,
                geometry: bounds,
            }],
            session_operations: vec![],
        };
        let mut reducer = sophia_engine::PolicyProjectionReducer::new(scene).unwrap();
        reducer.connect(1).unwrap();
        Self {
            child,
            transport,
            reducer,
            transaction: 100,
            // This fixture provides policy actions only; no session launch or
            // logout operation is advertised in its snapshot.
            actions: configuration
                .actions
                .into_iter()
                .filter(|action| action.session_operation_slot.is_none())
                .collect(),
        }
    }
    fn action(&self, name: &str) -> WmActionId {
        self.actions.iter().find(|a| a.name == name).unwrap().action
    }
    fn switch_away(&mut self, origins: &Arc<Mutex<LaunchOriginRegistry>>, hidden_workspace: bool) {
        if hidden_workspace {
            let action = self.action("focus-workspace 2");
            let proposal = self.cycle(
                origins,
                PolicyRequestCause::Action {
                    activation_serial: 1,
                    action,
                },
                true,
            );
            assert!(proposal.outputs.iter().all(|o| o.placements.is_empty()));
        }
        let proposal = self.cycle(
            origins,
            PolicyRequestCause::PointerFocus {
                output: OutputId::from_raw(2),
                target: None,
            },
            true,
        );
        assert_eq!(proposal.active_output, OutputId::from_raw(2));
    }
    fn cycle(
        &mut self,
        origins: &Arc<Mutex<LaunchOriginRegistry>>,
        cause: PolicyRequestCause,
        commit: bool,
    ) -> PolicyProjectionProposal {
        let request = self
            .reducer
            .issue_request_with_cause(vec![OutputId::from_raw(1), OutputId::from_raw(2)], cause)
            .unwrap();
        let pending = origins
            .lock()
            .unwrap()
            .origins(self.reducer.scene().surfaces.iter().map(|s| s.surface));
        let mut snapshot = encode_wm_v1_policy_snapshot(
            TransactionId::from_raw(self.transaction),
            1,
            self.reducer.scene(),
            &self.actions,
            &[],
            self.transport.selected_capabilities(),
        )
        .unwrap();
        append_wm_launch_origins(
            &mut snapshot,
            &pending,
            self.transport.selected_capabilities(),
        )
        .unwrap();
        self.transport
            .send_snapshot(
                snapshot.transaction,
                &snapshot.begin,
                &snapshot.chunks,
                &snapshot.end,
            )
            .unwrap();
        self.transport
            .send_projection_request(TransactionId::from_raw(self.transaction + 1), &request)
            .unwrap();
        self.transaction += 2;
        let proposal = loop {
            match self.transport.receive_client_event_within(WAIT).unwrap() {
                sophia_runtime::PolicyClientEvent::Projection(
                    sophia_runtime::QueuedPolicyProjection::Admitted(transfer),
                ) => break decode_wm_v1_policy_projection(&transfer.into_wire_transfer()).unwrap(),
                sophia_runtime::PolicyClientEvent::ProjectionPending => {}
                event => panic!("unexpected event: {event:?}"),
            }
        };
        let staged = self.reducer.stage_proposal(&proposal).unwrap();
        let outcome = if commit {
            self.reducer.commit_staged(staged)
        } else {
            self.reducer.timeout(request.request_id)
        };
        assert_eq!(
            outcome,
            if commit {
                PolicyProjectionOutcome::Committed
            } else {
                PolicyProjectionOutcome::TimedOut
            }
        );
        if commit {
            let mut origins = origins.lock().unwrap();
            origins.publish(1, &proposal.launch_contexts);
            origins.committed(pending.iter().map(|c| c.surface));
        }
        self.transport
            .send_projection_outcome(
                proposal.transaction,
                request.request_id,
                self.reducer.scene().generation,
                outcome,
            )
            .unwrap();
        proposal
    }
}
impl Drop for PolicyFixture {
    fn drop(&mut self) {
        let _ = self.transport.disconnect();
        let _ = self.child.0.kill();
    }
}

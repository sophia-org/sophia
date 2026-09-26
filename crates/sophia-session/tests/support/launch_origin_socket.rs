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
    exercise_x_origin::<NoPolicy>("unit", |_, _| None, false, false);
}

/// A window-manager policy client the origin exercise may drive. Sophia's own
/// case runs without one; WM-specific pairings live outside this repository and
/// mount their implementation as a child of this module.
trait OriginPolicy {
    /// The launcher context the policy publishes for `parent`.
    fn launcher_context(
        &mut self,
        origins: &Arc<Mutex<LaunchOriginRegistry>>,
        parent: SurfaceId,
    ) -> PolicyLaunchContext;
    /// Move focus away from the launcher's output, and optionally its view.
    fn switch_away(&mut self, origins: &Arc<Mutex<LaunchOriginRegistry>>, hidden_workspace: bool);
    /// Judge the policy's placement of the mapped child.
    fn place_child(
        &mut self,
        origins: &Arc<Mutex<LaunchOriginRegistry>>,
        parent: SurfaceId,
        surface: SurfaceId,
        hidden_workspace: bool,
    );
}

/// No policy client: the origin is judged by Session's registry alone.
enum NoPolicy {}

impl OriginPolicy for NoPolicy {
    fn launcher_context(
        &mut self,
        _: &Arc<Mutex<LaunchOriginRegistry>>,
        _: SurfaceId,
    ) -> PolicyLaunchContext {
        match *self {}
    }

    fn switch_away(&mut self, _: &Arc<Mutex<LaunchOriginRegistry>>, _: bool) {
        match *self {}
    }

    fn place_child(
        &mut self,
        _: &Arc<Mutex<LaunchOriginRegistry>>,
        _: SurfaceId,
        _: SurfaceId,
        _: bool,
    ) {
        match *self {}
    }
}

fn exercise_x_origin<P: OriginPolicy>(
    label: &str,
    make_policy: impl FnOnce(&std::path::Path, SurfaceId) -> Option<P>,
    switch_before_connect: bool,
    hidden_workspace: bool,
) {
    let directory =
        std::env::temp_dir().join(format!("sophia-origin-x-{}-{label}", std::process::id()));
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
    let mut policy = make_policy(&directory, parent);
    let context = if let Some(policy) = &mut policy {
        policy.launcher_context(&origins, parent)
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
        policy.place_child(&origins, parent, surface, hidden_workspace);
    }
    child.0.stdin.as_mut().unwrap().write_all(b"q").unwrap();
    assert!(child.0.wait().unwrap().success());
    drop(parent_connection);
    server.join().unwrap();
    drop(policy);
    std::fs::remove_dir_all(directory).unwrap();
}

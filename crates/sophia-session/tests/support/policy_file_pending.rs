//! Actual protected Rust child and production endpoint credentials. Supervisor
//! evidence is not an independent namespace inventory or a native proof.
#![cfg(test)]
use super::super::runtime_adapter::NinePPolicyAdapter;
use super::super::startup::tests::{Peer, negotiate};
use super::*;
use crate::live_session::policy_transport_worker::{PolicyTransportEvent, PolicyTransportWorker};
use sophia_runtime::{
    ProcessLaunchSpec, ProtectionDomainSpec, ProtectionPath, SupervisedProcessKind,
    SupervisorCommand,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;

const CHILD: &str =
    "live_session::policy_transport_worker::ninep::pending::tests::protected_endpoint_child";
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "swmp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn endpoint(&self, role: PolicyRole) -> PolicyRoleEndpoint {
        PolicyRoleEndpoint::bind_role_for_supervised_uid(
            self.0.join("endpoint"),
            role,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap()
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn child(socket: &Path, mode: &str, role: Option<ProtectionDomainRole>) -> ProcessSupervisor {
    let executable = std::env::current_exe().unwrap();
    let markers = socket.parent().unwrap().parent().unwrap().join("markers");
    std::fs::create_dir(&markers).unwrap();
    let mut spec = ProcessLaunchSpec::new(executable)
        .arg("--exact")
        .arg(CHILD)
        .arg("--ignored")
        .arg("--nocapture")
        .env("SOPHIA_PENDING_TEST_SOCKET", socket)
        .env("SOPHIA_PENDING_TEST_MODE", mode)
        .env("SOPHIA_PENDING_TEST_MARKERS", &markers);
    if let Some(role) = role {
        spec = spec.protection_domain(
            ProtectionDomainSpec::bubblewrap([role])
                .unwrap()
                .path(ProtectionPath::read_only(socket.parent().unwrap()))
                .unwrap()
                .path(ProtectionPath::read_write(&markers))
                .unwrap(),
        );
    }
    let process = if role == Some(ProtectionDomainRole::MetadataBroker) {
        SupervisedProcessKind::MetadataBroker
    } else {
        SupervisedProcessKind::WindowManager
    };
    let mut supervisor = ProcessSupervisor::new(process, spec);
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process,
            delay: Duration::ZERO,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while !markers.join("ready").exists() && Instant::now() < deadline {
        assert!(
            supervisor.poll().unwrap().is_none(),
            "child exited before fixture ready"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(markers.join("ready").exists(), "child readiness deadline");
    supervisor
}

#[test]
#[ignore = "child fixture, invoked only by its supervised parent control"]
fn protected_endpoint_child() {
    let mode = std::env::var("SOPHIA_PENDING_TEST_MODE").expect("parent mode required");
    let markers =
        PathBuf::from(std::env::var_os("SOPHIA_PENDING_TEST_MARKERS").expect("markers required"));
    std::fs::write(markers.join("ready"), b"ready").unwrap();
    if mode == "idle" {
        std::thread::sleep(Duration::from_secs(10));
        return;
    }
    let socket = std::env::var_os("SOPHIA_PENDING_TEST_SOCKET").expect("parent socket required");
    let stream = UnixStream::connect(socket).unwrap();
    if mode == "negotiate" {
        let mut peer = Peer::from_stream(stream);
        negotiate(
            &mut peer,
            sophia_protocol::SOPHIA_WM_CAPABILITY_CONFIGURATION,
        );
        std::fs::write(markers.join("negotiated"), b"complete").unwrap();
    } else {
        std::thread::sleep(Duration::from_secs(10));
    }
}

#[test]
fn pending_endpoint_requires_wm_role_and_supervisor_protection_evidence() {
    for (endpoint_role, protected_role, expected) in [
        (
            PolicyRole::Output,
            Some(ProtectionDomainRole::SpatialPolicy),
            "WM role",
        ),
        (PolicyRole::Wm, None, "protection evidence"),
        (
            PolicyRole::Wm,
            Some(ProtectionDomainRole::MetadataBroker),
            "spatial peer",
        ),
    ] {
        let directory = Directory::new();
        let endpoint = directory.endpoint(endpoint_role);
        let socket = endpoint.socket_path().to_owned();
        let supervisor = child(&socket, "idle", protected_role);
        let error = PendingEndpoint::authorize(endpoint, &supervisor, WmQids::new())
            .err()
            .expect("must refuse");
        assert!(error.contains(expected), "{error}");
        assert!(!socket.exists());
    }
}

#[test]
fn actual_protected_child_reaches_existing_worker_negotiation() {
    let directory = Directory::new();
    let endpoint = directory.endpoint(PolicyRole::Wm);
    let socket = endpoint.socket_path().to_owned();
    let supervisor = child(
        &socket,
        "negotiate",
        Some(ProtectionDomainRole::SpatialPolicy),
    );
    assert_eq!(
        supervisor.peer_id(),
        Some(supervisor.protection_evidence().unwrap().peer_pid)
    );
    let adapter = NinePPolicyAdapter::pending(
        endpoint,
        &supervisor,
        9,
        WmFileLimits {
            capability_ceiling: sophia_protocol::SOPHIA_WM_CAPABILITY_CONFIGURATION,
            profile_required: false,
        },
        WmQids::new(),
    )
    .unwrap();
    let worker = PolicyTransportWorker::spawn(adapter, 9, None).unwrap();
    assert!(matches!(
        worker.event_timeout(Duration::from_secs(5)).unwrap(),
        PolicyTransportEvent::Negotiated
    ));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !directory.0.join("markers/negotiated").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        directory.0.join("markers/negotiated").exists(),
        "child must drain negotiated ACK"
    );
    drop(worker);
    assert!(!socket.exists());
}

#[test]
fn wrong_process_terminates_the_accept_attempt_without_retrying() {
    let directory = Directory::new();
    let endpoint = directory.endpoint(PolicyRole::Wm);
    let socket = endpoint.socket_path().to_owned();
    let supervisor = child(&socket, "idle", Some(ProtectionDomainRole::SpatialPolicy));
    let mut pending = PendingEndpoint::authorize(endpoint, &supervisor, WmQids::new()).unwrap();
    let _wrong = UnixStream::connect(&socket).unwrap();
    let started = Instant::now();
    let error = pending
        .accept(&NinePCancellation::new(), started + Duration::from_secs(1))
        .err()
        .expect("wrong peer refused");
    assert!(error.contains("UnauthorizedPeer"), "{error}");
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(pending.endpoint.active_peer().is_none());
    drop(pending);
    assert!(!socket.exists());
}

#[test]
fn pending_stop_and_absolute_accept_expiry_leave_no_peer_or_endpoint() {
    for stop in [true, false] {
        let directory = Directory::new();
        let endpoint = directory.endpoint(PolicyRole::Wm);
        let socket = endpoint.socket_path().to_owned();
        let supervisor = child(&socket, "idle", Some(ProtectionDomainRole::SpatialPolicy));
        let mut pending = PendingEndpoint::authorize(endpoint, &supervisor, WmQids::new()).unwrap();
        let cancellation = NinePCancellation::new();
        if stop {
            cancellation.handle().stop();
        }
        let started = Instant::now();
        let error = pending
            .accept(&cancellation, started + Duration::from_millis(30))
            .err()
            .expect("accept refused");
        assert!(error.contains(if stop { "125" } else { "110" }), "{error}");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(pending.endpoint.active_peer().is_none());
        drop(pending);
        assert!(!socket.exists());
    }
}

#[test]
fn one_cancellation_survives_stop_before_and_after_wake_installation() {
    for stop_first in [true, false] {
        let cancellation = NinePCancellation::new();
        let stop = cancellation.handle();
        let owner = WmFiles::awaiting_negotiation(
            9,
            WmFileLimits {
                capability_ceiling: 0,
                profile_required: false,
            },
            WmQids::new(),
            super::super::typed_codec::TypedFileCodec,
        )
        .unwrap();
        let (server, _client) = UnixStream::pair().unwrap();
        if stop_first {
            stop.stop();
        }
        let reactor = NinePReactor::adopt_with_cancellation(server, owner, cancellation.clone());
        if stop_first {
            assert!(reactor.is_err());
        } else {
            let mut reactor = reactor.unwrap();
            stop.stop();
            assert!(reactor.turn(Duration::from_secs(1)).is_err());
            assert!(Arc::ptr_eq(&reactor.stopped, &cancellation.stopped));
        }
        assert!(cancellation.stopped.load(Ordering::SeqCst));
    }
}

#[test]
fn stop_racing_wake_registration_is_not_lost() {
    let cancellation = NinePCancellation::new();
    let owner = WmFiles::awaiting_negotiation(
        9,
        WmFileLimits {
            capability_ceiling: 0,
            profile_required: false,
        },
        WmQids::new(),
        super::super::typed_codec::TypedFileCodec,
    )
    .unwrap();
    let server = Server::new(owner, Limits::new(65536, 512, 16, 32, 131072, 1).unwrap()).unwrap();
    let wake = server.wake();
    // Hold only the registration lock, forcing Stop and installation to
    // overlap. Neither path may wait for any socket operation under this lock.
    let held = cancellation.wake.lock().unwrap();
    let installing = cancellation.clone();
    let install = std::thread::spawn(move || installing.install(wake));
    let stop = cancellation.handle();
    let stopping = std::thread::spawn(move || stop.stop());
    let deadline = Instant::now() + Duration::from_secs(1);
    while !cancellation.stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::yield_now();
    }
    let stopped = cancellation.stopped.load(Ordering::SeqCst);
    drop(held);
    stopping.join().unwrap();
    assert!(stopped);
    assert!(install.join().unwrap().is_err());
}

#[test]
fn protected_reconnect_keeps_the_supplied_logical_qid_allocator() {
    let qids = WmQids::new();
    let mut previous = 0;
    for epoch in [9, 10] {
        let directory = Directory::new();
        let endpoint = directory.endpoint(PolicyRole::Wm);
        let socket = endpoint.socket_path().to_owned();
        let supervisor = child(
            &socket,
            "connect",
            Some(ProtectionDomainRole::SpatialPolicy),
        );
        let mut pending = PendingEndpoint::authorize(endpoint, &supervisor, qids.clone()).unwrap();
        let (_stream, supplied_qids) = pending
            .accept(
                &NinePCancellation::new(),
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(
            pending.endpoint.active_peer().unwrap().pid,
            supervisor.peer_id().unwrap()
        );
        let owner = WmFiles::awaiting_negotiation(
            epoch,
            WmFileLimits {
                capability_ceiling: 0,
                profile_required: false,
            },
            supplied_qids.clone(),
            super::super::typed_codec::TypedFileCodec,
        )
        .unwrap();
        let next = supplied_qids.allocate(1).unwrap();
        assert!(next > previous);
        previous = next;
        drop(owner);
        drop(pending);
        assert!(!socket.exists());
    }
}

#[test]
fn actual_child_wrong_uid_is_refused_despite_matching_supervisor_pid() {
    let directory = Directory::new();
    let wrong_uid = rustix::process::geteuid().as_raw().checked_add(1).unwrap();
    let endpoint = PolicyRoleEndpoint::bind_role_for_supervised_uid(
        directory.0.join("endpoint"),
        PolicyRole::Wm,
        wrong_uid,
    )
    .unwrap();
    let socket = endpoint.socket_path().to_owned();
    let supervisor = child(
        &socket,
        "connect",
        Some(ProtectionDomainRole::SpatialPolicy),
    );
    let mut pending = PendingEndpoint::authorize(endpoint, &supervisor, WmQids::new()).unwrap();
    let error = pending
        .accept(
            &NinePCancellation::new(),
            Instant::now() + Duration::from_secs(2),
        )
        .err()
        .expect("wrong uid refused");
    assert!(error.contains("UnauthorizedPeer"), "{error}");
    assert!(pending.endpoint.active_peer().is_none());
}

#[test]
fn stop_wakes_the_pending_accept_without_waiting_for_its_deadline() {
    let directory = Directory::new();
    let endpoint = directory.endpoint(PolicyRole::Wm);
    let socket = endpoint.socket_path().to_owned();
    let supervisor = child(&socket, "idle", Some(ProtectionDomainRole::SpatialPolicy));
    let mut pending = PendingEndpoint::authorize(endpoint, &supervisor, WmQids::new()).unwrap();
    let cancellation = NinePCancellation::new();
    let stop = cancellation.handle();
    let (started, running) = std::sync::mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        started.send(()).unwrap();
        pending
            .accept(&cancellation, Instant::now() + Duration::from_secs(12))
            .err()
    });
    running.recv_timeout(Duration::from_secs(1)).unwrap();
    let before = Instant::now();
    stop.stop();
    assert!(thread.join().unwrap().unwrap().contains("125"));
    assert!(before.elapsed() < Duration::from_secs(1));
    assert!(!socket.exists());
}

#[test]
fn stop_after_credential_accept_refuses_adoption_and_releases_only_owned_endpoint() {
    let directory = Directory::new();
    let endpoint = directory.endpoint(PolicyRole::Wm);
    let socket = endpoint.socket_path().to_owned();
    let supervisor = child(
        &socket,
        "connect",
        Some(ProtectionDomainRole::SpatialPolicy),
    );
    let mut pending = PendingEndpoint::authorize(endpoint, &supervisor, WmQids::new()).unwrap();
    let cancellation = NinePCancellation::new();
    let (stream, qids) = pending
        .accept(&cancellation, Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(
        pending.endpoint.active_peer().unwrap().pid,
        supervisor.peer_id().unwrap()
    );
    cancellation.handle().stop();
    let owner = WmFiles::awaiting_negotiation(
        9,
        WmFileLimits {
            capability_ceiling: 0,
            profile_required: false,
        },
        qids,
        super::super::typed_codec::TypedFileCodec,
    )
    .unwrap();
    assert!(NinePReactor::adopt_with_cancellation(stream, owner, cancellation).is_err());
    // Same endpoint Drop used by FileStartup::close after failed adoption.
    drop(pending);
    assert!(!socket.exists());
    assert!(directory.0.join("markers/ready").exists());
    let replacement = directory.endpoint(PolicyRole::Wm);
    assert!(replacement.active_peer().is_none());
}

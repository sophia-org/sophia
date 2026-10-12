//! Real protected launch and Factotum admission; WM semantics use the raw peer.
use super::*;
use crate::launch_authentication::{
    LaunchPolicy,
    startup::{Recipe, Startup},
};
use crate::live_session::tests::lock_provider_factotum as factotum;

const AUTH_CHILD: &str = "live_session::policy_transport_worker::ninep::pending::tests::authentication::authenticated_wm_child";

#[test]
#[ignore = "protected child, invoked by authenticated WM parent"]
fn authenticated_wm_child() {
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let stream = UnixStream::connect(std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap()).unwrap();
    let mut client = sophia_launch_client::Client::over(
        stream.try_clone().unwrap(),
        relay,
        b"wm",
        b"spatial-policy",
    )
    .unwrap();
    while !client.ready() {
        client
            .poll()
            .expect("protected WM must authenticate before file negotiation");
        client.wait_for_io(Duration::from_millis(10)).unwrap();
    }
    let admitted = client.into_authenticated().unwrap();
    let root = admitted.root.0;
    drop(admitted);
    stream.set_nonblocking(false).unwrap();
    let mut peer = Peer::from_stream(stream);
    // Transfer the quiescent SDK root to the raw file fixture; never reattach.
    if root != 1 {
        assert_eq!(
            peer.rpc(
                110,
                &[
                    root.to_le_bytes().as_slice(),
                    &1u32.to_le_bytes(),
                    &0u16.to_le_bytes(),
                ]
                .concat()
            )
            .unwrap()
            .0,
            111
        );
        assert_eq!(peer.rpc(120, &root.to_le_bytes()).unwrap().0, 121);
    }
    peer.open(2, b"events", 0);
    peer.open(3, b"submit", 1);
    peer.open(4, b"ack", 1);
    super::super::super::startup::tests::negotiate_opened(
        &mut peer,
        sophia_protocol::SOPHIA_WM_CAPABILITY_CONFIGURATION,
        9,
    );
    std::fs::write(
        std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap(),
        b"authenticated-and-negotiated",
    )
    .unwrap();
}

#[test]
fn protected_wm_authenticates_then_negotiates_on_the_same_worker_connection() {
    let directory = Directory::new();
    let endpoint = directory.endpoint(PolicyRole::Wm);
    let socket = endpoint.socket_path().to_owned();
    let markers = directory.0.join("markers");
    std::fs::create_dir(&markers).unwrap();
    let report = markers.join("complete");
    let executable = std::env::current_exe().unwrap();
    let bootstrap = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").unwrap())
        .join(format!("tmp/t317-wm-auth-{}", std::process::id()));
    std::fs::create_dir_all(bootstrap.parent().unwrap()).unwrap();
    assert!(
        std::process::Command::new("/usr/bin/cc")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
            .arg(format!("-DSOPHIA_TEST_CHILD=\"{AUTH_CHILD}\""))
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/lock_rust_bootstrap.c"
            ))
            .arg("-o")
            .arg(&bootstrap)
            .status()
            .unwrap()
            .success()
    );
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::SpatialPolicy])
        .unwrap()
        .path(ProtectionPath::read_only(&executable))
        .unwrap()
        .path(ProtectionPath::read_only(socket.parent().unwrap()))
        .unwrap()
        .path(ProtectionPath::read_write(&markers))
        .unwrap();
    let supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new(&bootstrap)
            .arg(&executable)
            .arg(&socket)
            .arg(&report)
            .protection_domain(domain),
    );
    let mut auth = factotum::Fixture::new();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut startup = Startup::new(
        supervisor,
        sophia_runtime::SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO,
        },
        auth.coordinator.endpoint(),
        Recipe {
            policy: LaunchPolicy {
                principal: 3,
                namespace: 2,
                audience: 3,
                role: ProtectionDomainRole::SpatialPolicy,
            },
            uname: b"wm".to_vec(),
            aname: b"spatial-policy".to_vec(),
        },
        Instant::now(),
    )
    .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let started = loop {
        assert!(Instant::now() < end, "authenticated WM launch deadline");
        auth.tick();
        if let Some(started) = startup
            .poll(&mut auth.coordinator, wake.notifier(), Instant::now())
            .unwrap()
        {
            break started;
        }
    };
    let mut supervisor = started.supervisor;
    let _lease = started.lease;
    let adapter = NinePPolicyAdapter::pending(
        endpoint,
        &supervisor,
        9,
        WmFileLimits {
            capability_ceiling: sophia_protocol::SOPHIA_WM_CAPABILITY_CONFIGURATION,
            profile_required: false,
        },
        WmQids::new(),
        Some(started.factory),
    )
    .unwrap();
    let worker = PolicyTransportWorker::spawn(adapter, 9, None).unwrap();
    let mut negotiated = false;
    while !report.exists() || !negotiated {
        assert!(
            Instant::now() < end,
            "authenticated WM negotiation deadline"
        );
        auth.tick();
        if !negotiated && let Some(event) = worker.try_event().unwrap() {
            match event {
                PolicyTransportEvent::Negotiated => (),
                PolicyTransportEvent::Failed(error) => panic!("WM authentication failed: {error}"),
                _ => panic!("unexpected WM event before negotiation"),
            }
            negotiated = true;
        }
    }
    assert_eq!(
        std::fs::read(&report).unwrap(),
        b"authenticated-and-negotiated"
    );
    drop(worker);
    for _ in 0..10 {
        auth.tick();
    }
    assert!(
        auth.coordinator.ready(),
        "WM cleanup must preserve shared Factotum"
    );
    auth.coordinator
        .revoke(started.registration, Instant::now())
        .unwrap();
    supervisor.terminate().unwrap();
}

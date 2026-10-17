//! Normal provider owner, real Factotum, protected SDK peer and actual service.
use super::*;
use std::{
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::PathBuf,
};

struct Directory(PathBuf);
impl Directory {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("t317-provider-{label}-{}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Peer(PathBuf);
impl Peer {
    fn build() -> Self {
        let target =
            PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").expect("cargo-slot target"));
        let temporary = target.join("tmp");
        std::fs::create_dir_all(&temporary).unwrap();
        // One owned directory per caller: tests in this module run in
        // parallel and must not overwrite a peer another test is
        // executing. The directory is removed with the Peer.
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let owned = temporary.join(format!("t317-provider-sdk-{}-{serial}", std::process::id()));
        std::fs::create_dir_all(&owned).unwrap();
        let path = owned.join("peer");
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/c-desktop-sdk/source/src");
        let mut units: Vec<_> = [
            "nine_p",
            "launch_auth",
            "lock_files",
            "image_channel",
            "image_producer",
        ]
        .into_iter()
        .flat_map(|dir| std::fs::read_dir(source.join(dir)).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "c"))
        .collect();
        units.sort();
        assert!(
            std::process::Command::new("/usr/bin/cc")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .args([
                    "-std=c11",
                    "-Wall",
                    "-Wextra",
                    "-Werror",
                    "-O2",
                    "-DSOPHIA_TEST_C_SDK"
                ])
                .arg("-I")
                .arg(source)
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/support/lock_authentication_peer.c"
                ))
                .args(units)
                .arg("-o")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        Self(path)
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        // The binary lives in its own per-build directory.
        if let Some(owned) = self.0.parent() {
            let _ = std::fs::remove_dir_all(owned);
        }
    }
}

fn step(provider: &mut LockProvider, auth: &mut factotum::Fixture) -> Vec<LockFileServiceEvent> {
    auth.tick();
    let events = provider.poll(Instant::now(), &mut auth.coordinator);
    if let Some(status) = provider
        .supervisor
        .as_ref()
        .and_then(ProcessSupervisor::exit_status)
    {
        assert!(
            status.success(),
            "protected SDK peer must complete authenticated lock bootstrap: {status}"
        );
    }
    events
}

#[test]
fn normal_provider_owner_authenticates_and_recovers_after_factotum_loss() {
    let peer = Peer::build();
    let directory = Directory::new("sdk");
    let reports = directory.0.join("reports");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let report = reports.join("progress");
    let transport = LockFileTransport::bind_for_supervised_uid(
        directory.0.join("endpoint"),
        rustix::process::geteuid().as_raw(),
        1,
        limits(),
    )
    .unwrap();
    let socket = transport.socket_path().to_owned();
    let base = ProcessLaunchSpec::new(&peer.0)
        .arg(&socket)
        .arg(&report)
        .process_group()
        .protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider])
                .unwrap()
                .path(ProtectionPath::read_only(socket.parent().unwrap()))
                .unwrap()
                .path(ProtectionPath::read_write(&reports))
                .unwrap(),
        );
    let mut auth = factotum::Fixture::new();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut provider = LockProvider::with_service(
        transport,
        base,
        ShellGpuMode::Denied,
        None,
        locked(),
        Vec::new(),
        wake.notifier(),
        policy(),
        auth.coordinator.endpoint(),
    )
    .unwrap();
    let mut connected = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            Instant::now() < deadline,
            "normal provider must reach SDK lock bootstrap"
        );
        for event in step(&mut provider, &mut auth) {
            if let LockFileServiceEvent::Connected {
                connection_epoch, ..
            } = event
            {
                connected.push(connection_epoch);
            }
        }
        if connected == [1] && std::fs::read_to_string(&report).ok().as_deref() == Some("waiting\n")
        {
            break;
        }
    }
    assert!(provider.authorized);
    let first = provider.supervisor.as_ref().unwrap().child_id().unwrap();
    let registration = provider.registration.unwrap();
    assert_eq!(auth.coordinator.phase(registration), Some(Phase::Active));

    // Stop only the shared connection. The worker must revoke the live role
    // without an owner poll, process reap or retirement command from Session.
    auth._authenticator
        .as_ref()
        .unwrap()
        .launch_service()
        .current()
        .unwrap()
        .ingress
        .invalidate();
    let deadline = Instant::now() + Duration::from_secs(5);
    while std::fs::read_to_string(&report).ok().as_deref() != Some("revoked\n") {
        assert!(
            Instant::now() < deadline,
            "Factotum loss must revoke the SDK role before owner progress"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    // Observe wrapper exit without reaping: the normal provider owner must
    // consume the exit and drive succession itself when it resumes.
    let pidfd = rustix::process::pidfd_open(
        rustix::process::Pid::from_raw(first as i32).unwrap(),
        rustix::process::PidfdFlags::empty(),
    )
    .unwrap();
    let mut fds = [rustix::event::PollFd::new(
        &pidfd,
        rustix::event::PollFlags::IN,
    )];
    while rustix::event::poll(
        &mut fds,
        Some(&rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )
    .unwrap()
        == 0
    {
        assert!(Instant::now() < deadline, "revoked SDK peer exit deadline");
        std::thread::sleep(Duration::from_millis(1));
    }
    std::fs::remove_file(&report).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut revoked_images = false;
    loop {
        assert!(
            Instant::now() < deadline,
            "normal provider must recover through the replacement Factotum"
        );
        for event in step(&mut provider, &mut auth) {
            if let LockFileServiceEvent::Connected {
                connection_epoch, ..
            } = event
            {
                connected.push(connection_epoch);
            }
        }
        revoked_images |= provider.take_replacement();
        if connected.len() == 2
            && std::fs::read_to_string(&report).ok().as_deref() == Some("waiting\n")
        {
            break;
        }
    }
    assert_eq!(connected, [1, 2]);
    assert!(
        revoked_images,
        "authority loss must revoke the old provider's images and chords"
    );
    assert_ne!(provider.registration, Some(registration));
    assert_ne!(
        provider.supervisor.as_ref().unwrap().child_id(),
        Some(first)
    );
    assert!(provider.authorized);
    provider.replace_process(Instant::now(), &mut auth.coordinator);
    assert_eq!(auth.coordinator.phase(registration), None);
}

#[test]
fn unavailable_factotum_never_falls_back_to_an_ordinary_provider_launch() {
    let mut provider = idle_provider("no-factotum");
    let mut launches = LaunchCoordinator::new(Default::default(), factotum::namespaces());
    assert!(provider.poll(Instant::now(), &mut launches).is_empty());
    assert!(provider.supervisor.as_ref().unwrap().child_id().is_none());
    assert!(provider.registration.is_none());
    assert_eq!(provider.grant_epoch, 0);
}

#[test]
fn exhausted_launch_capacity_keeps_the_locked_provider_unavailable_and_backs_off() {
    let mut auth = factotum::Fixture::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    for _ in 0..64 {
        let registration = auth.coordinator.reserve(policy(), Instant::now()).unwrap();
        while auth.coordinator.phase(registration) != Some(Phase::Ready) {
            assert!(Instant::now() < deadline, "launch quota setup deadline");
            auth.tick();
        }
    }
    // This fixture's service starts with the Locked object, without a renderer.
    let mut provider = idle_provider("full-launch-quota");
    let now = Instant::now();
    assert!(provider.poll(now, &mut auth.coordinator).is_empty());
    assert!(provider.registration.is_none());
    assert!(provider.spawning.is_none());
    assert!(
        provider.supervisor.as_ref().unwrap().child_id().is_none(),
        "exhausted launch quota must not start an unauthenticated provider"
    );
    assert!(!provider.authorized && provider.assignee.is_none());
    let retry = provider
        .restart_at
        .expect("quota refusal retains a bounded retry");
    assert!(
        retry > now,
        "quota refusal must back off instead of spinning"
    );
    for _ in 0..64 {
        assert!(provider.poll(now, &mut auth.coordinator).is_empty());
        assert_eq!(provider.restart_at, Some(retry));
        assert!(provider.registration.is_none() && provider.spawning.is_none());
    }
}

#[test]
fn slow_protected_spawn_does_not_block_and_cancelled_completion_cannot_authorize() {
    let directory = Directory::new("slow");
    let bubblewrap = directory.0.join("bwrap");
    std::fs::write(&bubblewrap, "#!/bin/sh\nif [ \"$1\" = --version ]; then /usr/bin/sleep 2; fi\nexec /usr/bin/bwrap \"$@\"\n").unwrap();
    std::fs::set_permissions(&bubblewrap, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut auth = factotum::Fixture::new();
    let transport = LockFileTransport::bind_for_supervised_uid(
        directory.0.join("endpoint"),
        rustix::process::geteuid().as_raw(),
        1,
        limits(),
    )
    .unwrap();
    let base = ProcessLaunchSpec::new("/usr/bin/sleep")
        .arg("30")
        .protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider])
                .unwrap()
                .bubblewrap_path(&bubblewrap),
        );
    let wake = sophia_wake::Wake::new().unwrap();
    let mut provider = LockProvider::with_service(
        transport,
        base,
        ShellGpuMode::Denied,
        None,
        locked(),
        Vec::new(),
        wake.notifier(),
        policy(),
        auth.coordinator.endpoint(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while provider.spawning.is_none() {
        assert!(Instant::now() < deadline, "protected spawn start deadline");
        auth.tick();
        provider.poll(Instant::now(), &mut auth.coordinator);
    }
    let start = Instant::now();
    provider.poll(start, &mut auth.coordinator);
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "owner poll must not wait for protected spawn completion"
    );
    let old = provider.registration.unwrap();
    provider.replace_process(Instant::now(), &mut auth.coordinator);
    assert_eq!(auth.coordinator.phase(old), Some(Phase::Retiring));
    // Suppress this test's next launch, but continue the real cancellation/reap.
    provider.restart_at = Some(deadline + Duration::from_secs(10));
    while provider.spawning.is_some()
        || provider
            .supervisor
            .as_ref()
            .and_then(ProcessSupervisor::child_id)
            .is_some()
    {
        assert!(
            Instant::now() < deadline,
            "cancelled protected spawn cleanup deadline"
        );
        auth.tick();
        assert!(
            provider
                .poll(Instant::now(), &mut auth.coordinator)
                .is_empty()
        );
        assert!(
            !provider.authorized && provider.assignee.is_none(),
            "a cancelled spawn must never authorize its late child"
        );
    }
    assert!(provider.registration.is_none());
}

#[test]
fn sealed_image_offer_crosses_through_the_normal_provider_owner() {
    let peer = Peer::build();
    let directory = Directory::new("image");
    let reports = directory.0.join("reports");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&reports)
        .unwrap();
    let report = reports.join("progress");
    let mut transport = LockFileTransport::bind_for_supervised_uid(
        directory.0.join("endpoint"),
        rustix::process::geteuid().as_raw(),
        1,
        limits(),
    )
    .unwrap();
    // The supplied test transport carries exactly the policy the
    // normal start path enables; with_service itself is unchanged.
    transport
        .enable_sealed_images(crate::session_lock_object::session_lock_image_policy(
            &limits(),
        ))
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let base = ProcessLaunchSpec::new(&peer.0)
        .arg(&socket)
        .arg(&report)
        .arg("image")
        .process_group()
        .protection_domain(
            ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::LockProvider])
                .unwrap()
                .path(ProtectionPath::read_only(socket.parent().unwrap()))
                .unwrap()
                .path(ProtectionPath::read_write(&reports))
                .unwrap(),
        );
    let mut auth = factotum::Fixture::new();
    let wake = sophia_wake::Wake::new().unwrap();
    let mut provider = LockProvider::with_service(
        transport,
        base,
        ShellGpuMode::Denied,
        None,
        locked(),
        Vec::new(),
        wake.notifier(),
        policy(),
        auth.coordinator.endpoint(),
    )
    .unwrap();
    let expected: Vec<u8> = (0..64u32)
        .map(|i| if i % 4 == 3 { 255 } else { (i * 31 + 7) as u8 })
        .collect();
    let mut held: Option<sophia_runtime::SharedPixels> = None;
    let mut connected = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        assert!(
            Instant::now() < deadline,
            "sealed image must be accepted through the normal provider"
        );
        for event in step(&mut provider, &mut auth) {
            match event {
                LockFileServiceEvent::Connected {
                    connection_epoch, ..
                } => connected.push(connection_epoch),
                LockFileServiceEvent::Inbound {
                    inbound:
                        sophia_runtime::lock_files::LockInbound::ResourceReady {
                            resource,
                            width_px,
                            height_px,
                            pixels,
                        },
                    ..
                } => {
                    assert_eq!(resource.id, 7);
                    assert_eq!((width_px, height_px), (4, 4));
                    assert_eq!(pixels.len(), expected.len());
                    assert_eq!(pixels.as_ref(), expected.as_slice());
                    held = Some(pixels);
                }
                _ => {}
            }
        }
        if connected == [1]
            && held.is_some()
            && std::fs::read_to_string(&report).ok().as_deref() == Some("image-accepted\n")
        {
            break;
        }
    }
    assert!(provider.authorized);
    let held = held.expect("ResourceReady pixels retained by the owner");
    assert_eq!(held.as_ref(), expected.as_slice());

    // Revoke Factotum as in the authority test. The connection's
    // journal dies with no SourceReleased owed, while the retained
    // pixels outlive the epoch that delivered them.
    auth._authenticator
        .as_ref()
        .unwrap()
        .launch_service()
        .current()
        .unwrap()
        .ingress
        .invalidate();
    // Observe revocation without the bootstrap step: the supervisor
    // legitimately terminates the old peer during replacement, so its
    // exit status is not a success assertion here.
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::fs::read_to_string(&report).ok().as_deref() != Some("revoked\n") {
        assert!(
            Instant::now() < deadline,
            "image provider must settle revoked after Factotum loss"
        );
        // No owner poll here, as in the authority test: driving the
        // provider would let the supervisor terminate the peer before
        // it observes the revocation itself.
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut replaced = false;
    while !replaced {
        assert!(
            Instant::now() < deadline,
            "revoked image provider must be replaced"
        );
        auth.tick();
        let _ = provider.poll(Instant::now(), &mut auth.coordinator);
        replaced |= provider.take_replacement();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        !provider.authorized,
        "replacement is observed only after the old provider's authority ended"
    );
    assert_eq!(
        held.as_ref(),
        expected.as_slice(),
        "retained pixels stay readable across Factotum revocation"
    );
    drop(held);
}

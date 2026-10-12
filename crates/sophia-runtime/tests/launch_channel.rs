//! Fresh per-spawn fd custody through the production supervisor and Bubblewrap.
//! This does not yet join the child relay, Factotum or role authentication.

use sophia_runtime::{
    LAUNCH_AUTH_FD_ENV, LaunchChannel, ProcessLaunchSpec, ProcessSupervisor,
    ProcessSupervisorError, ProtectionDomainRole, ProtectionDomainSpec,
};
use sophia_runtime::{SupervisedProcessKind, SupervisorCommand, SupervisorEvent};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

#[path = "support/host_namespace_channel.rs"]
mod host_namespace_channel;

fn launch(program: &std::path::Path) -> ProcessLaunchSpec {
    ProcessLaunchSpec::new(program).protection_domain(
        ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell]).unwrap(),
    )
}
fn eof(mut stream: UnixStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    assert_eq!(
        stream.read(&mut [0]).unwrap(),
        0,
        "failed handoff must close every child-end copy"
    );
}
fn ordinary_start(
    supervisor: &mut ProcessSupervisor,
) -> Result<Option<SupervisorEvent>, ProcessSupervisorError> {
    supervisor.apply(SupervisorCommand::StartProcess {
        process: SupervisedProcessKind::Shell,
        delay: Duration::ZERO,
    })
}

struct Probe(PathBuf);
impl Probe {
    fn build() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "launch-channel-probe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let status = Command::new("/usr/bin/cc")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O2"])
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/launch_channel_probe.c"
            ))
            .arg("-o")
            .arg(&path)
            .status()
            .unwrap();
        assert!(
            status.success(),
            "independent descriptor probe must compile"
        );
        Self(path)
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn protected_handoff_is_single_generation_and_excludes_unrelated_descriptors() {
    let probe = Probe::build();
    let sentinel = std::fs::File::open("/dev/null").unwrap();
    rustix::io::fcntl_setfd(&sentinel, rustix::io::FdFlags::empty()).unwrap();
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, launch(&probe.0));
    for generation in [1, 4] {
        let (mut owner, handoff) = LaunchChannel::reserve(generation).unwrap();
        owner
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_eq!(
            supervisor.start_with_launch_channel(handoff).unwrap(),
            SupervisorEvent::ProcessStarted
        );
        assert_eq!(supervisor.launch_generation(), Some(generation));
        assert!(
            supervisor
                .protection_evidence()
                .unwrap()
                .roles
                .contains(&ProtectionDomainRole::MetadataShell)
        );
        let mut ready = [0; 6];
        assert!(
            owner.read_exact(&mut ready).is_ok(),
            "handoff must expose only the designated channel"
        );
        assert_eq!(
            &ready, b"READY\n",
            "handoff must expose only the designated channel"
        );
        if generation == 1 {
            let (other_owner, other_launch) = LaunchChannel::reserve(2).unwrap();
            assert!(matches!(
                supervisor.start_with_launch_channel(other_launch),
                Err(ProcessSupervisorError::AlreadyRunning { .. })
            ));
            assert_eq!(
                supervisor.launch_generation(),
                Some(1),
                "another reservation must not relabel the live spawn"
            );
            eof(other_owner);
        }
        assert!(
            !rustix::io::fcntl_getfd(&sentinel)
                .unwrap()
                .contains(rustix::io::FdFlags::CLOEXEC),
            "child descriptor filtering must not change the parent's flags"
        );
        owner.write_all(b"Q").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if supervisor.poll().unwrap() == Some(SupervisorEvent::ProcessExited) {
                break;
            }
            assert!(Instant::now() < deadline, "probe reap deadline");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(supervisor.take_exit_status().unwrap().success());
        assert_eq!(supervisor.launch_generation(), None);
        eof(owner);
        assert!(
            matches!(
                ordinary_start(&mut supervisor),
                Err(ProcessSupervisorError::LaunchChannelRequired)
            ),
            "authenticated restart must not silently omit its new channel"
        );
        let (owner, duplicate) = LaunchChannel::reserve(generation).unwrap();
        assert!(matches!(
            supervisor.start_with_launch_channel(duplicate),
            Err(ProcessSupervisorError::StaleLaunchGeneration)
        ));
        eof(owner);
        if generation == 1 {
            supervisor
                .replace_launch_spec(launch(std::path::Path::new(
                    "/nonexistent-sophia-launch-probe",
                )))
                .unwrap();
            let (owner, failed) = LaunchChannel::reserve(3).unwrap();
            assert!(supervisor.start_with_launch_channel(failed).is_err());
            assert!(supervisor.protection_evidence().is_none());
            assert!(supervisor.launch_generation().is_none());
            eof(owner);
            supervisor.replace_launch_spec(launch(&probe.0)).unwrap();
            let (owner, duplicate) = LaunchChannel::reserve(3).unwrap();
            assert!(matches!(
                supervisor.start_with_launch_channel(duplicate),
                Err(ProcessSupervisorError::StaleLaunchGeneration)
            ));
            eof(owner);
        }
    }
}

#[test]
fn an_unprotected_spawn_or_configured_descriptor_claim_cannot_take_the_channel() {
    assert!(LaunchChannel::reserve(0).is_err());
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new("/usr/bin/true"),
    );
    let (owner, channel) = LaunchChannel::reserve(1).unwrap();
    assert!(matches!(
        supervisor.start_with_launch_channel(channel),
        Err(ProcessSupervisorError::ProtectedLaunchRequired)
    ));
    eof(owner);
    supervisor
        .replace_launch_spec(
            launch(std::path::Path::new("/usr/bin/true")).env(LAUNCH_AUTH_FD_ENV, "999"),
        )
        .unwrap();
    let (owner, channel) = LaunchChannel::reserve(2).unwrap();
    assert!(matches!(
        supervisor.start_with_launch_channel(channel),
        Err(ProcessSupervisorError::ProtectionDomainFailed { .. })
    ));
    assert!(supervisor.child_id().is_none());
    eof(owner);
}

fn ready_probe(
    probe: &Probe,
    generation: u64,
) -> (
    ProcessSupervisor,
    UnixStream,
    sophia_runtime::LaunchReservation,
) {
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        launch(&probe.0).process_group(),
    );
    let (mut owner, channel) = LaunchChannel::reserve(generation).unwrap();
    let reservation = channel.reservation();
    supervisor.start_with_launch_channel(channel).unwrap();
    owner
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut ready = [0; 6];
    owner.read_exact(&mut ready).unwrap();
    assert_eq!(&ready, b"READY\n");
    (supervisor, owner, reservation)
}

#[test]
fn worker_custody_requires_the_exact_spawn_and_retires_before_reaping() {
    let probe = Probe::build();
    let (mut supervisor, _owner, reservation) = ready_probe(&probe, 1);
    let (_other_owner, other) = LaunchChannel::reserve(1).unwrap();
    assert!(
        supervisor
            .launch_custody(&other.reservation(), ProtectionDomainRole::MetadataShell)
            .unwrap()
            .is_none(),
        "equal generation numbers cannot manufacture worker custody"
    );
    assert!(
        supervisor
            .launch_custody(&reservation, ProtectionDomainRole::LockProvider)
            .unwrap()
            .is_none(),
        "worker custody must name this spawn's protected role"
    );
    let custody = supervisor
        .launch_custody(&reservation, ProtectionDomainRole::MetadataShell)
        .unwrap()
        .unwrap();
    let observed = custody.clone();
    assert!(
        std::thread::spawn(move || observed.is_live().unwrap())
            .join()
            .unwrap()
    );
    // Keep both processes alive while TERM is pending. Otherwise kernel exit
    // could mask a missing synchronous owner revocation flag.
    for raw in [
        supervisor.child_id().unwrap(),
        supervisor.peer_id().unwrap(),
    ] {
        let pid = rustix::process::Pid::from_raw(raw as i32).unwrap();
        rustix::process::kill_process(pid, rustix::process::Signal::STOP).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            let status = std::fs::read_to_string(format!("/proc/{raw}/status")).unwrap();
            if status.lines().any(|line| line.starts_with("State:\tT")) {
                break;
            }
            assert!(
                Instant::now() < end,
                "custody control must stop both processes"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    assert!(custody.is_live().unwrap(), "stopping is not exiting");
    supervisor.request_termination().unwrap();
    assert!(
        supervisor.child_id().is_some(),
        "termination has not handed away child ownership"
    );
    assert!(
        !custody.is_live().unwrap(),
        "termination must revoke worker custody before reap"
    );
    for raw in [
        supervisor.child_id().unwrap(),
        supervisor.peer_id().unwrap(),
    ] {
        let pid = rustix::process::Pid::from_raw(raw as i32).unwrap();
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::CONT);
    }
    supervisor.terminate().unwrap();
    let (mut next, _owner, next_reservation) = ready_probe(&probe, 1);
    assert!(
        next.launch_custody(&next_reservation, ProtectionDomainRole::MetadataShell)
            .unwrap()
            .unwrap()
            .is_live()
            .unwrap()
    );
    assert!(
        !custody.is_live().unwrap(),
        "another live spawn cannot revive retired worker custody"
    );
}

#[test]
fn worker_custody_observes_exit_without_a_session_poll() {
    let probe = Probe::build();
    let (mut supervisor, mut owner, reservation) = ready_probe(&probe, 1);
    let custody = supervisor
        .launch_custody(&reservation, ProtectionDomainRole::MetadataShell)
        .unwrap()
        .unwrap();
    assert!(custody.is_live().unwrap());
    owner.write_all(b"Q").unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    while custody.is_live().unwrap() {
        assert!(
            Instant::now() < end,
            "worker must detect kernel exit without Session progress"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        supervisor.child_id().is_some(),
        "Session has not polled or reaped the wrapper"
    );
    supervisor.terminate().unwrap();
    assert!(
        supervisor.take_exit_status().unwrap().success(),
        "worker observation must not consume the child's exit status"
    );
}

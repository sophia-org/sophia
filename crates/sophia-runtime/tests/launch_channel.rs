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
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("launch-channel-probe-{}", std::process::id()));
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

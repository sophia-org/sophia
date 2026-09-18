use super::*;

#[test]
fn supervisor_start_request_emits_immediate_start_without_consuming_restart_budget() {
    let state = SupervisorState::new(SupervisedProcessKind::WindowManager);
    let (state, command) = update_supervisor(
        state,
        SupervisorEvent::StartRequested,
        RestartPolicy::default(),
    );
    assert_eq!(state.restart_attempts, 0);
    assert!(!state.running);
    assert_eq!(
        command,
        SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO
        }
    );
}

#[test]
fn supervisor_restart_request_consumes_budget_and_applies_backoff() {
    let policy = RestartPolicy {
        max_attempts: 4,
        initial_backoff: Duration::from_millis(25),
        max_backoff: Duration::from_millis(60),
    };
    let state = SupervisorState::new(SupervisedProcessKind::WindowManager);
    let (state, first) = update_supervisor(state, SupervisorEvent::RestartRequested, policy);
    let (state, second) = update_supervisor(state, SupervisorEvent::RestartRequested, policy);
    let (state, third) = update_supervisor(state, SupervisorEvent::RestartRequested, policy);
    let (state, fourth) = update_supervisor(state, SupervisorEvent::RestartRequested, policy);
    assert_eq!(state.restart_attempts, 4);
    assert_eq!(
        first,
        SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO
        }
    );
    assert_eq!(
        second,
        SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::from_millis(25)
        }
    );
    assert_eq!(
        third,
        SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::from_millis(50)
        }
    );
    assert_eq!(
        fourth,
        SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::from_millis(60)
        }
    );
}

#[test]
fn supervisor_gives_up_after_restart_budget_is_exhausted() {
    let policy = RestartPolicy {
        max_attempts: 1,
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(100),
    };
    let state = SupervisorState::new(SupervisedProcessKind::PortalBroker);
    let (state, first) = update_supervisor(state, SupervisorEvent::ProcessExited, policy);
    let (_state, second) = update_supervisor(state, SupervisorEvent::ProcessExited, policy);
    assert_eq!(
        first,
        SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::PortalBroker,
            delay: Duration::ZERO
        }
    );
    assert_eq!(
        second,
        SupervisorCommand::GiveUp {
            process: SupervisedProcessKind::PortalBroker
        }
    );
}

#[test]
fn supervisor_healthy_event_resets_restart_budget() {
    let policy = RestartPolicy {
        max_attempts: 2,
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(100),
    };
    let state = SupervisorState::new(SupervisedProcessKind::MetadataBroker);
    let (state, _) = update_supervisor(state, SupervisorEvent::RestartRequested, policy);
    let (state, command) = update_supervisor(state, SupervisorEvent::ProcessHealthy, policy);
    assert!(state.running);
    assert_eq!(state.restart_attempts, 0);
    assert_eq!(command, SupervisorCommand::None);
}

#[test]
fn process_supervisor_spawns_and_observes_process_exit() {
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new("/usr/bin/true"),
    );
    let event = supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO,
        })
        .unwrap();
    assert_eq!(event, Some(SupervisorEvent::ProcessStarted));
    assert!(supervisor.child_id().is_some());
    let mut observed = None;
    for _ in 0..100 {
        observed = supervisor.poll().unwrap();
        if observed.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(observed, Some(SupervisorEvent::ProcessExited));
    assert_eq!(supervisor.child_id(), None);
}

#[test]
fn runtime_broker_supervisors_start_and_observe_placeholder_exits() {
    let mut supervisors = RuntimeBrokerSupervisors::new(
        ProcessLaunchSpec::new("/usr/bin/true"),
        ProcessLaunchSpec::new("/usr/bin/true"),
    );
    let report = supervisors.start_placeholders().unwrap();
    assert_eq!(report.portal_start, Some(SupervisorEvent::ProcessStarted));
    assert_eq!(report.metadata_start, Some(SupervisorEvent::ProcessStarted));
    let mut portal_exit = report.portal_poll;
    let mut metadata_exit = report.metadata_poll;
    for _ in 0..100 {
        if portal_exit == Some(SupervisorEvent::ProcessExited)
            && metadata_exit == Some(SupervisorEvent::ProcessExited)
        {
            break;
        }
        let (portal, metadata) = supervisors.poll_all().unwrap();
        portal_exit = portal_exit.or(portal);
        metadata_exit = metadata_exit.or(metadata);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(portal_exit, Some(SupervisorEvent::ProcessExited));
    assert_eq!(metadata_exit, Some(SupervisorEvent::ProcessExited));
    assert_eq!(supervisors.portal.child_id(), None);
    assert_eq!(supervisors.metadata.child_id(), None);
}

#[test]
fn runtime_authority_supervisor_reports_reduced_x_authority_health() {
    let mut supervisor =
        RuntimeAuthoritySupervisor::new_x_authority(ProcessLaunchSpec::new("/usr/bin/true"));
    let report = supervisor
        .start()
        .expect("placeholder authority should start");
    assert_eq!(report.start, Some(SupervisorEvent::ProcessStarted));
    assert_eq!(
        report.observations[0],
        SessionRuntimeObservation::AuthorityProcessHealthChanged {
            process: SupervisedProcessKind::SophiaXAuthority,
            state: BrokerHealthState::Ready,
            generation: 1,
            status_message_len: 0,
        }
    );
    let mut runtime = SessionRuntimeLoop::default();
    let mut observations = report.observations;
    let mut exit = report.poll;
    for _ in 0..100 {
        if exit == Some(SupervisorEvent::ProcessExited) {
            break;
        }
        let (event, next_observations) = supervisor.poll().expect("poll should succeed");
        exit = exit.or(event);
        observations.extend(next_observations);
        std::thread::sleep(Duration::from_millis(1));
    }
    runtime
        .step_observations(observations.clone())
        .expect("authority health should be accepted");
    assert_eq!(exit, Some(SupervisorEvent::ProcessExited));
    assert_eq!(observations.len(), 2);
    assert_eq!(
        observations[1],
        SessionRuntimeObservation::AuthorityProcessHealthChanged {
            process: SupervisedProcessKind::SophiaXAuthority,
            state: BrokerHealthState::Stopped,
            generation: 2,
            status_message_len: 0,
        }
    );
    assert_eq!(
        runtime.state().x_authority_health,
        Some(RuntimeAuthorityHealth {
            process: SupervisedProcessKind::SophiaXAuthority,
            state: BrokerHealthState::Stopped,
            generation: 2,
            status_message_len: 0,
        })
    );
}

#[test]
fn process_supervisor_rejects_wrong_process_command() {
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new("/usr/bin/true"),
    );
    let error = supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::PortalBroker,
            delay: Duration::ZERO,
        })
        .unwrap_err();
    assert_eq!(
        error,
        ProcessSupervisorError::WrongProcess {
            expected: SupervisedProcessKind::WindowManager,
            actual: SupervisedProcessKind::PortalBroker
        }
    );
}

#[test]
fn process_supervisor_rejects_start_while_child_is_running() {
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new("/usr/bin/sleep").arg("1"),
    );
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO,
        })
        .unwrap();
    let error = supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO,
        })
        .unwrap_err();
    assert_eq!(
        error,
        ProcessSupervisorError::AlreadyRunning {
            process: SupervisedProcessKind::WindowManager
        }
    );
    supervisor.terminate().unwrap();
    assert_eq!(supervisor.child_id(), None);
}

#[test]
fn process_supervisor_replaces_only_an_inactive_launch_spec() {
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new("/usr/bin/true"),
    );
    supervisor
        .replace_launch_spec(ProcessLaunchSpec::new("/usr/bin/sleep").arg("1"))
        .unwrap();
    assert_eq!(supervisor.launch_spec().program, "/usr/bin/sleep");

    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO,
        })
        .unwrap();
    assert!(matches!(
        supervisor.replace_launch_spec(ProcessLaunchSpec::new("/usr/bin/true")),
        Err(ProcessSupervisorError::AlreadyRunning {
            process: SupervisedProcessKind::WindowManager
        })
    ));
    supervisor.terminate().unwrap();
}

#[test]
fn bubblewrap_supervisor_reports_the_actual_role_peer() {
    if std::env::var_os("SOPHIA_RUN_PROTECTION_DOMAIN_SMOKE").is_none() {
        return;
    }
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::SpatialPolicy]).unwrap();
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new("/usr/bin/sleep")
            .arg("5")
            .protection_domain(domain),
    );
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO,
        })
        .unwrap();
    let launcher = supervisor.child_id().unwrap();
    let peer = supervisor.peer_id().unwrap();
    assert_ne!(launcher, peer);
    assert_eq!(supervisor.protection_evidence().unwrap().peer_pid, peer);
    assert!(std::path::Path::new(&format!("/proc/{peer}/ns/pid")).exists());
    supervisor.terminate().unwrap();
}

#[test]
fn bubblewrap_mounts_owned_filesystems_read_only_without_host_tree_disclosure() {
    if std::env::var_os("SOPHIA_RUN_PROTECTION_DOMAIN_SMOKE").is_none() {
        return;
    }
    let evidence =
        std::env::temp_dir().join(format!("sophia-protection-evidence-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&evidence);
    std::fs::create_dir(&evidence).unwrap();
    let mut filesystem = ProtectionFilesystemManifest::new();
    filesystem.directory("devices").unwrap();
    filesystem.directory("devices/gpu").unwrap();
    filesystem
        .file("devices/gpu/vendor", b"0x1002\n".to_vec())
        .unwrap();
    filesystem.symlink("selected", "devices/gpu").unwrap();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::SpatialPolicy])
        .unwrap()
        .read_only_filesystem("/sys", filesystem)
        .unwrap()
        .path(ProtectionPath::read_write_at(&evidence, "/evidence"))
        .unwrap();
    let script = "set -eu; test \"$(cat /sys/selected/vendor)\" = 0x1002; \
                  test ! -e /sys/kernel; if touch /sys/new 2>/dev/null; then exit 1; fi; \
                  printf pass > /evidence/result";
    let mut supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::WindowManager,
        ProcessLaunchSpec::new("/usr/bin/sh")
            .arg("-c")
            .arg(script)
            .protection_domain(domain),
    );
    supervisor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::WindowManager,
            delay: Duration::ZERO,
        })
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while supervisor.poll().unwrap().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        std::fs::read_to_string(evidence.join("result")).unwrap(),
        "pass"
    );
    std::fs::remove_dir_all(evidence).unwrap();
}

#[test]
fn nonblocking_termination_retains_child_while_neighbor_progresses() {
    let ready = std::env::temp_dir().join(format!("supervisor-stop-ready-{}", std::process::id()));
    let _ = std::fs::remove_file(&ready);
    let mut owner = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new("/bin/sh")
            .arg("-c")
            .arg("trap '' TERM; echo ready > \"$1\"; while :; do sleep 1; done")
            .arg("fixture")
            .arg(&ready)
            .process_group(),
    );
    owner
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::Shell,
            delay: Duration::ZERO,
        })
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while !ready.exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let pid = owner.child_id();
    owner.request_termination().unwrap();
    owner.request_termination().unwrap();
    assert_eq!(owner.child_id(), pid);
    assert!(!owner.poll_termination().unwrap());
    assert!(matches!(
        owner.replace_launch_spec(ProcessLaunchSpec::new("/bin/true")),
        Err(ProcessSupervisorError::AlreadyRunning { .. })
    ));
    let mut neighbor = ProcessSupervisor::new(
        SupervisedProcessKind::Shell,
        ProcessLaunchSpec::new("/bin/true"),
    );
    neighbor
        .apply(SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::Shell,
            delay: Duration::ZERO,
        })
        .unwrap();
    while neighbor.poll().unwrap().is_none() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(owner.child_id(), pid);
    while !owner.poll_termination().unwrap() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(owner.child_id().is_none());
    assert!(owner.poll_termination().unwrap());
    owner
        .replace_launch_spec(ProcessLaunchSpec::new("/bin/true"))
        .unwrap();
    std::fs::remove_file(ready).unwrap();
}

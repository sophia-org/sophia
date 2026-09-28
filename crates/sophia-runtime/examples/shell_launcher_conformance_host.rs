use sophia_protocol::*;
use sophia_runtime::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn wait_for<T>(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
    mut poll: impl FnMut(
        &mut ShellComponentTransport,
        &mut ContentEpochRegistry,
    ) -> Result<Option<T>, ShellTransportError>,
) -> Result<T, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(value) = poll(transport, epochs)? {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err("launcher conformance timeout".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn candidate(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
) -> Result<(TransactionId, ShellLauncherCandidate), Box<dyn std::error::Error>> {
    match wait_for(transport, epochs, |transport, epochs| {
        transport.poll_launcher_candidate(epochs)
    })? {
        ShellLauncherCandidateEvent::Candidate(tx, candidate) => Ok((tx, candidate)),
        ShellLauncherCandidateEvent::Refused(_) => Err("launcher candidate refused".into()),
    }
}

fn finish_client(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
    supervisor: &mut ProcessSupervisor,
) -> Result<(), Box<dyn std::error::Error>> {
    // Signal the authenticated client, not bwrap's leader: killing the leader
    // invokes --die-with-parent before the client can handle SIGTERM.
    let peer_pid =
        rustix::process::Pid::from_raw(supervisor.peer_id().ok_or("missing admitted peer")? as i32)
            .ok_or("invalid peer pid")?;
    rustix::process::kill_process(peer_pid, rustix::process::Signal::TERM)?;
    let leader = rustix::process::Pid::from_raw(
        supervisor.child_id().ok_or("missing supervisor child")? as i32,
    )
    .ok_or("invalid supervisor child")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = rustix::process::waitid(
            rustix::process::WaitId::Pid(leader),
            rustix::process::WaitIdOptions::EXITED
                | rustix::process::WaitIdOptions::NOHANG
                | rustix::process::WaitIdOptions::NOWAIT,
        )? {
            if status.exit_status() != Some(0) {
                return Err(format!("launcher client failed: {status:?}").into());
            }
            supervisor.poll()?;
            return Ok(());
        }
        match transport.poll_io(epochs) {
            Ok(()) | Err(ShellTransportError::NotConnected) => {}
            Err(error) => return Err(error.into()),
        }
        if Instant::now() >= deadline {
            return Err("launcher client did not exit cleanly".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or("usage: shell_launcher_conformance_host CLIENT")?;
    if !client.is_absolute() || !client.is_file() {
        return Err("launcher client must be an absolute executable path".into());
    }
    let directory = std::env::temp_dir().join(format!(
        "sophia-launcher-conformance-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )?;
    let mut epochs =
        ContentEpochRegistry::new(64 * 1024 * 1024).map_err(|error| format!("{error:?}"))?;
    let socket = transport.socket_path().to_path_buf();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?.path(
        ProtectionPath::read_only(socket.parent().ok_or("socket parent")?),
    )?;
    let spec = ProcessLaunchSpec::new(client)
        .arg("--serve")
        .env("SOPHIA_SHELL_9P_SOCKET", &socket)
        .process_group()
        .protection_domain(domain);
    let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::Shell, spec);
    supervisor.apply(SupervisorCommand::StartProcess {
        process: SupervisedProcessKind::Shell,
        delay: Duration::ZERO,
    })?;
    transport.authorize_protected_peer(
        supervisor
            .protection_evidence()
            .ok_or("missing protection evidence")?,
    )?;
    transport.begin_descriptor_file_negotiation(
        &epochs,
        5,
        Duration::from_secs(5),
        ShellContentAdmissionPolicy::Unavailable,
    )?;
    wait_for(&mut transport, &mut epochs, |transport, epochs| {
        transport.poll_negotiation(epochs, 65536)
    })?;
    assert!(transport.supports_launcher());
    let tx = TransactionId::from_raw(1);
    let catalog = ShellApplicationCatalog {
        connection_epoch: 5,
        generation: 7,
        entries: (1..=4096)
            .map(|slot| ShellApplicationDescriptor {
                slot,
                available: slot != 2,
                label: format!("Application {slot}"),
                keywords: "editor terminal".into(),
            })
            .collect(),
    };
    transport.publish_launcher_catalog(&epochs, tx, &catalog)?;
    let mut request = ShellLauncherRequest {
        connection_epoch: 5,
        catalog_generation: 7,
        request_generation: 8,
        output: OutputId::from_raw(1),
        output_generation: 1,
        presentation_epoch: 0,
        operation: ShellLauncherOperation::Open,
        query: String::new(),
    };
    transport.begin_launcher_request(&mut epochs, tx, &request)?;
    let (reply, first) = candidate(&mut transport, &mut epochs)?;
    assert_eq!(reply, tx);
    assert_eq!(first.request_generation, 8);
    assert!(first.visible);
    assert!(!first.entries.is_empty());
    sophia_engine::launcher_projection(
        &first,
        &catalog,
        "",
        1,
        Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        |text, size| (text.len() as i32 * 8, i32::from(size)),
    )?;
    let activation_tx = TransactionId::from_raw(12);
    let mut activation = ShellLauncherActivation {
        connection_epoch: 5,
        catalog_generation: 7,
        request_generation: 8,
        candidate_generation: first.candidate_generation,
        presentation_epoch: 11,
        activation: 12,
        slot: first.selected,
    };
    // The file owner refuses unpresented grants before publication. A malicious
    // server's pre-presentation grant is covered by the client's policy tests.
    assert!(matches!(
        transport.queue_launcher_activation(&mut epochs, activation_tx, activation),
        Err(ShellTransportError::WrongActivation)
    ));
    for kind in [
        ShellV1CandidateOutcomeKind::Prepared,
        ShellV1CandidateOutcomeKind::Presented,
    ] {
        transport.send_launcher_outcome(
            &mut epochs,
            tx,
            ShellLauncherOutcome {
                connection_epoch: 5,
                request_generation: 8,
                candidate_generation: first.candidate_generation,
                presentation_epoch: if kind == ShellV1CandidateOutcomeKind::Presented {
                    11
                } else {
                    0
                },
                kind,
            },
        )?;
    }
    activation.activation = 13;
    transport.queue_launcher_activation(&mut epochs, activation_tx, activation)?;
    let (ack_tx, ack) = wait_for(&mut transport, &mut epochs, |transport, epochs| {
        transport.poll_launcher_activation_ack(epochs)
    })?;
    assert_eq!(ack_tx, activation_tx);
    assert_eq!(ack.activation, activation);
    assert!(ack.consumed);
    transport.send_launch_outcome(
        &mut epochs,
        activation_tx,
        ShellLaunchOutcome {
            activation,
            status: ShellLaunchStatus::Started,
        },
    )?;

    transport.queue_launcher_activation(&mut epochs, activation_tx, activation)?;
    let (_, ack) = wait_for(&mut transport, &mut epochs, |transport, epochs| {
        transport.poll_launcher_activation_ack(epochs)
    })?;
    assert_eq!(ack.activation, activation);
    assert!(!ack.consumed);
    transport.send_launch_outcome(
        &mut epochs,
        activation_tx,
        ShellLaunchOutcome {
            activation,
            status: ShellLaunchStatus::Rejected,
        },
    )?;

    request.request_generation = 14;
    request.operation = ShellLauncherOperation::Query;
    request.query = "editor".into();
    request.presentation_epoch = 11;
    transport.begin_launcher_request(&mut epochs, tx, &request)?;
    let (_, next) = candidate(&mut transport, &mut epochs)?;
    assert_eq!(next.request_generation, 14);
    assert!(next.candidate_generation > first.candidate_generation);
    activation.activation = 15;
    assert!(matches!(
        transport.queue_launcher_activation(&mut epochs, activation_tx, activation),
        Err(ShellTransportError::WrongActivation)
    ));
    // The query reply follows processing the earlier launch outcome. Stop only
    // after that witness; no final unobserved control is labelled delivered.
    finish_client(&mut transport, &mut epochs, &mut supervisor)?;
    transport.disconnect(&mut epochs)?;
    println!(
        "sophia_launcher_conformance status=passed wire=9p catalog=4096 unpresented=denied_by_host replay=denied_by_client pending_query=denied_by_host protected=true clean_exit=true"
    );
    Ok(())
}

use sophia_protocol::*;
use sophia_runtime::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[allow(dead_code)]
#[path = "../../sophia-protocol/tests/support/launcher_fixture.rs"]
mod fixture;
fn receive(
    transport: &mut ShellSessionTransport,
    kind: IpcMessageKind,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(frame) = transport.poll_kind(kind)? {
            return Ok(frame);
        }
        if Instant::now() > deadline {
            return Err("launcher conformance timeout".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn send(
    transport: &mut ShellSessionTransport,
    frame: Result<Vec<u8>, IpcCodecError>,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = frame.map_err(|e| format!("{e:?}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        // Refusal precedes FIFO ownership. Retry this exact frame only; once
        // accepted, a later I/O error must not replay it.
        match transport.enqueue_async(frame.clone()) {
            Ok(()) => {
                transport.poll_io()?;
                return Ok(());
            }
            Err(ShellTransportError::ActivationQueueSaturated) if Instant::now() < deadline => {
                transport.poll_io()?;
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error.into()),
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or("usage: shell_launcher_conformance_host CLIENT")?;
    let directory = std::env::temp_dir().join(format!(
        "sophia-launcher-conformance-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let mut transport = ShellSessionTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )?;
    let socket = transport.socket_path().to_path_buf();
    let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataShell])?
        .path(ProtectionPath::read_only(socket.parent().unwrap()))?;
    let spec = ProcessLaunchSpec::new(client)
        .arg("--serve")
        .env(SOPHIA_SHELL_SOCKET_ENV, &socket)
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
    transport.accept_and_negotiate(5, Duration::from_secs(5))?;
    assert!(transport.supports_launcher());
    let tx = TransactionId::from_raw(1);
    let catalog = fixture::catalog(4096);
    for frame in encode_shell_application_catalog(tx, &catalog).map_err(|e| format!("{e:?}"))? {
        send(&mut transport, Ok(frame))?;
    }
    let request = ShellLauncherRequest {
        connection_epoch: 5,
        catalog_generation: 7,
        request_generation: 8,
        output: OutputId::from_raw(1),
        output_generation: 1,
        presentation_epoch: 0,
        operation: ShellLauncherOperation::Open,
        query: String::new(),
    };
    send(&mut transport, encode_shell_launcher_request(tx, &request))?;
    let (reply, candidate) = decode_shell_launcher_candidate(&receive(
        &mut transport,
        IpcMessageKind::ShellLauncherCandidate,
    )?)
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(reply, tx);
    assert_eq!(candidate.request_generation, 8);
    assert!(candidate.visible);
    assert!(!candidate.entries.is_empty());
    sophia_engine::launcher_projection(
        &candidate,
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
    let mut activation = ShellLauncherActivation {
        connection_epoch: 5,
        catalog_generation: 7,
        request_generation: 8,
        candidate_generation: candidate.candidate_generation,
        presentation_epoch: 11,
        activation: 12,
        slot: candidate.selected,
    };
    let activation_tx = TransactionId::from_raw(12);
    send(
        &mut transport,
        encode_shell_launcher_activation(activation_tx, activation),
    )?;
    let (_, ack) = decode_shell_launcher_activation_ack(&receive(
        &mut transport,
        IpcMessageKind::ShellLauncherActivationAck,
    )?)
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(ack.activation, activation);
    assert!(!ack.consumed);
    send(
        &mut transport,
        encode_shell_launch_outcome(
            activation_tx,
            ShellLaunchOutcome {
                activation,
                status: ShellLaunchStatus::Rejected,
            },
        ),
    )?;
    for kind in [
        ShellV1CandidateOutcomeKind::Prepared,
        ShellV1CandidateOutcomeKind::Presented,
    ] {
        send(
            &mut transport,
            encode_shell_launcher_outcome(
                tx,
                ShellLauncherOutcome {
                    connection_epoch: 5,
                    request_generation: 8,
                    candidate_generation: candidate.candidate_generation,
                    presentation_epoch: if kind == ShellV1CandidateOutcomeKind::Presented {
                        11
                    } else {
                        0
                    },
                    kind,
                },
            ),
        )?;
    }
    activation.activation = 13;
    send(
        &mut transport,
        encode_shell_launcher_activation(activation_tx, activation),
    )?;
    let (_, ack) = decode_shell_launcher_activation_ack(&receive(
        &mut transport,
        IpcMessageKind::ShellLauncherActivationAck,
    )?)
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(ack.activation, activation);
    assert!(ack.consumed);
    send(
        &mut transport,
        encode_shell_launch_outcome(
            activation_tx,
            ShellLaunchOutcome {
                activation,
                status: ShellLaunchStatus::Started,
            },
        ),
    )?;
    send(
        &mut transport,
        encode_shell_launcher_activation(activation_tx, activation),
    )?;
    let (_, ack) = decode_shell_launcher_activation_ack(&receive(
        &mut transport,
        IpcMessageKind::ShellLauncherActivationAck,
    )?)
    .map_err(|e| format!("{e:?}"))?;
    assert!(!ack.consumed);
    send(
        &mut transport,
        encode_shell_launch_outcome(
            activation_tx,
            ShellLaunchOutcome {
                activation,
                status: ShellLaunchStatus::Rejected,
            },
        ),
    )?;
    let mut request = request;
    request.request_generation = 14;
    request.operation = ShellLauncherOperation::Query;
    request.query = "editor".into();
    request.presentation_epoch = 11;
    send(&mut transport, encode_shell_launcher_request(tx, &request))?;
    let (_, next) = decode_shell_launcher_candidate(&receive(
        &mut transport,
        IpcMessageKind::ShellLauncherCandidate,
    )?)
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.request_generation, 14);
    assert!(next.candidate_generation > candidate.candidate_generation);
    activation.activation = 15;
    send(
        &mut transport,
        encode_shell_launcher_activation(activation_tx, activation),
    )?;
    let (_, ack) = decode_shell_launcher_activation_ack(&receive(
        &mut transport,
        IpcMessageKind::ShellLauncherActivationAck,
    )?)
    .map_err(|e| format!("{e:?}"))?;
    assert!(!ack.consumed);
    supervisor.terminate()?;
    transport.disconnect()?;
    println!(
        "sophia_launcher_conformance status=passed catalog=4096 unpresented=denied replay=denied pending_query=denied protected=true"
    );
    Ok(())
}

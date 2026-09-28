#![cfg(target_os = "linux")]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::*;
use sophia_runtime::*;

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

#[test]
fn optional_output_service_exchanges_candidate_and_settlement() {
    let directory = temporary_directory("exchange");
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let transport = OutputSessionTransport::bind(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_owned();
    let snapshot = snapshot();
    let service =
        OutputTransportService::spawn(transport, 1, TransactionId::from_raw(1), snapshot.clone())
            .unwrap();
    let expected = proposal(1);
    let sent = expected.clone();
    let client = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(socket_path).unwrap();
        stream
            .write_all(
                &encode_output_v1_client_hello_frame(OutputV1ClientHello {
                    minimum_revision: 1,
                    maximum_revision: 1,
                    capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE
                        | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
                })
                .unwrap(),
            )
            .unwrap();
        let welcome = decode_output_v1_server_welcome_frame(&read_frame(&mut stream)).unwrap();
        assert_eq!(welcome.connection_epoch, 1);
        let (_, observed) = decode_output_v1_snapshot_frame(&read_frame(&mut stream)).unwrap();
        assert_eq!(observed.snapshot, snapshot);
        stream
            .write_all(&encode_output_v1_proposal_frame(TransactionId::from_raw(2), &sent).unwrap())
            .unwrap();
        decode_output_v1_outcome_frame(&read_frame(&mut stream)).unwrap()
    });

    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::Connected {
            connection_epoch: 1
        }
    );
    let OutputTransportServiceEvent::Proposal {
        proposal: admitted,
        admission,
    } = service.event_timeout(Duration::from_secs(1)).unwrap()
    else {
        panic!("output service omitted admitted proposal");
    };
    assert_eq!(admitted.transaction, TransactionId::from_raw(2));
    assert_eq!(admitted.message, expected);
    assert_eq!(admission, OutputProposalAdmission::Active);
    service
        .command(OutputTransportServiceCommand::Settle {
            transaction: admitted.transaction,
            outcome: OutputV1Outcome {
                connection_epoch: 1,
                topology_epoch: 1,
                kind: OutputV1OutcomeKind::Validated,
                reason: 0,
            },
        })
        .unwrap();
    let (transaction, outcome) = client.join().unwrap();
    assert_eq!(transaction, TransactionId::from_raw(2));
    assert_eq!(outcome.kind, OutputV1OutcomeKind::Validated);
}

#[test]
fn connected_output_service_publishes_a_replacement_hardware_snapshot() {
    let directory = temporary_directory("publish-snapshot");
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let transport = OutputSessionTransport::bind(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_owned();
    let initial = snapshot();
    let service =
        OutputTransportService::spawn(transport, 1, TransactionId::from_raw(1), initial.clone())
            .unwrap();
    let mut client = connect_output_client(&socket_path);
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    decode_output_v1_server_welcome_frame(&read_frame(&mut client)).unwrap();
    let (_, observed) = decode_output_v1_snapshot_frame(&read_frame(&mut client)).unwrap();
    assert_eq!(observed.snapshot, initial);
    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::Connected {
            connection_epoch: 1
        }
    );

    let mut replacement = snapshot();
    replacement.topology_epoch = 2;
    replacement.groups[0].logical.width = 1_280;
    service
        .command(OutputTransportServiceCommand::PublishSnapshot {
            transaction: TransactionId::from_raw(2),
            snapshot: replacement.clone(),
        })
        .unwrap();
    let (transaction, observed) =
        decode_output_v1_snapshot_frame(&read_frame(&mut client)).unwrap();
    assert_eq!(transaction, TransactionId::from_raw(2));
    assert_eq!(observed.connection_epoch, 1);
    assert_eq!(observed.snapshot, replacement);
}

/// A client that has left takes its connection with it, not the service.
///
/// A client may close as soon as it has what it asked for, and an unsolicited
/// snapshot is the frame that finds it gone. Ending the service there ends its
/// listening socket too, so the next client -- a restarted policy -- finds
/// nothing to connect to and the supervisor gives up. The read path already
/// treated a departed peer this way; the write path did not.
#[test]
fn publishing_to_a_departed_client_retires_the_connection_and_keeps_serving() {
    let directory = temporary_directory("publish-after-close");
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let transport = OutputSessionTransport::bind(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_owned();
    let initial = snapshot();
    let service =
        OutputTransportService::spawn(transport, 1, TransactionId::from_raw(1), initial.clone())
            .unwrap();
    let mut client = connect_output_client(&socket_path);
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    decode_output_v1_server_welcome_frame(&read_frame(&mut client)).unwrap();
    decode_output_v1_snapshot_frame(&read_frame(&mut client)).unwrap();
    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::Connected {
            connection_epoch: 1
        }
    );

    // The order a live session produces: the snapshot goes out first and the
    // client leaves without reading it, so the close discards a queued frame
    // and the poll that follows is what meets the departure. Publishing after
    // the close would instead break the pipe on the write, which is a different
    // path with the same meaning.
    let mut replacement = snapshot();
    replacement.topology_epoch = 2;
    service
        .command(OutputTransportServiceCommand::PublishSnapshot {
            transaction: TransactionId::from_raw(3),
            snapshot: replacement.clone(),
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    drop(client);

    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::Disconnected {
            connection_epoch: 1
        }
    );

    // The next client is answered from the newest published snapshot.
    let mut successor = connect_output_client(&socket_path);
    successor
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    decode_output_v1_server_welcome_frame(&read_frame(&mut successor)).unwrap();
    let (_, observed) = decode_output_v1_snapshot_frame(&read_frame(&mut successor)).unwrap();
    assert_eq!(observed.snapshot, replacement);
}

/// Settling to a client that already left retires the connection too.
///
/// This is the site a live session met: the policy asked its question, and
/// between the question and the answer its process was restarted. Every write
/// shares one retirement rule for that reason -- which frame happens to meet
/// the close is timing, not meaning.
#[test]
fn settling_to_a_departed_client_retires_the_connection_and_keeps_serving() {
    let directory = temporary_directory("settle-after-close");
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let transport = OutputSessionTransport::bind(&directory, peer).unwrap();
    let socket_path = transport.socket_path().to_owned();
    let initial = snapshot();
    let service =
        OutputTransportService::spawn(transport, 1, TransactionId::from_raw(1), initial.clone())
            .unwrap();
    let mut client = connect_output_client(&socket_path);
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    decode_output_v1_server_welcome_frame(&read_frame(&mut client)).unwrap();
    decode_output_v1_snapshot_frame(&read_frame(&mut client)).unwrap();
    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::Connected {
            connection_epoch: 1
        }
    );

    drop(client);
    for transaction in [5, 6] {
        service
            .command(OutputTransportServiceCommand::Settle {
                transaction: TransactionId::from_raw(transaction),
                outcome: OutputV1Outcome {
                    connection_epoch: 1,
                    topology_epoch: initial.topology_epoch,
                    kind: OutputV1OutcomeKind::Committed,
                    reason: 0,
                },
            })
            .unwrap();
    }

    assert_eq!(
        service.event_timeout(Duration::from_secs(2)).unwrap(),
        OutputTransportServiceEvent::Disconnected {
            connection_epoch: 1
        }
    );

    let mut successor = connect_output_client(&socket_path);
    successor
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    decode_output_v1_server_welcome_frame(&read_frame(&mut successor)).unwrap();
    decode_output_v1_snapshot_frame(&read_frame(&mut successor)).unwrap();
}

#[test]
fn optional_output_service_stops_without_a_client() {
    let directory = temporary_directory("idle");
    let peer = PolicyPeerIdentity {
        uid: rustix::process::geteuid().as_raw(),
        pid: std::process::id(),
    };
    let transport = OutputSessionTransport::bind(&directory, peer).unwrap();
    let started = Instant::now();
    let service =
        OutputTransportService::spawn(transport, 1, TransactionId::from_raw(1), snapshot())
            .unwrap();
    drop(service);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn output_service_reauthorizes_a_restarted_supervised_process() {
    let directory = temporary_directory("restart");
    let uid = rustix::process::geteuid().as_raw();
    let pid = std::process::id();
    let mut transport = OutputSessionTransport::bind_for_supervised_uid(&directory, uid).unwrap();
    transport.authorize_supervised_pid(pid).unwrap();
    let socket_path = transport.socket_path().to_owned();
    let service =
        OutputTransportService::spawn(transport, 1, TransactionId::from_raw(1), snapshot())
            .unwrap();
    let mut first = connect_output_client(&socket_path);
    assert_eq!(
        decode_output_v1_server_welcome_frame(&read_frame(&mut first))
            .unwrap()
            .connection_epoch,
        1
    );
    decode_output_v1_snapshot_frame(&read_frame(&mut first)).unwrap();
    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::Connected {
            connection_epoch: 1
        }
    );

    service
        .command(OutputTransportServiceCommand::ReplaceSupervisedPid { pid })
        .unwrap();
    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::AssigneeReplaced {
            connection_epoch: 2,
            abandoned: Vec::new(),
        }
    );
    first
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut closed = [0u8; 1];
    assert_eq!(first.read(&mut closed).unwrap(), 0);

    let mut second = connect_output_client(&socket_path);
    assert_eq!(
        decode_output_v1_server_welcome_frame(&read_frame(&mut second))
            .unwrap()
            .connection_epoch,
        2
    );
    decode_output_v1_snapshot_frame(&read_frame(&mut second)).unwrap();
    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::Connected {
            connection_epoch: 2
        }
    );
}

#[test]
fn output_service_pauses_acceptance_across_the_spawn_to_pid_handoff() {
    let directory = temporary_directory("restart-barrier");
    let uid = rustix::process::geteuid().as_raw();
    let pid = std::process::id();
    let mut transport = OutputSessionTransport::bind_for_supervised_uid(&directory, uid).unwrap();
    transport.authorize_supervised_pid(pid).unwrap();
    let socket_path = transport.socket_path().to_owned();
    let service =
        OutputTransportService::spawn(transport, 1, TransactionId::from_raw(1), snapshot())
            .unwrap();

    assert!(
        service
            .pause_acceptance(Duration::from_secs(1))
            .unwrap()
            .is_empty()
    );
    let mut replacement = connect_output_client(&socket_path);
    replacement
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    let error = replacement
        .read_exact(&mut header)
        .expect_err("paused service must not negotiate against the old PID");
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ));

    service
        .command(OutputTransportServiceCommand::ReplaceSupervisedPid { pid })
        .unwrap();
    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::AssigneeReplaced {
            connection_epoch: 2,
            abandoned: Vec::new(),
        }
    );
    replacement
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    assert_eq!(
        decode_output_v1_server_welcome_frame(&read_frame(&mut replacement))
            .unwrap()
            .connection_epoch,
        2
    );
    decode_output_v1_snapshot_frame(&read_frame(&mut replacement)).unwrap();
    assert_eq!(
        service.event_timeout(Duration::from_secs(1)).unwrap(),
        OutputTransportServiceEvent::Connected {
            connection_epoch: 2
        }
    );
}

/// Malformed input from the admitted client is that client's fault. It ends
/// that connection, not the optional service and its socket: the same
/// supervised process may connect again at the next epoch and be served.
#[test]
fn a_malformed_client_frame_disconnects_only_that_client() {
    let valid = encode_output_v1_proposal_frame(TransactionId::from_raw(2), &proposal(1)).unwrap();
    let mut zero_epoch = valid.clone();
    // The proposal payload starts with its connection epoch; zero is invalid.
    zero_epoch[SOPHIA_IPC_HEADER_LEN..SOPHIA_IPC_HEADER_LEN + 8].fill(0);
    let mut oversized = valid[..SOPHIA_IPC_HEADER_LEN].to_vec();
    oversized[16..20].copy_from_slice(
        &u32::try_from(SOPHIA_IPC_MAX_PAYLOAD_LEN + 1)
            .unwrap()
            .to_le_bytes(),
    );
    let wrong_kind = encode_output_v1_client_hello_frame(OutputV1ClientHello {
        minimum_revision: 1,
        maximum_revision: 1,
        capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
    })
    .unwrap();
    for (label, malformed) in [
        ("zero-epoch", zero_epoch),
        ("oversized", oversized),
        ("wrong-kind", wrong_kind),
    ] {
        let directory = temporary_directory(&format!("malformed-{label}"));
        let peer = PolicyPeerIdentity {
            uid: rustix::process::geteuid().as_raw(),
            pid: std::process::id(),
        };
        let transport = OutputSessionTransport::bind(&directory, peer).unwrap();
        let socket_path = transport.socket_path().to_owned();
        let service =
            OutputTransportService::spawn(transport, 1, TransactionId::from_raw(1), snapshot())
                .unwrap();

        let mut first = connect_output_client(&socket_path);
        first
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        decode_output_v1_server_welcome_frame(&read_frame(&mut first)).unwrap();
        decode_output_v1_snapshot_frame(&read_frame(&mut first)).unwrap();
        assert_eq!(
            service.event_timeout(Duration::from_secs(2)).unwrap(),
            OutputTransportServiceEvent::Connected {
                connection_epoch: 1
            },
            "{label}"
        );
        first.write_all(&malformed).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut disconnected = false;
        while !disconnected {
            assert!(Instant::now() < deadline, "{label}: no disconnect");
            match service.event_timeout(Duration::from_millis(200)).ok() {
                Some(OutputTransportServiceEvent::Disconnected { connection_epoch }) => {
                    assert_eq!(connection_epoch, 1, "{label}");
                    disconnected = true;
                }
                Some(OutputTransportServiceEvent::ConnectionRejected { .. }) | None => {}
                Some(OutputTransportServiceEvent::Failed { message }) => {
                    panic!("{label}: malformed client input ended the service: {message}")
                }
                Some(other) => panic!("{label}: unexpected event {other:?}"),
            }
        }
        let mut rest = Vec::new();
        assert_eq!(
            first.read_to_end(&mut rest).unwrap(),
            0,
            "{label}: the offending connection must be closed without a reply"
        );

        // The same admitted process connects again and is served normally.
        let mut second = connect_output_client(&socket_path);
        second
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let welcome = decode_output_v1_server_welcome_frame(&read_frame(&mut second)).unwrap();
        assert_eq!(welcome.connection_epoch, 2, "{label}");
        decode_output_v1_snapshot_frame(&read_frame(&mut second)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "{label}: replacement not connected"
            );
            match service.event_timeout(Duration::from_millis(200)).ok() {
                Some(OutputTransportServiceEvent::Connected { connection_epoch }) => {
                    assert_eq!(connection_epoch, 2, "{label}");
                    break;
                }
                Some(OutputTransportServiceEvent::ConnectionRejected { .. }) | None => {}
                Some(other) => panic!("{label}: unexpected event {other:?}"),
            }
        }
        second
            .write_all(
                &encode_output_v1_proposal_frame(TransactionId::from_raw(2), &proposal(2)).unwrap(),
            )
            .unwrap();
        let Ok(OutputTransportServiceEvent::Proposal { admission, .. }) =
            service.event_timeout(Duration::from_secs(2))
        else {
            panic!("{label}: the replacement's proposal was not admitted");
        };
        assert_eq!(admission, OutputProposalAdmission::Active, "{label}");
        drop(service);
    }
}

fn temporary_directory(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "sophia-output-service-{label}-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ))
}

fn snapshot() -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: 1,
        primary_output: OutputId::from_raw(1),
        heads: vec![OutputHeadDescriptor {
            head: DisplayHeadId::from_raw(1),
            generation: 1,
            label: "Display 1".into(),
            connected: true,
            enabled: true,
            current_mode: Some(DisplayModeId::from_raw(10)),
            transforms: OutputTransformSet::NORMAL,
            vrr_capable: false,
            modes: vec![OutputModeDescriptor {
                mode: DisplayModeId::from_raw(10),
                pixel_size: Size {
                    width: 1_920,
                    height: 1_080,
                },
                refresh_millihz: 60_000,
                preferred: true,
            }],
        }],
        groups: vec![OutputLogicalGroupState {
            output: OutputId::from_raw(1),
            generation: 1,
            logical: Rect {
                x: 0,
                y: 0,
                width: 1_920,
                height: 1_080,
            },
            members: vec![OutputGroupMember {
                head: DisplayHeadId::from_raw(1),
                mapping: OutputHeadMapping::Exact,
            }],
        }],
    }
}

fn proposal(connection_epoch: u64) -> OutputV1Proposal {
    OutputV1Proposal {
        connection_epoch,
        candidate: OutputTopologyCandidate {
            base_topology_epoch: 1,
            intent: OutputTopologyIntent::ValidateOnly,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(1),
                head_generation: 1,
                mode: DisplayModeId::from_raw(10),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Disabled,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: OutputId::from_raw(1),
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 1_920,
                    height: 1_080,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(1),
                    mapping: OutputHeadMapping::Exact,
                }],
            }],
        },
    }
}

fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut header = [0; SOPHIA_IPC_HEADER_LEN];
    stream.read_exact(&mut header).unwrap();
    let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
    let mut frame = header.to_vec();
    frame.resize(SOPHIA_IPC_HEADER_LEN + payload_len, 0);
    stream
        .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    frame
}

fn connect_output_client(path: &std::path::Path) -> UnixStream {
    let mut stream = UnixStream::connect(path).unwrap();
    stream
        .write_all(
            &encode_output_v1_client_hello_frame(OutputV1ClientHello {
                minimum_revision: 1,
                maximum_revision: 1,
                capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
            })
            .unwrap(),
        )
        .unwrap();
    stream
}

use OutputFileNode as Node;

#[path = "support/shell_file_peer.rs"]
mod raw_peer;
use sophia_9p::connection::ConnectionId;
use sophia_9p::export::*;
use sophia_9p::{Errno, OpenFlags, ReadOutcome};
use sophia_protocol::output_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use std::time::{Duration, Instant};

#[path = "support/output_file_connection_retirement.rs"]
mod connection_retirement;

fn snapshot(epoch: u64) -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: epoch,
        primary_output: OutputId::from_raw(1),
        heads: vec![OutputHeadDescriptor {
            head: DisplayHeadId::from_raw(1),
            generation: 1,
            label: "panel".into(),
            connected: true,
            enabled: true,
            vrr_capable: false,
            transforms: OutputTransformSet::ALL,
            current_mode: Some(DisplayModeId::from_raw(1)),
            modes: vec![OutputModeDescriptor {
                mode: DisplayModeId::from_raw(1),
                pixel_size: Size {
                    width: 800,
                    height: 600,
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
                width: 800,
                height: 600,
            },
            members: vec![OutputGroupMember {
                head: DisplayHeadId::from_raw(1),
                mapping: OutputHeadMapping::Exact,
            }],
        }],
    }
}

fn export() -> OutputFileExport {
    let mut export = OutputFileExport::new(
        7,
        OutputFileLimits::default(),
        snapshot(4),
        OutputFileQids::default(),
    )
    .unwrap();
    export.bind_connection(ConnectionId(1)).unwrap();
    export
        .attach(&AttachContext {
            connection: ConnectionId(1),
            peer: None,
            uname: b"",
            aname: b"",
            n_uname: 0,
        })
        .unwrap();
    export
}

fn negotiate(export: &mut OutputFileExport, capabilities: u64) -> (OutputFileHandle, Vec<u8>) {
    let candidate = encode_output_file_record(
        OutputFileHeader {
            kind: OutputFileKind::Negotiate,
            connection_epoch: 7,
            submission_id: 1,
            sequence: 0,
        },
        &encode_output_file_negotiate(OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities,
        }),
    )
    .unwrap();
    let mut staging = export.open(&Node::Transaction, OpenFlags(2)).unwrap();
    export
        .write(&Node::Transaction, &mut staging, 0, &candidate)
        .unwrap();
    let control = encode_output_file_submit(OutputFileSubmit {
        connection_epoch: 7,
        submission_id: 1,
        candidate_bytes: candidate.len() as u32,
    })
    .unwrap()
    .to_vec();
    let mut submit = export.open(&Node::Submit, OpenFlags(1)).unwrap();
    export
        .write(&Node::Submit, &mut submit, 0, &control)
        .unwrap();
    (staging, control)
}

fn ack(export: &mut OutputFileExport, sequence: u64) -> Result<u32, Errno> {
    let mut handle = export.open(&Node::Ack, OpenFlags(1))?;
    export.write(
        &Node::Ack,
        &mut handle,
        0,
        &encode_output_file_ack(OutputFileAck {
            connection_epoch: 7,
            sequence,
        })
        .unwrap(),
    )
}

fn bytes(result: ReadOutcome) -> Vec<u8> {
    match result {
        ReadOutcome::Ready(bytes) => bytes,
        ReadOutcome::Pending => panic!("pending"),
    }
}

#[test]
fn cumulative_ack_requires_every_event_byte_and_exact_topology() {
    let mut export = export();
    assert!(matches!(
        export.open(&Node::Topology, OpenFlags(0)),
        Err(Errno::EAGAIN)
    ));
    let (staging, _) = negotiate(&mut export, SOPHIA_OUTPUT_CAPABILITY_OBSERVE);
    let mut events = export.open(&Node::Events, OpenFlags(0)).unwrap();
    let tail = export.describe(&Node::Events, None).size;
    assert_eq!(ack(&mut export, 3), Err(Errno::EAGAIN));
    // Out-of-order reads cover everything except one byte in Submitted.
    export
        .read(&Node::Events, &mut events, 1, tail as u32)
        .unwrap();
    assert_eq!(ack(&mut export, 1), Err(Errno::EAGAIN));
    export.read(&Node::Events, &mut events, 0, 1).unwrap();
    assert_eq!(ack(&mut export, 1), Ok(16));
    assert_eq!(ack(&mut export, 3), Err(Errno::EAGAIN));
    let mut topology = export.open(&Node::Topology, OpenFlags(0)).unwrap();
    let size = export.describe(&Node::Topology, Some(&topology)).size;
    export
        .read(&Node::Topology, &mut topology, 1, size as u32)
        .unwrap();
    assert_eq!(ack(&mut export, 3), Err(Errno::EAGAIN));
    export.read(&Node::Topology, &mut topology, 0, 1).unwrap();
    assert_eq!(ack(&mut export, 3), Ok(16));
    assert_eq!(ack(&mut export, 3), Ok(16));
    assert_eq!(
        export.read(&Node::Events, &mut events, 0, 1),
        Err(Errno::ESTALE)
    );
    assert_eq!(
        export.read(&Node::Events, &mut events, tail, 1),
        Ok(ReadOutcome::Pending)
    );
    assert_eq!(
        export.read(&Node::Events, &mut events, tail + 1, 1),
        Err(Errno::EINVAL)
    );
    export.release(Node::Transaction, Some(staging));
    assert!(export.open(&Node::Transaction, OpenFlags(2)).is_ok());
}

#[test]
fn publication_backpressure_preserves_announced_object_and_old_pin() {
    let mut export = export();
    negotiate(&mut export, SOPHIA_OUTPUT_CAPABILITY_OBSERVE);
    let mut topology = export.open(&Node::Topology, OpenFlags(0)).unwrap();
    let old_qid = export.describe(&Node::Topology, Some(&topology)).qid_path;
    assert_eq!(export.publish(&snapshot(5)), Err(Errno::EAGAIN));
    let old_bytes = bytes(
        export
            .read(&Node::Topology, &mut topology, 0, 65_536)
            .unwrap(),
    );
    let mut events = export.open(&Node::Events, OpenFlags(0)).unwrap();
    export.read(&Node::Events, &mut events, 0, 65_536).unwrap();
    ack(&mut export, 3).unwrap();
    let qid = export.publish(&snapshot(5)).unwrap();
    assert_ne!(qid, old_qid);
    assert_eq!(
        export.describe(&Node::Topology, Some(&topology)).qid_path,
        old_qid
    );
    assert_eq!(
        bytes(
            export
                .read(&Node::Topology, &mut topology, 0, 65_536)
                .unwrap()
        ),
        old_bytes
    );
    assert!(matches!(
        export.open(&Node::Topology, OpenFlags(0)),
        Err(Errno(16))
    ));
    export
        .read(&Node::Events, &mut events, 160, 65_536)
        .unwrap();
    assert_eq!(ack(&mut export, 4), Err(Errno::EAGAIN));
    export.release(Node::Topology, Some(topology));
    let mut current = export.open(&Node::Topology, OpenFlags(0)).unwrap();
    assert_eq!(
        export.describe(&Node::Topology, Some(&current)).qid_path,
        qid
    );
    export
        .read(&Node::Topology, &mut current, 0, 65_536)
        .unwrap();
    ack(&mut export, 4).unwrap();
    assert!(export.publish(&snapshot(6)).is_ok());
}

#[test]
fn exact_submit_repeat_is_idempotent_and_receipt_blocks_next_staging() {
    let mut export = export();
    let (staging, control) = negotiate(&mut export, SOPHIA_OUTPUT_CAPABILITY_OBSERVE);
    let before = export.admission().journal().position();
    let mut submit = export.open(&Node::Submit, OpenFlags(1)).unwrap();
    export
        .write(&Node::Submit, &mut submit, 0, &control)
        .unwrap();
    assert_eq!(export.admission().journal().position(), before);
    assert!(matches!(
        export.take_delivery(),
        Some(OutputFileSubmission::Negotiated(_))
    ));
    assert!(export.take_delivery().is_none());
    export.release(Node::Transaction, Some(staging));
    assert!(matches!(
        export.open(&Node::Transaction, OpenFlags(2)),
        Err(Errno(16))
    ));
    let mut events = export.open(&Node::Events, OpenFlags(0)).unwrap();
    export.read(&Node::Events, &mut events, 0, 48).unwrap();
    ack(&mut export, 1).unwrap();
    assert!(export.open(&Node::Transaction, OpenFlags(2)).is_ok());
}

#[test]
fn refused_connection_drains_terminal_record_before_revocation() {
    let mut export = export();
    negotiate(&mut export, 0);
    assert!(!export.is_revoked());
    let mut events = export.open(&Node::Events, OpenFlags(0)).unwrap();
    assert_eq!(
        bytes(export.read(&Node::Events, &mut events, 0, 65_536).unwrap()).len(),
        88
    );
    ack(&mut export, 2).unwrap();
    assert!(export.is_revoked());
    assert_eq!(
        export.read(&Node::Events, &mut events, 0, 1),
        Err(Errno::ESTALE)
    );
}

#[test]
fn restaged_replay_does_not_need_another_owner_handoff_slot() {
    let mut export = export();
    let (mut staging, control) = negotiate(&mut export, SOPHIA_OUTPUT_CAPABILITY_OBSERVE);
    let original = bytes(
        export
            .read(&Node::Transaction, &mut staging, 0, 1784)
            .unwrap(),
    );
    export.release(Node::Transaction, Some(staging));
    let mut events = export.open(&Node::Events, OpenFlags(0)).unwrap();
    export.read(&Node::Events, &mut events, 0, 48).unwrap();
    ack(&mut export, 1).unwrap();
    // The worker has not taken the original negotiation delivery yet.
    let before = export.admission().journal().position();
    let mut restaged = export.open(&Node::Transaction, OpenFlags(2)).unwrap();
    export
        .write(&Node::Transaction, &mut restaged, 0, &original)
        .unwrap();
    let mut submit = export.open(&Node::Submit, OpenFlags(1)).unwrap();
    assert_eq!(
        export.write(&Node::Submit, &mut submit, 0, &control),
        Ok(24)
    );
    assert_eq!(export.admission().journal().position(), before);
    assert!(matches!(
        export.take_delivery(),
        Some(OutputFileSubmission::Negotiated(_))
    ));
    assert!(export.take_delivery().is_none());
}

#[test]
fn deadlines_revoke_stalled_journal_and_expire_only_unsubmitted_staging() {
    let mut export = export();
    let mut staging = export.open(&Node::Transaction, OpenFlags(2)).unwrap();
    export
        .write(&Node::Transaction, &mut staging, 0, &[48])
        .unwrap();
    export.expire(Instant::now() + Duration::from_secs(13));
    assert!(!export.is_revoked());
    assert_eq!(
        export.read(&Node::Transaction, &mut staging, 0, 1),
        Err(Errno::ESTALE)
    );
    negotiate(&mut export, SOPHIA_OUTPUT_CAPABILITY_OBSERVE);
    export.expire(Instant::now() + Duration::from_secs(3));
    assert!(export.is_revoked());
}

#[test]
fn attach_needs_bound_connection_and_cannot_renew_an_epoch() {
    let mut export = export();
    let context = AttachContext {
        connection: ConnectionId(1),
        peer: None,
        uname: b"root",
        aname: b"output",
        n_uname: 0,
    };
    assert!(matches!(export.attach(&context), Err(Errno::EACCES)));
    assert_eq!(export.bind_connection(ConnectionId(2)), Err(Errno::EACCES));
    assert_eq!(
        export.check(&Access {
            connection: ConnectionId(2),
            epoch: Epoch(7),
            node: &Node::Root,
            operation: Operation::Walk
        }),
        Err(Errno::ESTALE)
    );
}

#[test]
fn protected_transport_serves_native_records_and_fresh_reconnect_identity() {
    use std::os::unix::net::UnixStream;
    let directory = std::env::temp_dir().join(format!("output-files-{}", std::process::id()));
    let mut transport = OutputFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        7,
        OutputFileLimits::default(),
    )
    .unwrap();
    // A real connecting process with the wrong supervised PID is refused
    // before it can attach, even though its UID matches.
    transport
        .authorize_supervised_pid(rustix::process::getppid().unwrap().as_raw_pid() as u32)
        .unwrap();
    let wrong = UnixStream::connect(transport.socket_path()).unwrap();
    assert!(matches!(
        transport.poll_accept(&snapshot(4)),
        Err(OutputFileTransportError::Endpoint(_))
    ));
    drop(wrong);
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let mut last_qid = 0;
    for epoch in [7, 8] {
        assert!(!transport.poll_accept(&snapshot(4)).unwrap());
        let path = transport.socket_path().to_owned();
        let client = std::thread::spawn(move || {
            let mut peer = raw_peer::Peer::connect(&path);
            peer.setup();
            peer.open(6, b"limits", 0);
            let limits = peer.read(6, 0);
            let record = decode_output_file_record(&limits, OutputFileClass::Object).unwrap();
            assert_eq!(record.header.connection_epoch, epoch);
            assert_eq!(limits.len(), 72);
            peer.open(5, b"transaction", 2);
            let candidate = encode_output_file_record(
                OutputFileHeader {
                    kind: OutputFileKind::Negotiate,
                    connection_epoch: epoch,
                    submission_id: 1,
                    sequence: 0,
                },
                &encode_output_file_negotiate(OutputV1ClientHello {
                    minimum_revision: 1,
                    maximum_revision: 1,
                    capabilities: 1,
                }),
            )
            .unwrap();
            assert_eq!(peer.write(5, &candidate).0, 119);
            let submit = encode_output_file_submit(OutputFileSubmit {
                connection_epoch: epoch,
                submission_id: 1,
                candidate_bytes: candidate.len() as u32,
            })
            .unwrap();
            assert_eq!(peer.write(3, &submit).0, 119);
            let events = peer.read(2, 0);
            assert_eq!(events.len(), 160);
            let publication =
                decode_output_file_record(&events[104..], OutputFileClass::Event).unwrap();
            let publication = decode_output_file_publication(publication.body).unwrap();
            let ack = encode_output_file_ack(OutputFileAck {
                connection_epoch: epoch,
                sequence: 3,
            })
            .unwrap();
            let early = peer.write(4, &ack);
            assert_eq!(early.0, 7);
            assert_eq!(u32::from_le_bytes(early.1.try_into().unwrap()), 11);
            peer.walk(7, b"topology");
            let opened = peer
                .rpc(12, &[7u32.to_le_bytes(), 0u32.to_le_bytes()].concat())
                .unwrap();
            assert_eq!(opened.0, 13);
            assert_eq!(
                u64::from_le_bytes(opened.1[5..13].try_into().unwrap()),
                publication.qid_path
            );
            let topology = peer.read(7, 0);
            let topology = decode_output_file_record(&topology, OutputFileClass::Object).unwrap();
            assert_eq!(
                decode_output_file_topology(topology.body, epoch)
                    .unwrap()
                    .snapshot,
                snapshot(4)
            );
            assert_eq!(peer.write(4, &ack).0, 119);
            peer.clunk(5);
            peer.clunk(7);
            publication.qid_path
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut accepted = false;
        let mut delivered = 0;
        while !client.is_finished() {
            assert!(Instant::now() < deadline, "output file client stalled");
            if !accepted {
                accepted = transport.poll_accept(&snapshot(4)).unwrap()
            }
            if accepted {
                transport.turn().unwrap();
                if let Some(event) = transport.take_delivery() {
                    assert!(matches!(event, OutputFileSubmission::Negotiated(_)));
                    delivered += 1;
                }
            }
            std::thread::sleep(Duration::from_micros(100));
        }
        let qid = client.join().unwrap();
        assert_eq!(delivered, 1);
        assert!(qid > last_qid);
        last_qid = qid;
        assert!(transport.disconnect().unwrap().is_empty());
    }
    drop(transport);
    assert!(!directory.exists());
}

#[test]
fn worker_delivers_once_settles_reserved_outcome_and_pauses_without_a_peer() {
    let directory = std::env::temp_dir().join(format!("output-file-worker-{}", std::process::id()));
    let mut transport = OutputFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        7,
        OutputFileLimits::default(),
    )
    .unwrap();
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let path = transport.socket_path().to_owned();
    let service = OutputFileService::spawn(transport, snapshot(4)).unwrap();
    let client = std::thread::spawn(move || {
        let mut peer = raw_peer::Peer::connect(&path);
        peer.setup();
        let candidate = |kind, submission, body: &[u8]| {
            encode_output_file_record(
                OutputFileHeader {
                    kind,
                    connection_epoch: 7,
                    submission_id: submission,
                    sequence: 0,
                },
                body,
            )
            .unwrap()
        };
        let submit = |peer: &mut raw_peer::Peer, submission, bytes: &[u8]| {
            peer.open(5, b"transaction", 2);
            assert_eq!(peer.write(5, bytes).0, 119);
            assert_eq!(
                peer.write(
                    3,
                    &encode_output_file_submit(OutputFileSubmit {
                        connection_epoch: 7,
                        submission_id: submission,
                        candidate_bytes: bytes.len() as u32,
                    })
                    .unwrap()
                )
                .0,
                119
            );
        };
        submit(
            &mut peer,
            1,
            &candidate(
                OutputFileKind::Negotiate,
                1,
                &encode_output_file_negotiate(OutputV1ClientHello {
                    minimum_revision: 1,
                    maximum_revision: 1,
                    capabilities: 3,
                }),
            ),
        );
        assert_eq!(peer.read(2, 0).len(), 160);
        peer.open(7, b"topology", 0);
        peer.read(7, 0);
        let acknowledge = |peer: &mut raw_peer::Peer, sequence| {
            assert_eq!(
                peer.write(
                    4,
                    &encode_output_file_ack(OutputFileAck {
                        connection_epoch: 7,
                        sequence,
                    })
                    .unwrap()
                )
                .0,
                119
            );
        };
        acknowledge(&mut peer, 3);
        peer.clunk(5);
        peer.clunk(7);
        let facts = snapshot(4);
        let proposal = OutputV1Proposal {
            connection_epoch: 7,
            candidate: OutputTopologyCandidate {
                base_topology_epoch: 4,
                intent: OutputTopologyIntent::ValidateOnly,
                primary_group_index: 0,
                heads: vec![OutputHeadTargetProposal {
                    head: DisplayHeadId::from_raw(1),
                    head_generation: 1,
                    mode: DisplayModeId::from_raw(1),
                    transform: OutputTransform::Normal,
                    vrr: OutputVrrPolicy::Disabled,
                }],
                groups: vec![OutputLogicalGroupProposal {
                    output: facts.primary_output,
                    logical: facts.groups[0].logical,
                    members: facts.groups[0].members.clone(),
                }],
            },
        };
        submit(
            &mut peer,
            2,
            &candidate(
                OutputFileKind::Proposal,
                2,
                &encode_output_file_proposal(TransactionId::from_raw(90), &proposal).unwrap(),
            ),
        );
        let mut records = peer.read(2, 160);
        while records.len() < 104 {
            records.extend(peer.read(2, 160 + records.len() as u64));
        }
        assert_eq!(records.len(), 104);
        let receipt = decode_output_file_record(&records[..48], OutputFileClass::Event).unwrap();
        assert_eq!(receipt.header.sequence, 4);
        let terminal = decode_output_file_record(&records[48..], OutputFileClass::Event).unwrap();
        let (transaction, outcome) = decode_output_file_outcome(terminal.body, 7).unwrap();
        assert_eq!(transaction.raw(), 90);
        assert_eq!(outcome.kind, OutputV1OutcomeKind::Validated);
        acknowledge(&mut peer, 5);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut proposals = 0;
    while !client.is_finished() {
        assert!(Instant::now() < deadline, "worker client stalled");
        if let Some(event) = service.try_event().unwrap() {
            match event {
                OutputFileServiceEvent::Connected { connection_epoch } => {
                    assert_eq!(connection_epoch, 7)
                }
                OutputFileServiceEvent::Proposal {
                    proposal,
                    admission,
                } => {
                    assert_eq!(admission, OutputProposalAdmission::Active);
                    proposals += 1;
                    service
                        .command(OutputFileServiceCommand::Settle {
                            transaction: proposal.transaction,
                            outcome: OutputV1Outcome {
                                connection_epoch: 7,
                                topology_epoch: 4,
                                kind: OutputV1OutcomeKind::Validated,
                                reason: 0,
                            },
                        })
                        .unwrap();
                }
                OutputFileServiceEvent::Disconnected { .. } => {}
                other => panic!("unexpected event: {other:?}"),
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    client.join().unwrap();
    assert_eq!(proposals, 1);
    assert!(
        service
            .pause_acceptance(Duration::from_secs(1))
            .unwrap()
            .is_empty()
    );
    let start = Instant::now();
    drop(service);
    assert!(start.elapsed() < Duration::from_secs(1));
    assert!(!directory.exists());
}

#[test]
fn independent_c_sdk_session_against_the_output_file_worker() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let build = std::env::temp_dir().join(format!("output-c-peer-build-{}", std::process::id()));
    std::fs::create_dir(&build).unwrap();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = repo.join("vendor/c-desktop-sdk/source/src");
    let binary = build.join("output-peer");
    let mut compiler = Command::new("timeout");
    compiler
        .args(["-s", "KILL", "90"])
        .arg(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-UNDEBUG",
            "-I",
        ])
        .arg(&source);
    for directory in ["nine_p", "output_files", "output_session"] {
        let mut files = std::fs::read_dir(source.join(directory))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "c"))
            .collect::<Vec<_>>();
        files.sort();
        compiler.args(files);
    }
    let compiled = compiler
        .arg(repo.join("crates/sophia-runtime/tests/support/output_files_peer.c"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let directory = std::env::temp_dir().join(format!("output-c-worker-{}", std::process::id()));
    let mut transport = OutputFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        7,
        OutputFileLimits::default(),
    )
    .unwrap();
    let mut peer = Command::new(binary)
        .arg(transport.socket_path())
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    transport.authorize_supervised_pid(peer.id()).unwrap();
    let service = OutputFileService::spawn(transport, snapshot(4)).unwrap();
    peer.stdin.take().unwrap().write_all(b"G").unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut proposals = 0;
    let status = loop {
        if let Some(status) = peer.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            peer.kill().unwrap();
            peer.wait().unwrap();
            panic!("independent output peer exceeded its deadline");
        }
        if let Some(event) = service.try_event().unwrap() {
            match event {
                OutputFileServiceEvent::Connected { connection_epoch } => {
                    assert_eq!(connection_epoch, 7)
                }
                OutputFileServiceEvent::Proposal {
                    proposal,
                    admission,
                } => {
                    assert_eq!(admission, OutputProposalAdmission::Active);
                    assert_eq!(
                        proposal.message.candidate.intent,
                        OutputTopologyIntent::ValidateOnly
                    );
                    proposals += 1;
                    service
                        .command(OutputFileServiceCommand::Settle {
                            transaction: proposal.transaction,
                            outcome: OutputV1Outcome {
                                connection_epoch: 7,
                                topology_epoch: 4,
                                kind: OutputV1OutcomeKind::Validated,
                                reason: 0,
                            },
                        })
                        .unwrap();
                }
                OutputFileServiceEvent::Disconnected { .. } => {}
                other => panic!("unexpected event: {other:?}"),
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(status.success(), "independent peer: {status}");
    assert_eq!(proposals, 1);
    drop(service);
    assert!(!directory.exists());
    std::fs::remove_dir_all(build).unwrap();
}

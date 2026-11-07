use super::*;

const CAPS: u64 = BAR | IMAGE_CAPABILITY_OWNED_RING | SHELL_FRAME_CAPABILITY;

fn profile() -> Profile {
    Profile {
        role: "bar",
        revision: 13,
        capabilities: CAPS,
        limits_published: true,
    }
}

fn frame_connect(
    profile: Profile,
    options: ShellClientOptions,
    max_bytes: u32,
) -> Result<(ShellConnection, Peer), ShellClientError> {
    let path = socket_path();
    let listener = UnixListener::bind(&path).unwrap();
    let mut limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: EPOCH,
        content_grant_epoch: CONTENT_GRANT_EPOCH,
    });
    limits.max_candidate_bytes = max_bytes;
    let thread = std::thread::spawn(move || Peer::handshake_with_limits(listener, profile, limits));
    let result = ShellConnection::connect_files(&path, options);
    let peer = thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    result.map(|connection| (connection, peer))
}

fn frame_options() -> ShellClientOptions {
    ShellClientOptions {
        minimum_revision: 13,
        maximum_revision: 13,
        required_capabilities: CAPS,
        handshake_timeout: WAIT,
    }
}

fn key() -> ShellFrameStreamKey {
    ShellFrameStreamKey {
        grant: ContentGrant {
            connection_epoch: EPOCH,
            content_grant_epoch: CONTENT_GRANT_EPOCH,
        },
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        facts_generation: 1,
        stream_id: 1,
    }
}

fn open() -> ShellFrameRecord {
    ShellFrameRecord::Open {
        transaction: tx(10),
        key: key(),
    }
}

fn event(peer: &mut Peer, value: &ShellFrameRecord) -> Vec<u8> {
    encode_shell_frame_record(
        ShellFileHeader {
            kind: value.kind(),
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: peer.take_sequence(),
        },
        value,
        13,
    )
    .unwrap()
}

#[test]
fn frame_request_uses_whole_transaction_custody_and_typed_journal() {
    let (mut connection, mut peer) = frame_connect(profile(), frame_options(), 8192).unwrap();
    drop(connection.reserve_frame(&open()).unwrap());
    let ticket = connection.enqueue_frame(&open()).unwrap().first;
    let held = submit(&mut connection, &mut peer);
    let submit = decode_shell_file_submit(&held.data).unwrap();
    let raw = peer.transactions.last().unwrap();
    assert_eq!(
        decode_shell_frame_record(raw, ShellFileClass::Candidate, 13, 8192).unwrap(),
        open()
    );
    assert_eq!(raw.len(), submit.candidate_bytes as usize);
    assert!(connection.take_frame().unwrap().is_none());
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let receipt = encode_shell_file_submitted_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::Submitted,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: peer.take_sequence(),
        },
        ShellFileSubmitted {
            submission_id: submit.submission_id,
            candidate_kind: ShellFileKind::FrameStreamOpen,
        },
        13,
    )
    .unwrap();
    peer.push(receipt);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
    assert!(
        connection.take_frame().unwrap().is_none(),
        "custody is not stream credit"
    );
    let opened = ShellFrameRecord::Opened {
        transaction: tx(10),
        key: key(),
        ttl_millis: 100,
    };
    let bytes = event(&mut peer, &opened);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().handled >= 4
    })
    .unwrap();
    assert_eq!(connection.take_frame().unwrap(), Some(opened));
    assert!(connection.take_frame().unwrap().is_none());
    let request = ShellFrameRecord::Close {
        transaction: tx(11),
        key: key(),
    };
    assert!(connection.enqueue_frame(&request).is_ok());
}

#[test]
fn frame_retry_keeps_the_whole_record_and_custody_identity() {
    let (mut connection, mut peer) = frame_connect(profile(), frame_options(), 8192).unwrap();
    let ticket = connection.enqueue_frame(&open()).unwrap().first;
    let refused = submit(&mut connection, &mut peer);
    let raw = peer.transactions.last().unwrap().clone();
    peer.answer_error(refused.tag, EAGAIN);
    if !pass_after_eagain(&mut connection, &mut peer) {
        assert!(connection.wake_deadline().is_some());
        let opened = ShellFrameRecord::Opened {
            transaction: tx(10),
            key: key(),
            ttl_millis: 100,
        };
        let bytes = event(&mut peer, &opened);
        peer.push(bytes);
    }
    let again = submit(&mut connection, &mut peer);
    assert_eq!(again.data, refused.data);
    assert_eq!(peer.transactions, vec![raw]);
    assert_eq!(connection.custody(ticket), Some(Custody::InFlight));
    let submitted = decode_shell_file_submit(&again.data).unwrap();
    peer.answer_write(again.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let receipt = encode_shell_file_submitted_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::Submitted,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: peer.take_sequence(),
        },
        ShellFileSubmitted {
            submission_id: submitted.submission_id,
            candidate_kind: ShellFileKind::FrameStreamOpen,
        },
        13,
    )
    .unwrap();
    peer.push(receipt);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
}

#[test]
fn frame_queue_limit_refuses_without_transferring_custody() {
    let (mut connection, mut peer) = frame_connect(profile(), frame_options(), 8192).unwrap();
    connection
        .set_queue_limits(QueueLimits {
            records: 1,
            bytes: 8192,
            control_records: 0,
            control_bytes: 0,
        })
        .unwrap();
    let ticket = connection.enqueue_frame(&open()).unwrap().first;
    assert!(connection.enqueue_frame(&open()).is_err());
    assert_eq!(connection.custody(ticket), Some(Custody::Queued));
    assert!(peer.transactions.is_empty());
    let held = submit(&mut connection, &mut peer);
    assert_eq!(peer.transactions.len(), 1);
    assert_eq!(
        decode_shell_frame_record(
            peer.transactions.last().unwrap(),
            ShellFileClass::Candidate,
            13,
            8192
        )
        .unwrap(),
        open()
    );
    let submit = decode_shell_file_submit(&held.data).unwrap();
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let receipt = encode_shell_file_submitted_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::Submitted,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: peer.take_sequence(),
        },
        ShellFileSubmitted {
            submission_id: submit.submission_id,
            candidate_kind: ShellFileKind::FrameStreamOpen,
        },
        13,
    )
    .unwrap();
    peer.push(receipt);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
    assert!(connection.enqueue_frame(&open()).is_ok());
}

#[test]
fn frame_events_in_manual_mode_hold_the_shared_ack_boundary() {
    let (mut connection, mut peer) = frame_connect(profile(), frame_options(), 8192).unwrap();
    connection.set_automatic_event_delivery(false).unwrap();
    let value = ShellFrameRecord::Opened {
        transaction: tx(10),
        key: key(),
        ttl_millis: 100,
    };
    let bytes = event(&mut peer, &value);
    peer.push(bytes);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_event().is_some()
    })
    .unwrap();
    assert!(
        matches!(connection.journal_event().unwrap().record, JournalRecord::Frame(r) if *r == value)
    );
    assert_eq!(connection.journal_progress().handled, 2);
    assert!(connection.take_frame().unwrap().is_none());
    connection.consume_journal_event().unwrap();
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == 3
    })
    .unwrap();
    assert!(
        connection.take_frame().unwrap().is_none(),
        "manual consume does not duplicate delivery"
    );
}

#[test]
fn invalid_frame_event_stops_before_a_later_valid_event() {
    for wrong_epoch in [false, true] {
        let (mut connection, mut peer) = frame_connect(profile(), frame_options(), 8192).unwrap();
        let value = ShellFrameRecord::Opened {
            transaction: tx(10),
            key: key(),
            ttl_millis: 100,
        };
        let mut first = event(&mut peer, &value);
        if wrong_epoch {
            first[8..16].copy_from_slice(&(EPOCH + 1).to_le_bytes());
        } else {
            first[104..108].copy_from_slice(&2u32.to_le_bytes());
        }
        let second = event(&mut peer, &value);
        peer.push([first, second].concat());
        let error = drive(&mut connection, &mut peer, |_, _| false).unwrap_err();
        assert!(!matches!(error, ShellClientError::QueueSaturated));
        assert_eq!(connection.journal_progress().handled, 2);
        assert!(connection.take_frame().unwrap().is_none());
    }
}

#[test]
fn frame_request_refuses_legacy_direction_epoch_and_granted_byte_overflow() {
    let (mut legacy, _) = connect(bar(0));
    assert!(matches!(
        legacy.enqueue_frame(&open()),
        Err(ShellClientError::MissingCapability)
    ));
    let (mut connection, _) = frame_connect(profile(), frame_options(), 8192).unwrap();
    let value = ShellFrameRecord::Opened {
        transaction: tx(10),
        key: key(),
        ttl_millis: 100,
    };
    assert!(matches!(
        connection.enqueue_frame(&value),
        Err(ShellClientError::WrongDirection)
    ));
    let mut foreign = key();
    foreign.grant.connection_epoch += 1;
    assert!(
        connection
            .enqueue_frame(&ShellFrameRecord::Open {
                transaction: tx(10),
                key: foreign
            })
            .is_err()
    );
    let (mut small, _) = frame_connect(profile(), frame_options(), 64).unwrap();
    assert!(matches!(
        small.enqueue_frame(&open()),
        Err(ShellClientError::InvalidArgument(_))
    ));
}

#[test]
fn frame_negotiation_is_exact_and_never_implicitly_granted() {
    for index in 0..8 {
        let mut p = profile();
        let mut options = frame_options();
        match index {
            0 => p.revision = 10,
            1 => p.capabilities &= !SHELL_FRAME_CAPABILITY,
            2 => p.capabilities &= !IMAGE_CAPABILITY_OWNED_RING,
            3 => p.capabilities |= IMAGE_CAPABILITY_DMA_BUF,
            4 => p.limits_published = false,
            5 => options.required_capabilities &= !SHELL_FRAME_CAPABILITY,
            6 => p.capabilities |= 1 << 16,
            7 => p.capabilities |= 1 << 17,
            _ => unreachable!(),
        }
        assert!(frame_connect(p, options, 8192).is_err(), "case {index}");
    }
}

#[test]
fn all_complete_frame_variants_reuse_the_one_submission_lane() {
    let (mut connection, mut peer) = frame_connect(profile(), frame_options(), 8192).unwrap();
    for line in include_str!("../../../../spec/golden/sophia-frame-transactions-v1.tsv")
        .lines()
        .filter(|line| line.starts_with("commit-"))
    {
        let hex = line.split('\t').nth(2).unwrap();
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
            .collect();
        let ShellFrameRecord::Commit(mut frame) =
            decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap()
        else {
            panic!("commit golden");
        };
        frame.candidate.grant = key().grant;
        if let Some(image) = &mut frame.image {
            image.identity.connection_epoch = EPOCH;
            image.identity.content_grant_epoch = CONTENT_GRANT_EPOCH;
        }
        let record = ShellFrameRecord::Commit(frame);
        let ticket = connection.enqueue_frame(&record).unwrap().first;
        let held = submit(&mut connection, &mut peer);
        let id = decode_shell_file_submit(&held.data).unwrap().submission_id;
        assert_eq!(
            decode_shell_frame_record(
                peer.transactions.last().unwrap(),
                ShellFileClass::Candidate,
                13,
                8192
            )
            .unwrap(),
            record
        );
        peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
        let receipt = encode_shell_file_submitted_at_revision(
            ShellFileHeader {
                kind: ShellFileKind::Submitted,
                connection_epoch: EPOCH,
                submission_id: 0,
                sequence: peer.take_sequence(),
            },
            ShellFileSubmitted {
                submission_id: id,
                candidate_kind: ShellFileKind::FrameCommit,
            },
            13,
        )
        .unwrap();
        peer.push(receipt);
        drive(&mut connection, &mut peer, |c, _| {
            c.custody(ticket) == Some(Custody::Submitted)
        })
        .unwrap();
        assert!(
            peer.control_writes.is_empty(),
            "embedded offer must not use owned-ring control separately"
        );
        assert!(connection.take_frame().unwrap().is_none());
    }
}

#[path = "frame_stream.rs"]
mod stream;

#[test]
fn terminal_only_requires_matching_grant_and_sft_prerequisites() {
    for (requested, granted, sft) in [
        (true, false, true),
        (false, true, true),
        (true, true, false),
    ] {
        let mut options = frame_options();
        let mut offered = profile();
        if requested {
            options.required_capabilities |= SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
        }
        if granted {
            offered.capabilities |= SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
        }
        if !sft {
            options.required_capabilities &= !SHELL_FRAME_CAPABILITY;
            offered.capabilities &= !SHELL_FRAME_CAPABILITY;
        }
        assert!(frame_connect(offered, options, 8192).is_err());
    }
}

#[test]
fn terminal_only_uses_the_existing_role_negotiation() {
    for (role, caps) in [
        ("bar", 0x48081),
        ("launcher", 0x489a0),
        ("dock", 0x491a2),
        ("descriptor", 0x48083),
    ] {
        let caps = caps | SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
        let offered = Profile {
            role,
            revision: 13,
            capabilities: caps,
            limits_published: true,
        };
        let mut options = frame_options();
        options.required_capabilities = caps;
        assert!(frame_connect(offered, options, 8192).is_ok());
    }
}

#[path = "file_wire_frame_submit.rs"]
mod combined;

#[test]
fn lean_requires_terminal_only_and_a_matching_grant() {
    let both = SHELL_FRAME_TERMINAL_ONLY_CAPABILITY | SHELL_FRAME_LEAN_CAPABILITY;
    for (requested, granted) in [
        (both, both),
        (both, SHELL_FRAME_TERMINAL_ONLY_CAPABILITY),
        (SHELL_FRAME_TERMINAL_ONLY_CAPABILITY, both),
        (SHELL_FRAME_LEAN_CAPABILITY, both),
        (both, SHELL_FRAME_LEAN_CAPABILITY),
    ] {
        let mut options = frame_options();
        let mut offered = profile();
        options.required_capabilities |= requested;
        offered.capabilities |= granted;
        assert_eq!(
            frame_connect(offered, options, 8192).is_ok(),
            (requested, granted) == (both, both),
            "requested {requested:#x} granted {granted:#x}"
        );
    }
}

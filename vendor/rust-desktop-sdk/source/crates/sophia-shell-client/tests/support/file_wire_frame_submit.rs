use super::*;
use file_wire_peer::FrameSubmitOptions;

fn retained_head() -> (ShellConnection, Peer, u64) {
    let (connection, mut peer) = connected(FrameSubmitOptions::default());
    let mut connection = connection.unwrap();
    connection.set_automatic_event_delivery(false).unwrap();
    let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
    let first = issued(&mut connection, &mut peer);
    peer.answer_write(first.tag, first.data.len() as u32);
    let submitted = receipt(&mut peer, &first);
    peer.push(submitted);
    let head = event(
        &mut peer,
        &ShellFrameRecord::Opened {
            transaction: tx(10),
            key: key(),
            ttl_millis: 100,
        },
    );
    peer.push(head);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
            && c.journal_event().is_some()
            && !c.journal_progress().ack_pending
    })
    .unwrap();
    let sequence = connection.journal_event().unwrap().sequence;
    assert!(connection.journal_progress().ack_limit < sequence);
    peer.reset_logs();
    (connection, peer, sequence)
}

#[test]
fn consumed_head_ack_is_queued_before_an_already_ready_frame() {
    let (mut connection, mut peer, sequence) = retained_head();
    connection.consume_journal_event().unwrap();
    connection.enqueue_frame(&commit_record()).unwrap();
    eprintln!("ready-batch start");
    connection.poll_io().unwrap();
    eprintln!("ready-batch end");
    peer.pump();
    assert_eq!(peer.frame_writes.len(), 1);
    let ack = peer
        .log
        .iter()
        .position(|r| *r == Logged::Ack(sequence))
        .unwrap();
    let frame = peer
        .log
        .iter()
        .position(|r| *r == Logged::FrameSubmit)
        .unwrap();
    assert!(
        ack < frame,
        "consumed-head ACK was ready before the frame: {:?}",
        peer.log
    );
}

#[test]
fn ready_frame_does_not_ack_an_unconsumed_head_or_enable_manual_ack() {
    for consume in [false, true] {
        let (mut connection, mut peer, sequence) = retained_head();
        if consume {
            connection.set_automatic_journal_ack(false).unwrap();
            connection.consume_journal_event().unwrap();
        }
        connection.enqueue_frame(&commit_record()).unwrap();
        connection.poll_io().unwrap();
        peer.pump();
        assert_eq!(peer.frame_writes.len(), 1);
        assert!(peer.acks.is_empty(), "ineligible ACK was sent");
        assert_eq!(
            connection.journal_progress().ack_limit,
            sequence - u64::from(!consume)
        );
    }
}

#[test]
fn ready_batch_respects_zero_byte_budget_and_recovers_without_peer_input() {
    let (mut connection, mut peer, sequence) = retained_head();
    connection.consume_journal_event().unwrap();
    connection.enqueue_frame(&commit_record()).unwrap();
    let now = Instant::now();
    connection.dispatch(now, 0).unwrap();
    peer.pump();
    assert!(peer.frame_writes.is_empty() && peer.acks.is_empty());
    connection.dispatch(now, 65536).unwrap();
    peer.pump();
    assert_eq!(peer.frame_writes.len(), 1);
    assert_eq!(
        peer.acks.iter().map(|ack| ack.sequence).collect::<Vec<_>>(),
        [sequence]
    );
}

fn connected(options: FrameSubmitOptions) -> (Result<ShellConnection, ShellClientError>, Peer) {
    let path = socket_path();
    let listener = UnixListener::bind(&path).unwrap();
    let thread =
        std::thread::spawn(move || Peer::frame_submit_handshake(listener, profile(), options));
    let connection = ShellConnection::connect_files(&path, frame_options());
    let peer = thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    (connection, peer)
}

fn commit_record() -> ShellFrameRecord {
    let line = include_str!("../../../../spec/golden/sophia-frame-transactions-v1.tsv")
        .lines()
        .find(|line| line.starts_with("commit-copy-retire\t"))
        .unwrap();
    let bytes: Vec<u8> = line
        .split('\t')
        .nth(2)
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let ShellFrameRecord::Commit(mut frame) =
        decode_shell_frame_record(&bytes, ShellFileClass::Candidate, 13, 8192).unwrap()
    else {
        unreachable!()
    };
    frame.candidate.grant = key().grant;
    let image = frame.image.as_mut().unwrap();
    image.identity.connection_epoch = EPOCH;
    image.identity.content_grant_epoch = CONTENT_GRANT_EPOCH;
    ShellFrameRecord::Commit(frame)
}

fn issued(connection: &mut ShellConnection, peer: &mut Peer) -> Held {
    drive(connection, peer, |_, peer| !peer.frame_writes.is_empty()).unwrap();
    assert!(peer.transactions.is_empty() && peer.submits.is_empty());
    peer.frame_writes.pop_front().unwrap()
}

fn receipt(peer: &mut Peer, held: &Held) -> Vec<u8> {
    let header = decode_shell_file_record_at_revision(&held.data, ShellFileClass::Candidate, 13)
        .unwrap()
        .header;
    encode_shell_file_submitted_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::Submitted,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: peer.take_sequence(),
        },
        ShellFileSubmitted {
            submission_id: header.submission_id,
            candidate_kind: ShellFileKind::FrameCommit,
        },
        13,
    )
    .unwrap()
}

#[test]
fn complete_commit_is_one_write_and_custody_accepts_both_reply_orders() {
    for event_first in [false, true] {
        let (connection, mut peer) = connected(FrameSubmitOptions::default());
        let mut connection = connection.unwrap();
        peer.reset_logs();
        let record = commit_record();
        let ticket = connection.enqueue_frame(&record).unwrap().first;
        let held = issued(&mut connection, &mut peer);
        assert_eq!(
            decode_shell_frame_record(&held.data, ShellFileClass::Candidate, 13, 8192).unwrap(),
            record
        );
        let event = receipt(&mut peer, &held);
        if event_first {
            peer.push(event.clone());
            drive(&mut connection, &mut peer, |c, _| {
                c.custody(ticket) == Some(Custody::Submitted)
            })
            .unwrap();
        }
        peer.answer_write(held.tag, held.data.len() as u32);
        if !event_first {
            peer.push(event);
        }
        drive(&mut connection, &mut peer, |c, _| {
            c.journal_progress().acknowledged >= 3
        })
        .unwrap();
        assert_eq!(connection.custody(ticket), Some(Custody::Submitted));
        assert!(
            connection.take_frame().unwrap().is_none(),
            "custody grants no presentation or stream credit"
        );
        assert!(peer.transactions.is_empty() && peer.submits.is_empty());
    }
}

#[test]
fn absent_or_small_iounit_uses_the_existing_pair_before_issuing() {
    for options in [
        FrameSubmitOptions {
            lookup_error: Some(2),
            ..Default::default()
        },
        FrameSubmitOptions {
            iounit: 32,
            ..Default::default()
        },
    ] {
        let (connection, mut peer) = connected(options);
        let mut connection = connection.unwrap();
        peer.reset_logs();
        connection.enqueue_frame(&commit_record()).unwrap();
        let held = submit(&mut connection, &mut peer);
        assert_eq!(held.data.len(), 24);
        assert_eq!(peer.transactions.len(), 1);
        assert!(peer.frame_writes.is_empty());
    }
}

#[test]
fn discovery_errors_never_downgrade_an_advertised_node() {
    for error in [5, 13, 116] {
        for open in [false, true] {
            let options = if open {
                FrameSubmitOptions {
                    open_error: Some(error),
                    ..Default::default()
                }
            } else {
                FrameSubmitOptions {
                    lookup_error: Some(error),
                    ..Default::default()
                }
            };
            let (connection, peer) = connected(options);
            assert!(connection.is_err());
            assert!(peer.frame_writes.is_empty() && peer.transactions.is_empty());
        }
    }
}

#[test]
fn combined_eagain_retries_the_identical_complete_record_on_the_same_node() {
    let (connection, mut peer) = connected(FrameSubmitOptions::default());
    let mut connection = connection.unwrap();
    let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
    let held = issued(&mut connection, &mut peer);
    peer.answer_error(held.tag, EAGAIN);
    let retry = issued(&mut connection, &mut peer);
    assert_eq!(retry.data, held.data);
    peer.answer_write(retry.tag, retry.data.len() as u32);
    let event = receipt(&mut peer, &retry);
    peer.push(event);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
}

#[test]
fn combined_uncertain_or_short_replies_do_not_restart_on_the_old_path() {
    for short in [false, true] {
        let (connection, mut peer) = connected(FrameSubmitOptions::default());
        let mut connection = connection.unwrap();
        let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
        let held = issued(&mut connection, &mut peer);
        if short {
            peer.answer_write(held.tag, held.data.len() as u32 - 1);
        } else {
            peer.answer_error(held.tag, EALREADY);
        }
        assert!(drive(&mut connection, &mut peer, |_, _| false).is_err());
        assert_eq!(connection.custody(ticket), Some(Custody::Unknown));
        assert!(peer.transactions.is_empty() && peer.submits.is_empty());
    }
}

#[test]
fn combined_definitive_refusal_and_revocation_keep_existing_custody_results() {
    for error in [EINVAL, 116] {
        let (connection, mut peer) = connected(FrameSubmitOptions::default());
        let mut connection = connection.unwrap();
        let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
        let held = issued(&mut connection, &mut peer);
        peer.answer_error(held.tag, error);
        let (custody, _) = settle(&mut connection, &mut peer, ticket);
        if error == EINVAL {
            assert_eq!(custody, Custody::Refused(error));
        } else {
            assert_ne!(custody, Custody::Submitted);
        }
        assert!(peer.transactions.is_empty() && peer.submits.is_empty());
    }
}

fn caller_connected(msize: u32) -> (ShellConnection, Peer) {
    use sophia_9p_client::pipeline::PipelineLimits;
    use sophia_shell_client::Connecting;
    let path = socket_path();
    let listener = UnixListener::bind(&path).unwrap();
    let thread = std::thread::spawn(move || {
        Peer::frame_submit_handshake(listener, profile(), FrameSubmitOptions::default())
    });
    let mut connecting = Connecting::direct(
        std::os::unix::net::UnixStream::connect(&path).unwrap(),
        PipelineLimits {
            msize,
            ..PipelineLimits::default()
        },
        frame_options(),
        0,
        2000,
    )
    .unwrap();
    let started = std::time::Instant::now();
    while !connecting.ready() {
        assert!(started.elapsed() < WAIT);
        connecting
            .dispatch(started.elapsed().as_millis() as u64, 65536)
            .unwrap();
        std::thread::yield_now();
    }
    let connection = connecting.take_connection().unwrap();
    let mut peer = thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    peer.reset_logs();
    (connection, peer)
}

#[test]
fn caller_driven_handshake_discovers_the_same_complete_write_path() {
    let (mut connection, mut peer) = caller_connected(65536);
    connection.enqueue_frame(&commit_record()).unwrap();
    issued(&mut connection, &mut peer);
}

#[test]
fn complete_write_does_not_bypass_the_existing_small_msize_refusal() {
    let (mut connection, mut peer) = caller_connected(4096);
    let ShellFrameRecord::Commit(mut frame) = commit_record() else {
        unreachable!()
    };
    frame
        .candidate
        .placements
        .resize(32, frame.candidate.placements[0].clone());
    frame.candidate.targets = (1..=64)
        .map(|id| sophia_shell_protocol::ContentTarget {
            surface_index: 0,
            action_kind: 1,
            target_id: id,
            target_generation: 1,
            action_id: 1,
            bounds_px: sophia_shell_protocol::ContentPixelRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        })
        .collect();
    let ticket = connection
        .enqueue_frame(&ShellFrameRecord::Commit(frame))
        .unwrap()
        .first;
    assert!(connection.poll_io().is_err());
    peer.pump();
    assert!(
        peer.frame_writes.is_empty() && peer.transactions.is_empty() && peer.submits.is_empty()
    );
    assert_eq!(connection.custody(ticket), Some(Custody::DroppedUnsent));
}

#[test]
fn disconnect_does_not_erase_known_acceptance_or_replay_unknown_custody() {
    for accepted in [false, true] {
        let (connection, mut peer) = connected(FrameSubmitOptions::default());
        let mut connection = connection.unwrap();
        peer.reset_logs();
        let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
        let held = issued(&mut connection, &mut peer);
        if accepted {
            let event = receipt(&mut peer, &held);
            peer.push(event);
            drive(&mut connection, &mut peer, |c, _| {
                c.custody(ticket) == Some(Custody::Submitted)
            })
            .unwrap();
        }
        drop(peer);
        for _ in 0..16 {
            if connection.poll_io().is_err() {
                break;
            }
        }
        assert_eq!(
            connection.custody(ticket),
            Some(if accepted {
                Custody::Submitted
            } else {
                Custody::Unknown
            })
        );
    }
}

fn lean_connected() -> (ShellConnection, Peer) {
    let extra = SHELL_FRAME_TERMINAL_ONLY_CAPABILITY | SHELL_FRAME_LEAN_CAPABILITY;
    let path = socket_path();
    let listener = UnixListener::bind(&path).unwrap();
    let mut offered = profile();
    offered.capabilities |= extra;
    let mut options = frame_options();
    options.required_capabilities |= extra;
    let thread = std::thread::spawn(move || {
        Peer::frame_submit_handshake(listener, offered, FrameSubmitOptions::default())
    });
    let mut connection = ShellConnection::connect_files(&path, options).unwrap();
    let mut peer = thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    connection.set_automatic_event_delivery(false).unwrap();
    peer.reset_logs();
    (connection, peer)
}

fn word(held: &Held) -> u64 {
    u64::from_le_bytes(held.data[24..32].try_into().unwrap())
}

fn acks(peer: &Peer) -> Vec<u64> {
    peer.log
        .iter()
        .filter_map(|r| match r {
            Logged::Ack(sequence) => Some(*sequence),
            _ => None,
        })
        .collect()
}

/// Publishes one stream head and consumes it: its sequence is now above the
/// confirmed ACK.
fn consumed_head(connection: &mut ShellConnection, peer: &mut Peer, transaction: u64) -> u64 {
    let head = event(
        peer,
        &ShellFrameRecord::Opened {
            transaction: tx(transaction),
            key: key(),
            ttl_millis: 100,
        },
    );
    peer.push(head);
    drive(connection, peer, |c, _| c.journal_event().is_some()).unwrap();
    let sequence = connection.journal_event().unwrap().sequence;
    connection.consume_journal_event().unwrap();
    sequence
}

#[test]
fn lean_commit_carries_the_ack_and_its_rwrite_alone_is_custody() {
    let (mut connection, mut peer) = lean_connected();
    // Nothing consumed yet: no word, and no Submitted is needed.
    let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
    let first = issued(&mut connection, &mut peer);
    assert_eq!(word(&first), 0);
    peer.answer_write(first.tag, first.data.len() as u32);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
    assert!(acks(&peer).is_empty() && peer.transactions.is_empty() && peer.submits.is_empty());
    assert!(connection.take_frame().unwrap().is_none());

    // A consumed head is held, rides in the next commit, and the exact Rwrite confirms it.
    let sequence = consumed_head(&mut connection, &mut peer, 10);
    connection.poll_io().unwrap();
    peer.pump();
    assert!(
        acks(&peer).is_empty(),
        "lone ACK was not held: {:?}",
        peer.log
    );
    let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
    let second = issued(&mut connection, &mut peer);
    assert_eq!(word(&second), sequence);
    let progress = connection.journal_progress();
    assert!(progress.queued == sequence && progress.acknowledged < sequence);
    peer.answer_write(second.tag, second.data.len() as u32);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
    assert_eq!(connection.journal_progress().acknowledged, sequence);
    assert!(acks(&peer).is_empty());

    // A refused commit confirms nothing; the held ACK then goes out alone.
    let sequence = consumed_head(&mut connection, &mut peer, 12);
    let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
    let third = issued(&mut connection, &mut peer);
    assert_eq!(word(&third), sequence);
    peer.answer_error(third.tag, 22);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Refused(22))
    })
    .unwrap();
    assert!(connection.journal_progress().acknowledged < sequence);
    drive(&mut connection, &mut peer, |c, _| {
        c.journal_progress().acknowledged == sequence
    })
    .unwrap();
    assert_eq!(acks(&peer), [sequence]);
}

#[test]
fn lean_paired_records_keep_the_covering_ack_and_submitted() {
    let (mut connection, mut peer) = lean_connected();
    let sequence = consumed_head(&mut connection, &mut peer, 10);
    connection
        .enqueue_frame(&ShellFrameRecord::Close {
            transaction: tx(11),
            key: key(),
        })
        .unwrap();
    let held = submit(&mut connection, &mut peer);
    assert_eq!(held.data.len(), 24);
    assert_eq!(acks(&peer), [sequence]);
    assert!(peer.frame_writes.is_empty());
}

#[test]
fn lean_commit_after_a_paired_open_carries_the_covering_ack_itself() {
    let (mut connection, mut peer) = lean_connected();
    let open = connection.enqueue_frame(&open()).unwrap().first;
    let held = submit(&mut connection, &mut peer);
    peer.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let receipt = encode_shell_file_submitted_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::Submitted,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: peer.take_sequence(),
        },
        ShellFileSubmitted {
            submission_id: decode_shell_file_submit(&held.data).unwrap().submission_id,
            candidate_kind: ShellFileKind::FrameStreamOpen,
        },
        13,
    )
    .unwrap();
    peer.push(receipt);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(open) == Some(Custody::Submitted)
    })
    .unwrap();
    peer.transactions.clear();
    let before = acks(&peer);
    let submitted = connection.journal_progress().handled;
    // The Open's Submitted is handled but unacknowledged: the commit's word covers it.
    let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
    let commit = issued(&mut connection, &mut peer);
    assert!(
        word(&commit) >= submitted,
        "{} < {submitted}",
        word(&commit)
    );
    assert_eq!(acks(&peer), before);
    peer.answer_write(commit.tag, commit.data.len() as u32);
    drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap();
}

#[test]
fn submitted_naming_a_lean_commit_fails_closed() {
    let (mut connection, mut peer) = lean_connected();
    let ticket = connection.enqueue_frame(&commit_record()).unwrap().first;
    let held = issued(&mut connection, &mut peer);
    let receipt = receipt(&mut peer, &held);
    peer.push(receipt);
    let error = drive(&mut connection, &mut peer, |c, _| {
        c.custody(ticket) == Some(Custody::Submitted)
    })
    .unwrap_err();
    assert!(matches!(error, ShellClientError::Protocol(_)), "{error:?}");
}

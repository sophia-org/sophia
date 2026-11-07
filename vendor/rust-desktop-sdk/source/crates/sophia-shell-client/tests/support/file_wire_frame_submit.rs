use super::*;
use file_wire_peer::FrameSubmitOptions;

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

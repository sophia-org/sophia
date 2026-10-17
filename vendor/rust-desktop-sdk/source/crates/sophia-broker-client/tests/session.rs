use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_broker_client::{
    protocol::{Limits, ResponseRow, RowKind},
    session::Session,
    startup::Opened,
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};
fn n(b: &[u8]) -> u64 {
    b.iter()
        .enumerate()
        .fold(0, |v, (i, b)| v | (u64::from(*b) << (8 * i)))
}
fn send(s: &mut UnixStream, k: u8, t: &[u8], b: &[u8]) {
    let mut v = ((b.len() + 7) as u32).to_le_bytes().to_vec();
    v.push(k);
    v.extend(t);
    v.extend(b);
    if let Err(e) = s.write_all(&v) {
        assert!(matches!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
        ));
    }
}
fn record(kind: u16, size: usize, seq: u64) -> Vec<u8> {
    let mut v = vec![0; size];
    v[..4].copy_from_slice(&(size as u32).to_le_bytes());
    v[4] = 1;
    v[6..8].copy_from_slice(&kind.to_le_bytes());
    v[8] = 7;
    v[24..32].copy_from_slice(&seq.to_le_bytes());
    v
}
fn scenario(fault: u8) {
    let target = if fault == 0 { 2 } else { 1 };
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let job = thread::spawn(move || {
        let mut names = HashMap::new();
        let mut journal = Vec::new();
        let mut staged = Vec::new();
        let mut pending = None;
        let mut sent_requests = 0;
        let mut responses = 0;
        let mut clunks = 0;
        let mut submit_walks = 0;
        let mut acknowledged = 0;
        let mut refused_response: Option<(Vec<u8>, Vec<u8>)> = None;
        loop {
            let mut h = [0; 7];
            if let Err(e) = peer.read_exact(&mut h) {
                assert!(matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ));
                break;
            }
            let mut b = vec![0; n(&h[..4]) as usize - 7];
            peer.read_exact(&mut b).unwrap();
            match h[4] {
                100 => send(&mut peer, 101, &h[5..], &b),
                104 => {
                    let mut q = [0; 13];
                    q[0] = 128;
                    send(&mut peer, 105, &h[5..], &q);
                }
                110 => {
                    let name = String::from_utf8(b[12..].to_vec()).unwrap();
                    if name == "transaction" {
                        staged.clear();
                    }
                    if name == "submit" {
                        submit_walks += 1;
                        assert_eq!(submit_walks, 1, "submit is connection-owned");
                    }
                    names.insert(n(&b[4..8]), name);
                    let mut q = [0; 15];
                    q[0] = 1;
                    send(&mut peer, 111, &h[5..], &q);
                }
                12 => {
                    let mut q = [0; 17];
                    q[13] = if names[&n(&b[..4])] == "transaction" {
                        7
                    } else {
                        24
                    };
                    send(&mut peer, 13, &h[5..], &q);
                }
                116 => {
                    assert!(pending.is_none());
                    pending = Some((
                        h[5..].to_vec(),
                        n(&b[4..12]) as usize,
                        n(&b[12..16]) as usize,
                    ));
                }
                118 => {
                    let count = n(&b[12..16]) as usize;
                    assert_eq!(count, b.len() - 16);
                    match names[&n(&b[..4])].as_str() {
                        "transaction" => {
                            assert_eq!(n(&b[4..12]) as usize, staged.len());
                            staged.extend(&b[16..]);
                        }
                        "submit" => {
                            let kind = n(&staged[6..8]);
                            let id = n(&staged[16..24]);
                            assert_eq!(n(&b[24..32]), id);
                            if fault == 8 && kind == 257 {
                                if let Some((candidate, control)) = &refused_response {
                                    assert_eq!(&staged, candidate);
                                    assert_eq!(&b[16..], control);
                                } else {
                                    refused_response = Some((staged.clone(), b[16..].to_vec()));
                                    send(&mut peer, 7, &h[5..], &11u32.to_le_bytes());
                                    continue;
                                }
                            }
                            if fault == 7 && kind == 257 {
                                assert_eq!(staged.len(), 448);
                                return;
                            }
                            let seq = if kind == 256 { 1 } else { 4 + 2 * responses };
                            let mut receipt = record(18, 48, seq);
                            receipt[32..40].copy_from_slice(
                                &(if fault == 5 { id + 1 } else { id }).to_le_bytes(),
                            );
                            receipt[40..42].copy_from_slice(&(kind as u16).to_le_bytes());
                            if fault == 1 {
                                receipt[8] = 8;
                            }
                            journal.extend(receipt);
                            if kind == 256 {
                                assert_eq!(staged.len(), 40);
                                let mut v = record(16, 56, 2);
                                v[32] = 2;
                                v[36..40].copy_from_slice(&1024u32.to_le_bytes());
                                v[40] = 7;
                                v[48] = 128;
                                v[50] = 2;
                                if fault == 2 {
                                    v[36..40].copy_from_slice(&512u32.to_le_bytes());
                                }
                                if fault == 3 {
                                    v = record(17, 40, 2);
                                    v[32] = 1;
                                }
                                journal.extend(v);
                            } else {
                                assert_eq!(kind, 257);
                                assert_eq!(staged.len(), 448);
                                assert_eq!(n(&staged[32..40]), 8 + responses);
                                assert_eq!(id, 2 + responses);
                                if fault == 9 || fault == 11 {
                                    assert_eq!(n(&staged[40..42]), if fault == 9 { 5 } else { 1 });
                                    assert_eq!(n(&staged[42..44]), 2);
                                    assert_eq!(n(&staged[48..50]), 1);
                                    assert_eq!(n(&staged[248..250]), 2);
                                } else if fault == 12 || fault == 13 {
                                    assert_eq!(n(&staged[40..42]), 3);
                                    assert_eq!(n(&staged[48..50]), if fault == 12 { 5 } else { 2 });
                                    if fault == 13 {
                                        assert_eq!(n(&staged[68..70]), 2);
                                    }
                                } else if fault == 10 {
                                    assert_eq!(n(&staged[40..42]), 2);
                                    assert_eq!(staged[112], 1);
                                    assert_eq!(&staged[120..124], b"\0\n\xc3\xa9");
                                } else {
                                    assert_eq!(n(&staged[40..42]), 4);
                                    assert_eq!(n(&staged[48..50]), 3);
                                }
                                responses += 1;
                            }
                        }
                        "ack" => {
                            acknowledged = n(&b[24..32]);
                            if (fault == 0 || fault >= 6)
                                && n(&b[24..32]) >= 2 + 2 * sent_requests
                                && sent_requests < target
                            {
                                let mut v = record(32, 200, 3 + 2 * sent_requests);
                                v[32] = (8 + sent_requests) as u8;
                                v[40] = 4;
                                v[48] = 2;
                                v[52] = 3;
                                if fault == 9 || fault == 11 {
                                    v[40] = if fault == 9 { 5 } else { 1 };
                                    v[56] = 2;
                                }
                                if fault == 10 {
                                    v[40] = 2;
                                    v[56] = 2;
                                    v[59] = 1;
                                    v[60] = 4;
                                    v[64] = 8;
                                    v[72..76].copy_from_slice(b"\0\n\xc3\xa9");
                                }
                                if fault == 12 || fault == 13 {
                                    v[40] = 3;
                                    v[56] = 2;
                                }
                                journal.extend(v);
                                sent_requests += 1;
                            }
                        }
                        _ => panic!(),
                    }
                    let reply_count = if fault == 4 && names[&n(&b[..4])] == "ack" {
                        15
                    } else {
                        count as u32
                    };
                    send(&mut peer, 119, &h[5..], &reply_count.to_le_bytes());
                }
                120 => {
                    assert_eq!(names[&n(&b[..4])], "transaction");
                    assert!(acknowledged >= if responses == 0 { 1 } else { 2 + 2 * responses });
                    clunks += 1;
                    send(&mut peer, 121, &h[5..], &[]);
                }
                _ => panic!(),
            }
            if let Some((tag, offset, count)) = pending.take() {
                if offset < journal.len() {
                    let end = journal.len().min(offset + count);
                    let mut v = ((end - offset) as u32).to_le_bytes().to_vec();
                    v.extend(&journal[offset..end]);
                    send(&mut peer, 117, &tag, &v);
                } else {
                    pending = Some((tag, offset, count));
                }
            }
        }
        assert_eq!(responses, if fault == 0 || fault >= 8 { target } else { 0 });
        if fault == 0 || fault >= 8 {
            assert_eq!(clunks, 1 + target);
            assert_eq!(submit_walks, 1);
        }
    });
    let mut pipeline =
        Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let (tag, root) = pipeline.attach(b"", b"").unwrap();
    pipeline
        .wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap();
    let limits = Limits {
        max_surfaces: 1024,
        max_label_bytes: 128,
        journal_records: 16,
        journal_bytes: 4096,
        staging_bytes: 512,
        response_timeout_ms: 5000,
        ack_progress_timeout_ms: 2000,
        assembly_timeout_ms: 12000,
    };
    let mut s = Session::new(
        Opened {
            pipeline,
            root,
            epoch: 7,
            limits,
            now: 0,
        },
        2,
        2,
        1000,
    )
    .unwrap();
    assert!(!s.retry_ready());
    let end = Instant::now() + Duration::from_secs(2);
    if (1..=5).contains(&fault) {
        use sophia_broker_client::{Custody, Error as StateError, session::Error};
        let failure = loop {
            assert!(Instant::now() < end);
            if let Err(e) = s.dispatch(1, 4096) {
                break e;
            }
            let mut error = None;
            while s.event().is_some() {
                if let Err(e) = s.consume(1) {
                    error = Some(e);
                    break;
                }
            }
            if let Some(e) = error {
                break e;
            }
            match s.acknowledge(1) {
                Ok(()) | Err(Error::State(StateError::Busy)) => (),
                Err(e) => break e,
            }
            thread::yield_now();
        };
        assert_eq!(
            failure,
            if fault == 3 {
                Error::Refused
            } else {
                Error::State(StateError::Protocol)
            }
        );
        assert!(!s.ready() && s.fd().is_none());
        assert_eq!(
            s.custody(),
            if fault == 1 || fault == 5 {
                Custody::Unknown
            } else {
                Custody::Submitted
            }
        );
        job.join().unwrap();
        return;
    }
    let mut responded = 0;
    let mut receipts = 0;
    while receipts < target || s.custody() != sophia_broker_client::Custody::Submitted {
        assert!(Instant::now() < end);
        if let Err(e) = s.dispatch(1, 4096) {
            assert_eq!(fault, 7);
            assert!(matches!(e, sophia_broker_client::session::Error::Wire(_)));
            assert_eq!(s.custody(), sophia_broker_client::Custody::Unknown);
            assert!(s.fd().is_none() && !s.ready());
            job.join().unwrap();
            return;
        }
        while let Some(event) = s.event() {
            if matches!(
                event.event,
                sophia_broker_client::protocol::Event::Submitted {
                    candidate_kind: 257,
                    ..
                }
            ) {
                receipts += 1;
            }
            s.consume(1).unwrap();
        }
        let _ = s.acknowledge(1);
        if fault == 6
            && s.request().is_some()
            && s.transaction_ready()
            && s.deadline() == Some(5001)
        {
            assert!(matches!(
                s.dispatch(5001, 4096),
                Err(sophia_broker_client::session::Error::State(
                    sophia_broker_client::Error::Deadline
                ))
            ));
            assert!(s.fd().is_none() && !s.ready());
            job.join().unwrap();
            return;
        }
        if fault != 6 && responded < target && s.transaction_ready() && s.request().is_some() {
            let label = s.request().unwrap().label.map(str::to_owned);
            let mut rows = vec![ResponseRow {
                kind: RowKind::RetireSurface,
                surface_index: 2,
                surface_generation: 3,
                ..Default::default()
            }];
            if fault == 12 {
                rows = vec![ResponseRow {
                    kind: RowKind::NoChange,
                    ..Default::default()
                }];
            } else if fault >= 9 {
                let descriptor = ResponseRow {
                    kind: RowKind::EmitDescriptor,
                    surface_index: 2,
                    surface_generation: 3,
                    trust: 1,
                    attention: 2,
                    generation: 8,
                    grant_token: 12,
                    grant_revocation_epoch: 1,
                    grant_target_generation: 8,
                    ..Default::default()
                };
                if fault == 10 || fault == 13 {
                    if fault == 10 {
                        assert_eq!(label.as_deref(), Some("\0\né"));
                    }
                    rows = vec![ResponseRow {
                        label: label.as_deref(),
                        ..descriptor
                    }];
                } else {
                    rows = vec![
                        ResponseRow {
                            kind: RowKind::PublishRule,
                            surface_index: 2,
                            surface_generation: 3,
                            disclosure: 2,
                            trust: 1,
                            generation: 8,
                            ..Default::default()
                        },
                        descriptor,
                    ];
                }
            }
            // Borrowed rows must be independent of the session for mutation.
            if fault == 10 {
                rows[0].label = label.as_deref();
            }
            if (fault == 9 || fault == 11) && s.transaction_ready() {
                let mut bad = rows.clone();
                bad[1].grant_token = 0;
                assert_eq!(
                    s.respond(8, &bad, 1),
                    Err(sophia_broker_client::session::Error::State(
                        sophia_broker_client::Error::Invalid
                    ))
                );
                assert!(s.transaction_ready() && s.request().is_some());
            }
            if fault == 13 {
                let mut bad = rows.clone();
                bad[0].attention = 1;
                assert_eq!(
                    s.respond(8, &bad, 1),
                    Err(sophia_broker_client::session::Error::State(
                        sophia_broker_client::Error::Invalid
                    ))
                );
                assert!(s.transaction_ready() && s.request().is_some());
            }
            match s.respond(8 + responded, &rows, 1) {
                Ok(()) => {
                    responded += 1;
                    assert!(!s.retry_ready());
                }
                Err(sophia_broker_client::session::Error::State(
                    sophia_broker_client::Error::Busy,
                )) => (),
                Err(e) => panic!("{e:?}"),
            }
        }
        if fault == 8 && responded != 0 && s.retry_ready() {
            assert_eq!(s.custody(), sophia_broker_client::Custody::Queued);
            match s.retry(1) {
                Ok(())
                | Err(sophia_broker_client::session::Error::State(
                    sophia_broker_client::Error::Busy,
                )) => (),
                Err(e) => panic!("{e:?}"),
            }
            assert!(!s.retry_ready());
        }
        thread::yield_now();
    }
    // Drain the acknowledged response and assembly clunk without a timed wait.
    while !s.transaction_ready() {
        assert!(Instant::now() < end);
        s.dispatch(1, 4096).unwrap();
        thread::yield_now();
    }
    s.close();
    assert!(!s.retry_ready());
    job.join().unwrap();
}

#[test]
fn negotiation_request_response_and_ack_cleanup_share_one_connection() {
    scenario(0);
}
#[test]
fn stale_epoch_fails_without_inventing_a_receipt() {
    scenario(1);
}
#[test]
fn negotiated_limits_must_match_discovery() {
    scenario(2);
}
#[test]
fn refusal_remains_readable_until_terminal_ack() {
    scenario(3);
}

#[test]
fn short_ack_does_not_erase_known_submission() {
    scenario(4);
}
#[test]
fn wrong_receipt_id_does_not_establish_custody() {
    scenario(5);
}

#[test]
fn pending_request_expires_after_journal_ack() {
    scenario(6);
}
#[test]
fn connection_loss_after_response_submit_is_unknown() {
    scenario(7);
}

#[test]
fn explicit_response_retry_preserves_candidate_and_control() {
    scenario(8);
}

#[test]
fn disclosure_pair_rejects_bad_second_row_before_submission() {
    scenario(9);
}
#[test]
fn control_and_utf8_label_survives_journal_consumption() {
    scenario(10);
}
#[test]
fn admission_pair_rejects_bad_second_row_before_submission() {
    scenario(11);
}

#[test]
fn attention_response_can_leave_descriptor_unchanged() {
    scenario(12);
}

#[test]
fn attention_response_can_publish_a_descriptor() {
    scenario(13);
}

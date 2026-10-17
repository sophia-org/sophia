use sophia_9p_client::pipeline::PipelineLimits;
use sophia_wm_client::{
    protocol::{wm_files::*, *},
    submission::Custody,
    *,
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

fn frame(kind: u8, tag: u16, body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    b.push(kind);
    b.extend(tag.to_le_bytes());
    b.extend(body);
    b
}
fn qid(dir: bool) -> Vec<u8> {
    let mut b = vec![if dir { 128 } else { 0 }];
    b.extend([0; 4]);
    b.extend(1u64.to_le_bytes());
    b
}
fn record(kind: u16, seq: u64, body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 32) as u32).to_le_bytes().to_vec();
    b.extend(1u16.to_le_bytes());
    b.extend(kind.to_le_bytes());
    b.extend(7u64.to_le_bytes());
    b.extend(0u64.to_le_bytes());
    b.extend(seq.to_le_bytes());
    b.extend(body);
    b
}
fn limits() -> Vec<u8> {
    let mut body = 128u64.to_le_bytes().to_vec();
    body.extend(1_048_576u32.to_le_bytes());
    body.extend(1_048_576u32.to_le_bytes());
    body.extend(64u16.to_le_bytes());
    body.extend(32u16.to_le_bytes());
    body.extend(12_000u32.to_le_bytes());
    body.extend(4_000u32.to_le_bytes());
    body.extend([0; 4]);
    record(1, 0, &body)
}
fn read_reply(tag: u16, bytes: &[u8]) -> Vec<u8> {
    let mut body = (bytes.len() as u32).to_le_bytes().to_vec();
    body.extend(bytes);
    frame(117, tag, &body)
}
fn submitted(id: u64, kind: u16, sequence: u64) -> Vec<u8> {
    let mut body = id.to_le_bytes().to_vec();
    body.extend(kind.to_le_bytes());
    body.extend([0; 6]);
    record(17, sequence, &body)
}

fn cycle_bytes() -> Vec<u8> {
    let mut body = vec![];
    for n in [55u64, 56, 31, 9, 3] {
        body.extend(n.to_le_bytes());
    }
    body.extend(0u16.to_le_bytes());
    body.extend(1u16.to_le_bytes());
    body.extend([0; 4]);
    body.extend(1u64.to_le_bytes());
    record(22, 3, &body)
}
fn snapshot_bytes(mode: u8) -> Vec<u8> {
    // Pinned independent golden output row; construct the surrounding file
    // without using the snapshot encoder under test.
    let corpus = include_str!("../../../spec/golden/sophia-wm-v1.records");
    let hex = corpus
        .lines()
        .find_map(|s| s.strip_prefix("snapshot_output|"))
        .unwrap();
    let mut row: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    // This minimal scene has no surfaces, so clear the golden row's focus.
    row[16..24].fill(0);
    let mut body = vec![];
    for n in [55u64, 9, 1] {
        body.extend(n.to_le_bytes());
    }
    body.extend(1u16.to_le_bytes());
    body.extend([0; 6]);
    body.extend(1u16.to_le_bytes());
    body.extend([0; 2]);
    body.extend(1u32.to_le_bytes());
    body.extend((row.len() as u32).to_le_bytes());
    body.extend([0; 4]);
    body.extend(row);
    let mut bytes = record(2, 0, &body);
    match mode {
        2 => bytes[32] = 54,
        3 => bytes[40] = 8,
        4 => bytes[8] = 8,
        5 => {
            bytes.pop();
        }
        6 => bytes.push(0),
        11 => bytes[..4].copy_from_slice(&1_048_577u32.to_le_bytes()),
        _ => (),
    }
    bytes
}

struct Report {
    candidates: usize,
    retries: usize,
    acked: u64,
    snapshot_opens: usize,
}
// Literal API-1 bodies, independent of the codec under test. Semantic profile
// policy belongs to the caller; this checks ordered delivery and cumulative ACK.
fn application_events() -> Vec<u8> {
    let mut bytes = vec![];
    let mut seq = 3;
    for kind in 18..=20 {
        let mut body = 40u64.to_le_bytes().to_vec();
        body.extend(3u64.to_le_bytes());
        body.extend([7; 32]);
        bytes.extend(record(kind, seq, &body));
        seq += 1;
    }
    for kind in [21, 23, 24, 25] {
        for outcome in 1u16..=if kind == 25 { 3 } else { 5 } {
            let ids: &[u64] = match kind {
                21 | 24 => &[40, 3],
                23 => &[40, 3, 9],
                25 => &[40, 3, 1, 9, 1],
                _ => unreachable!(),
            };
            let mut body = vec![];
            for id in ids {
                body.extend(id.to_le_bytes());
            }
            body.extend(outcome.to_le_bytes());
            body.extend([0; 6]);
            bytes.extend(record(kind, seq, &body));
            seq += 1;
        }
    }
    bytes
}
#[derive(Clone, Copy, Default)]
enum PeerScenario {
    #[default]
    None,
    ShortAck,
    AckError(u32),
    RevokeAccepted,
    RevokeUncertain,
    ApplicationEvents,
    ProfileCompletions,
}
fn fixture(
    event_first: bool,
    retry: bool,
    eof: bool,
    snapshot: u8,
) -> (Session, thread::JoinHandle<Report>) {
    fixture_scenario(event_first, retry, eof, snapshot, PeerScenario::None)
}
fn fixture_scenario(
    event_first: bool,
    retry: bool,
    eof: bool,
    snapshot: u8,
    fault: PeerScenario,
) -> (Session, thread::JoinHandle<Report>) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    let caps = if matches!(
        fault,
        PeerScenario::ApplicationEvents | PeerScenario::ProfileCompletions
    ) {
        (1u64 << 23) - 1
    } else {
        128
    };
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let server = thread::spawn(move || {
        let mut files = HashMap::<u32, String>::new();
        let mut candidate = vec![];
        let mut event_read = None;
        let mut queued = vec![];
        let mut report = Report {
            candidates: 0,
            retries: 0,
            acked: 0,
            snapshot_opens: 0,
        };
        let mut old_submit = None;
        let mut offset = 0u64;
        loop {
            let mut h = [0; 7];
            match peer.read_exact(&mut h) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => panic!("peer: {e}"),
            }
            let mut b = vec![0; u32::from_le_bytes(h[..4].try_into().unwrap()) as usize - 7];
            peer.read_exact(&mut b).unwrap();
            let tag = u16::from_le_bytes(h[5..].try_into().unwrap());
            let fid = if b.len() >= 4 {
                u32::from_le_bytes(b[..4].try_into().unwrap())
            } else {
                0
            };
            match h[4] {
                100 => peer.write_all(&frame(101, tag, &b)).unwrap(),
                104 => {
                    files.insert(fid, "root".into());
                    peer.write_all(&frame(105, tag, &qid(true))).unwrap();
                }
                110 => {
                    assert_eq!(files[&fid], "root");
                    let newfid = u32::from_le_bytes(b[4..8].try_into().unwrap());
                    assert_eq!(u16::from_le_bytes(b[8..10].try_into().unwrap()), 1);
                    let name = String::from_utf8(b[12..].to_vec()).unwrap();
                    if name == "transaction" {
                        if report.candidates > 0 {
                            assert!(report.acked >= 2, "walk before Submitted ACK");
                        }
                        report.candidates += 1;
                        candidate.clear();
                    }
                    files.insert(newfid, name);
                    let mut body = 1u16.to_le_bytes().to_vec();
                    body.extend(qid(false));
                    peer.write_all(&frame(111, tag, &body)).unwrap();
                }
                12 => {
                    if files[&fid] == "snapshot" {
                        report.snapshot_opens += 1;
                        if (snapshot == 7 && report.snapshot_opens == 1) || snapshot == 10 {
                            peer.write_all(&frame(7, tag, &11u32.to_le_bytes()))
                                .unwrap();
                            continue;
                        }
                    }
                    let mut body = qid(false);
                    body.extend(if files[&fid] == "snapshot" { 7u32 } else { 0 }.to_le_bytes());
                    peer.write_all(&frame(13, tag, &body)).unwrap();
                }
                116 => {
                    if files[&fid] == "snapshot" && snapshot == 8 {
                        peer.write_all(&frame(7, tag, &11u32.to_le_bytes()))
                            .unwrap();
                        continue;
                    }
                    let at = u64::from_le_bytes(b[4..12].try_into().unwrap());
                    let count = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                    if files[&fid] == "events" {
                        assert!(event_read.is_none());
                        assert_eq!(at, offset);
                        if report.candidates == 2 && matches!(fault, PeerScenario::RevokeAccepted) {
                            peer.write_all(&frame(7, tag, &116u32.to_le_bytes()))
                                .unwrap();
                            break;
                        }
                        if queued.is_empty() {
                            event_read = Some((tag, count));
                        } else {
                            assert!(queued.len() <= count);
                            offset += queued.len() as u64;
                            peer.write_all(&read_reply(tag, &queued)).unwrap();
                            queued.clear();
                        }
                    } else {
                        let object = match files[&fid].as_str() {
                            "api" => {
                                b"sophia-wm-files version=1 output_transport=9p2000.L\n".to_vec()
                            }
                            "limits" => {
                                let mut value = limits();
                                value[32..40].copy_from_slice(&caps.to_le_bytes());
                                value
                            }
                            "snapshot" => snapshot_bytes(snapshot),
                            _ => panic!("unexpected read"),
                        };
                        let at = at as usize;
                        let end = object.len().min(at + count);
                        peer.write_all(&read_reply(tag, &object[at..end])).unwrap();
                    }
                }
                118 => {
                    let at = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
                    let data = &b[16..];
                    let response = frame(119, tag, &(data.len() as u32).to_le_bytes());
                    match files[&fid].as_str() {
                        "transaction" => {
                            assert_eq!(at, candidate.len());
                            candidate.extend(data);
                            peer.write_all(&response).unwrap();
                        }
                        "ack" => {
                            assert_eq!(data.len(), 16);
                            report.acked = u64::from_le_bytes(data[8..16].try_into().unwrap());
                            if report.acked >= 3 {
                                let refusal = match fault {
                                    PeerScenario::ShortAck => {
                                        Some(frame(119, tag, &15u32.to_le_bytes()))
                                    }
                                    PeerScenario::AckError(errno) => {
                                        Some(frame(7, tag, &errno.to_le_bytes()))
                                    }
                                    _ => None,
                                };
                                if let Some(refusal) = refusal {
                                    peer.write_all(&refusal).unwrap();
                                    break;
                                }
                            }
                            peer.write_all(&response).unwrap();
                        }
                        "submit" => {
                            assert_eq!(data.len(), 24);
                            let id = u64::from_le_bytes(data[8..16].try_into().unwrap());
                            let kind = u16::from_le_bytes(candidate[6..8].try_into().unwrap());
                            assert_eq!(data[8..16], candidate[16..24]);
                            assert_eq!(
                                u32::from_le_bytes(data[16..20].try_into().unwrap()) as usize,
                                candidate.len()
                            );
                            if ((kind == 263 && retry) || (kind == 256 && snapshot == 12))
                                && report.retries == 0
                            {
                                report.retries += 1;
                                old_submit = Some(data.to_vec());
                                peer.write_all(&frame(7, tag, &11u32.to_le_bytes()))
                                    .unwrap();
                                continue;
                            }
                            if let Some(old) = &old_submit {
                                assert_eq!(old, data);
                            }
                            if kind == 263 && matches!(fault, PeerScenario::RevokeUncertain) {
                                let (event_tag, _) =
                                    event_read.take().expect("outstanding event read");
                                // The submit has reached the peer, but no Submitted
                                // record establishes custody before revocation.
                                peer.write_all(&frame(7, event_tag, &116u32.to_le_bytes()))
                                    .unwrap();
                                break;
                            }
                            if kind == 256 {
                                queued.extend(record(16, 1, &caps.to_le_bytes()));
                                queued.extend(submitted(id, kind, 2));
                                if matches!(fault, PeerScenario::ApplicationEvents) {
                                    queued.extend(application_events());
                                }
                                if snapshot != 0 {
                                    queued.extend(cycle_bytes());
                                }
                            } else if matches!(fault, PeerScenario::ProfileCompletions) {
                                assert!((257..=259).contains(&kind));
                                assert_eq!(candidate.len(), 88);
                                assert_eq!(&candidate[32..40], &40u64.to_le_bytes());
                                assert_eq!(&candidate[40..48], &3u64.to_le_bytes());
                                assert_eq!(&candidate[48..80], &[7; 32]);
                                let outcome =
                                    u16::from_le_bytes(candidate[80..82].try_into().unwrap());
                                assert!((1..=3).contains(&outcome));
                                assert_eq!(&candidate[82..], &[0; 6]);
                                queued.extend(submitted(id, kind, id + 1));
                            } else {
                                assert_eq!(kind, 263);
                                queued.extend(submitted(id, kind, 3));
                                let mut body = candidate[32..48].to_vec();
                                body.extend(1u16.to_le_bytes());
                                body.extend([0; 6]);
                                queued.extend(record(24, 4, &body));
                            }
                            let mut batch = vec![];
                            if !event_first {
                                batch.extend(&response);
                            }
                            if let Some((event_tag, count)) = event_read.take() {
                                assert!(queued.len() <= count);
                                offset += queued.len() as u64;
                                batch.extend(read_reply(event_tag, &queued));
                                queued.clear();
                            }
                            if event_first {
                                batch.extend(&response);
                            }
                            peer.write_all(&batch).unwrap();
                            if eof && kind == 263 {
                                break;
                            }
                        }
                        _ => panic!("unexpected write"),
                    }
                }
                120 => {
                    if files[&fid] == "snapshot" && snapshot == 9 {
                        files.remove(&fid);
                        peer.write_all(&frame(7, tag, &5u32.to_le_bytes())).unwrap();
                        continue;
                    }
                    files.remove(&fid);
                    peer.write_all(&frame(121, tag, &[])).unwrap();
                }
                _ => panic!("unexpected opcode {}", h[4]),
            }
        }
        report
    });
    let mut opening = Connecting::direct(
        client,
        PipelineLimits::default(),
        WmFileNegotiationOffer {
            required_capabilities: caps,
            optional_capabilities: 0,
        },
        10,
        1_000_000,
    )
    .unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    let mut startup_now = 10;
    let mut saw_retry_wake = false;
    while !opening.ready() {
        assert!(Instant::now() < end);
        opening.dispatch(startup_now, 65536).unwrap();
        if let Some(wake) = opening.wake_deadline()
            && snapshot == 12
            && wake < 1000
        {
            saw_retry_wake = true;
            startup_now = startup_now.max(wake);
        }
        thread::yield_now();
    }
    assert!(opening.io_interest().unwrap().immediate);
    assert_eq!(opening.state(), SessionState::Ready);
    assert_eq!(opening.epoch(), Some(7));
    assert_eq!(opening.capabilities(), Some(caps));
    assert_eq!(opening.limits().unwrap().capability_ceiling, caps);
    assert_eq!(opening.custody(1), Some(Custody::Submitted));
    assert!(opening.error().is_none());
    if snapshot == 12 {
        assert!(saw_retry_wake);
    }
    (opening.take_session().unwrap(), server)
}

fn drive_until(s: &mut Session, now: &mut u64, done: impl Fn(&Session) -> bool) {
    let end = Instant::now() + Duration::from_secs(2);
    while !done(s) {
        assert!(Instant::now() < end);
        *now += 1;
        s.dispatch(*now, 65536).unwrap();
        thread::yield_now();
    }
}

#[test]
fn negotiated_session_submits_retries_and_drains_ordered_events_over_real_pipeline() {
    for (event_first, retry) in [(true, false), (false, false), (true, true)] {
        let (mut s, peer) = fixture(event_first, retry, false, 0);
        let mut now = 10;
        drive_until(&mut s, &mut now, |s| {
            s.state() == SessionState::Ready && s.acked() >= 2
        });
        assert_eq!(s.custody(1), Some(Custody::Submitted));
        let candidate = Candidate::SessionOperation(WmFileSessionOperation {
            transaction: TransactionId::from_raw(19),
            request: PolicySessionOperationRequest {
                connection_epoch: 7,
                request_id: 31,
                operation: 1,
                target: None,
            },
        });
        let id = loop {
            match s.submit(&candidate, now + 100_000) {
                Ok(id) => break id,
                Err(SessionError::Submission(submission::Error::Busy)) => {
                    now += 1;
                    s.dispatch(now, 65536).unwrap();
                }
                other => panic!("admission: {other:?}"),
            }
        };
        drive_until(&mut s, &mut now, |s| s.event().is_some());
        assert_eq!(s.custody(id), Some(Custody::Submitted));
        assert_eq!(s.event().unwrap().0.sequence, 4);
        assert!(s.obligations().event_pending);
        assert_eq!(s.obligations().submitted_sequence, 3);
        assert_eq!(s.consumed(), 3);
        s.consume().unwrap();
        drive_until(&mut s, &mut now, |s| s.acked() == 4);
        s.close();
        let obligations = s.obligations();
        assert!(
            !obligations.event_pending
                && !obligations.snapshot_pending
                && !obligations.snapshot_ready
        );
        assert_eq!(obligations.deadline, None);
        assert_eq!(obligations.retry_at, None);
        assert_eq!(obligations.submitted_sequence, 3);
        assert_eq!(s.custody(id), Some(Custody::Submitted));
        let report = peer.join().unwrap();
        assert_eq!(report.candidates, 2);
        assert_eq!(report.retries, usize::from(retry));
        assert_eq!(report.acked, 4);
    }
}

#[test]
fn every_profile_completion_preserves_echo_identity_without_spending_domain_ids() {
    let (mut s, peer) = fixture_scenario(true, false, false, 0, PeerScenario::ProfileCompletions);
    let mut now = 10;
    drive_until(&mut s, &mut now, |s| {
        s.acked() == 2 && s.obligations().deadline.is_none()
    });
    for kind in 0..3 {
        for outcome in [
            PolicyProfileOutcome::Accepted,
            PolicyProfileOutcome::RejectedIdentity,
            PolicyProfileOutcome::RejectedState,
        ] {
            let value = PolicyProfileCompletion {
                transaction: TransactionId::from_raw(40),
                identity: PolicyProfileIdentity::new(7, 3, [7; 32]).unwrap(),
                outcome,
            };
            let candidate = match kind {
                0 => Candidate::ProfilePrepared(value),
                1 => Candidate::ProfileActive(value),
                _ => Candidate::ProfileRolledBack(value),
            };
            let ticket = s.submit(&candidate, now + 100_000).unwrap();
            drive_until(&mut s, &mut now, |s| {
                s.custody(ticket) == Some(Custody::Submitted)
                    && s.acked() == ticket + 1
                    && s.obligations().deadline.is_none()
            });
            assert_eq!(s.state(), SessionState::Ready);
        }
    }
    assert_eq!(s.next_transaction().unwrap(), 1);
    s.close();
    let report = peer.join().unwrap();
    assert_eq!(report.candidates, 10);
    assert_eq!(report.acked, 11);
}

#[test]
fn profile_commands_and_all_outcomes_remain_ordered_application_events() {
    use sophia_wm_client::journal::Event;
    let (mut s, peer) = fixture_scenario(true, false, false, 0, PeerScenario::ApplicationEvents);
    let mut now = 10;
    for seq in 3..=23 {
        drive_until(&mut s, &mut now, |s| s.event().is_some());
        let (header, event) = s.event().unwrap();
        assert_eq!(header.sequence, seq);
        let event = event.clone();
        match &event {
            Event::ProfilePrepare(c) | Event::ProfileActivate(c) | Event::ProfileRollback(c) => {
                assert_eq!(header.kind as u16, 18 + (seq - 3) as u16);
                assert_eq!(c.transaction.raw(), 40);
                assert_eq!(c.identity.profile_generation, 3);
                assert_eq!(c.identity.profile_digest, [7; 32]);
            }
            Event::ConfigurationOutcome(c) => {
                assert_eq!(policy_projection_outcome_code(c.outcome), (seq - 5) as u16)
            }
            Event::ProjectionOutcome(c) => {
                assert_eq!(policy_projection_outcome_code(c.outcome), (seq - 10) as u16)
            }
            Event::SessionOperationOutcome(c) => assert_eq!(
                policy_projection_outcome_code(c.outcome.outcome),
                (seq - 15) as u16
            ),
            Event::PresentationReceipt(c) => {
                assert_eq!(c.receipt.outcome as u16, (seq - 20) as u16)
            }
            other => panic!("unexpected event {other:?}"),
        }
        assert!(s.acked() < seq);
        s.dispatch(now, 65536).unwrap();
        assert_eq!(
            s.event().unwrap().1,
            &event,
            "dispatch must not replace a held event"
        );
        s.consume().unwrap();
    }
    drive_until(&mut s, &mut now, |s| {
        s.acked() == 23 && s.obligations().deadline.is_none()
    });
    assert!(s.event().is_none());
    assert_eq!(
        s.state(),
        SessionState::Ready,
        "an outcome is not a transport failure"
    );
    assert_eq!(s.custody(1), Some(Custody::Submitted));
    s.close();
    assert_eq!(peer.join().unwrap().acked, 23);
}

#[test]
fn ack_faults_and_revocation_preserve_exact_custody() {
    for fault in [
        PeerScenario::ShortAck,
        PeerScenario::AckError(22),
        PeerScenario::AckError(116),
        PeerScenario::RevokeAccepted,
        PeerScenario::RevokeUncertain,
    ] {
        let (mut s, peer) = fixture_scenario(true, false, false, 0, fault);
        let mut now = 10;
        drive_until(&mut s, &mut now, |s| {
            s.acked() == 2 && s.obligations().deadline.is_none()
        });
        let ticket = s
            .submit(
                &Candidate::SessionOperation(WmFileSessionOperation {
                    transaction: TransactionId::from_raw(19),
                    request: PolicySessionOperationRequest {
                        connection_epoch: 7,
                        request_id: 31,
                        operation: 1,
                        target: None,
                    },
                }),
                now + 100_000,
            )
            .unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while s.error().is_none() {
            assert!(Instant::now() < end);
            now += 1;
            let _ = s.dispatch(now, 65536);
            thread::yield_now();
        }
        let errno = match fault {
            PeerScenario::AckError(errno) => Some(errno),
            PeerScenario::RevokeAccepted | PeerScenario::RevokeUncertain => Some(116),
            _ => None,
        };
        let expected = if errno == Some(116) {
            SessionState::Stale
        } else {
            SessionState::Failed
        };
        let custody = if matches!(fault, PeerScenario::RevokeUncertain) {
            Custody::UnknownDisconnected
        } else {
            Custody::Submitted
        };
        assert_eq!(s.state(), expected);
        assert_eq!(s.remote_error(), errno);
        assert_eq!(s.custody(ticket), Some(custody));
        assert_eq!(s.custody(1), Some(Custody::Submitted));
        assert!(s.event().is_none() && s.snapshot_result().is_none() && s.fd().is_none());
        assert_eq!(s.wake_deadline(), None);
        let error = s.error().unwrap().clone();
        assert_eq!(s.dispatch(now, 65536), Err(error.clone()));
        assert_eq!(s.consume(), Err(error));
        s.close();
        assert_eq!(s.state(), expected);
        assert_eq!(s.custody(ticket), Some(custody));
        assert_eq!(peer.join().unwrap().candidates, 2);
    }
}

#[test]
fn final_journal_bytes_preserve_custody_on_eof() {
    let (mut s, peer) = fixture(false, false, true, 0);
    let mut now = 10;
    drive_until(&mut s, &mut now, |s| {
        s.state() == SessionState::Ready && s.acked() >= 2
    });
    // Allow the negotiation transaction's clunk to settle as well.
    let candidate = Candidate::SessionOperation(WmFileSessionOperation {
        transaction: TransactionId::from_raw(19),
        request: PolicySessionOperationRequest {
            connection_epoch: 7,
            request_id: 31,
            operation: 1,
            target: None,
        },
    });
    let id = loop {
        match s.submit(&candidate, now + 100_000) {
            Ok(id) => break id,
            Err(SessionError::Submission(submission::Error::Busy)) => {
                now += 1;
                s.dispatch(now, 65536).unwrap();
            }
            other => panic!("{other:?}"),
        }
    };
    let end = Instant::now() + Duration::from_secs(2);
    while s.error().is_none() {
        assert!(Instant::now() < end);
        now += 1;
        let _ = s.dispatch(now, 65536);
        thread::yield_now();
    }
    assert_eq!(s.custody(id), Some(Custody::Submitted));
    assert!(s.fd().is_none());
    peer.join().unwrap();
}

#[test]
fn snapshot_freezes_cycle_identity_and_requires_verified_eof_and_clunk() {
    for mode in [1, 7] {
        let (mut s, peer) = fixture(true, false, false, mode);
        let mut now = 10;
        drive_until(&mut s, &mut now, |s| s.event().is_some());
        s.request_snapshot(now + 100_000).unwrap();
        assert!(s.obligations().snapshot_pending);
        assert!(!s.obligations().snapshot_ready);
        assert_eq!(
            s.request_snapshot(now + 100_000),
            Err(SessionError::SnapshotBusy)
        );
        // Fetch is bound before consumption; later journal work cannot retarget it.
        s.consume().unwrap();
        drive_until(&mut s, &mut now, |s| s.snapshot_result().is_some());
        let value = s.snapshot_result().unwrap();
        assert!(!s.obligations().snapshot_pending);
        assert!(s.obligations().snapshot_ready);
        assert_eq!(value.transaction.raw(), 55);
        assert_eq!(value.snapshot.scene.generation, 9);
        assert_eq!(value.snapshot.scene.outputs.len(), 1);
        s.release_snapshot().unwrap();
        assert!(s.snapshot_result().is_none());
        assert_eq!(s.release_snapshot(), Err(SessionError::NotReady));
        assert!(s.request_snapshot(now + 100_000).is_err());
        s.close();
        let report = peer.join().unwrap();
        assert_eq!(report.snapshot_opens, if mode == 7 { 2 } else { 1 });
    }
}

#[test]
fn connecting_exposes_negotiation_retry_wake_before_bootstrap_deadline() {
    let (mut session, peer) = fixture(true, false, false, 12);
    assert_eq!(session.state(), SessionState::Ready);
    let mut now = 14;
    session.consume().unwrap();
    drive_until(&mut session, &mut now, |s| {
        s.acked() == 3 && s.obligations().deadline.is_none()
    });
    session.close();
    assert_eq!(peer.join().unwrap().retries, 1);
}

#[test]
fn snapshot_rejects_identity_length_and_eof_mismatches() {
    for mode in [2, 3, 4, 5, 6, 8, 9, 11] {
        let (mut s, peer) = fixture(true, false, false, mode);
        let mut now = 10;
        drive_until(&mut s, &mut now, |s| s.event().is_some());
        s.request_snapshot(now + 100_000).unwrap();
        s.consume().unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while s.error().is_none() {
            assert!(Instant::now() < end);
            now += 1;
            let _ = s.dispatch(now, 65536);
            thread::yield_now();
        }
        assert!(s.snapshot_result().is_none());
        assert!(s.fd().is_none());
        peer.join().unwrap();
    }
}

#[test]
fn snapshot_open_retries_do_not_extend_the_original_deadline() {
    let (mut s, peer) = fixture(true, false, false, 10);
    let mut now = 10;
    drive_until(&mut s, &mut now, |s| s.event().is_some());
    let deadline = now + 100;
    s.request_snapshot(deadline).unwrap();
    s.consume().unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    while s.error().is_none() {
        assert!(Instant::now() < end);
        now += 1;
        let _ = s.dispatch(now, 65536);
        thread::yield_now();
    }
    assert_eq!(s.error(), Some(&SessionError::Deadline));
    assert_eq!(now, deadline);
    assert!(s.snapshot_result().is_none());
    peer.join().unwrap();
}

use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_lock_client::{
    Candidate, Session, SessionError, SessionState, journal::Event, protocol::lock_files::*,
    startup::Opening, submission::Custody,
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

fn get(b: &[u8], at: usize, n: usize) -> u64 {
    b[at..at + n]
        .iter()
        .enumerate()
        .fold(0, |v, (i, b)| v | (u64::from(*b) << (i * 8)))
}
fn send(s: &mut UnixStream, kind: u8, tag: u16, body: &[u8]) {
    let mut b = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    b.push(kind);
    b.extend(tag.to_le_bytes());
    b.extend(body);
    if let Err(e) = s.write_all(&b) {
        assert!(
            matches!(
                e.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            ),
            "peer write: {e}"
        );
    }
}
fn qid(dir: bool) -> Vec<u8> {
    let mut b = vec![if dir { 128 } else { 0 }];
    b.extend([0; 4]);
    b.extend(90u64.to_le_bytes());
    b
}
fn read_reply(s: &mut UnixStream, tag: u16, bytes: &[u8]) {
    let mut b = (bytes.len() as u32).to_le_bytes().to_vec();
    b.extend(bytes);
    send(s, 117, tag, &b);
}
fn record(kind: u16, seq: u64, body: &[u8]) -> Vec<u8> {
    let mut b = ((32 + body.len()) as u32).to_le_bytes().to_vec();
    b.extend([1, 0]);
    b.extend(kind.to_le_bytes());
    b.extend(7u64.to_le_bytes());
    b.extend([0; 8]);
    b.extend(seq.to_le_bytes());
    b.extend(body);
    b
}
fn limits() -> Vec<u8> {
    let mut b = vec![1, 0, 16, 0, 4, 0, 8, 0];
    b.extend(16384u32.to_le_bytes());
    b.extend(16384u32.to_le_bytes());
    b.extend((1u64 << 30).to_le_bytes());
    b.extend([32, 0, 1, 0]);
    for n in [64u32, 16384, 12000, 2000, 0] {
        b.extend(n.to_le_bytes());
    }
    record(1, 0, &b)
}
fn object() -> Vec<u8> {
    let mut b = vec![0; 32];
    b[8] = 3;
    b[16] = 1;
    record(2, 0, &b)
}
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Normal,
    Short,
    Reject,
    Refuse,
    AckShort,
    AckError,
    AckStale,
    Disconnect,
    Retry,
    NegotiationRetry,
}
#[derive(Default)]
struct Report {
    acked: u64,
    ended: usize,
    cancelled: usize,
    controls_while_held: bool,
    writes: usize,
    retries: usize,
}
fn peer_fixture(mode: Mode) -> (UnixStream, thread::JoinHandle<Report>) {
    let (client, mut server) = UnixStream::pair().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(4)))
        .unwrap();
    let peer = thread::spawn(move || {
        let mut files = HashMap::<u32, String>::new();
        let mut report = Report::default();
        let mut tx = vec![];
        let mut journal = vec![];
        let mut seq = 0u64;
        let mut event_read = None;
        let mut held: Vec<(u16, u32)> = vec![];
        let mut entry_seq = 0;
        let mut holding = true;
        let mut begin = vec![];
        let mut accepted_seq = 0;
        let mut versions = 0;
        let mut attaches = 0;
        let mut retried = false;
        loop {
            let mut h = [0; 7];
            match server.read_exact(&mut h) {
                Ok(()) => (),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                    ) =>
                {
                    break;
                }
                Err(e) => panic!("peer read {mode:?}: {e}"),
            }
            let tag = get(&h, 5, 2) as u16;
            let mut b = vec![0; get(&h, 0, 4) as usize - 7];
            server.read_exact(&mut b).unwrap();
            let fid = if b.len() >= 4 {
                get(&b, 0, 4) as u32
            } else {
                0
            };
            match h[4] {
                100 => {
                    versions += 1;
                    assert_eq!(versions, 1);
                    send(&mut server, 101, tag, &b);
                }
                104 => {
                    attaches += 1;
                    assert_eq!(attaches, 1);
                    files.insert(fid, "root".into());
                    send(&mut server, 105, tag, &qid(true));
                }
                110 => {
                    let n = get(&b, 8, 2) as usize;
                    let len = get(&b, 10, 2) as usize;
                    let name = String::from_utf8(b[12..12 + len].to_vec()).unwrap();
                    files.insert(get(&b, 4, 4) as u32, name);
                    let mut out = (n as u16).to_le_bytes().to_vec();
                    for i in 0..n {
                        out.extend(qid(i + 1 < n));
                    }
                    send(&mut server, 111, tag, &out);
                }
                12 => {
                    let name = &files[&fid];
                    if name == "transaction" {
                        assert!(report.acked >= accepted_seq);
                        tx.clear();
                    }
                    let mut out = qid(false);
                    out.extend(if name == "upload" { 4u32 } else { 0 }.to_le_bytes());
                    send(&mut server, 13, tag, &out);
                }
                116 => {
                    let offset = get(&b, 4, 8) as usize;
                    let count = get(&b, 12, 4) as usize;
                    let data = match files[&fid].as_str() {
                        "api" => b"sophia-lock-files version=1 epoch=7\n".to_vec(),
                        "limits" => limits(),
                        "lock" => object(),
                        "events" => {
                            event_read = Some((tag, offset, count));
                            vec![]
                        }
                        _ => panic!(),
                    };
                    if files[&fid] != "events" {
                        read_reply(
                            &mut server,
                            tag,
                            &data[offset..data.len().min(offset + count)],
                        );
                    }
                }
                118 => {
                    let offset = get(&b, 4, 8) as usize;
                    let n = get(&b, 12, 4) as usize;
                    let data = &b[16..];
                    assert_eq!(data.len(), n);
                    match files[&fid].as_str() {
                        "transaction" => {
                            assert_eq!(offset, tx.len());
                            tx.extend(data);
                            send(&mut server, 119, tag, &(n as u32).to_le_bytes());
                        }
                        "ack" => {
                            assert_eq!(get(data, 0, 8), 7);
                            report.acked = get(data, 8, 8);
                            if mode == Mode::AckShort {
                                send(&mut server, 119, tag, &15u32.to_le_bytes());
                                continue;
                            }
                            if matches!(mode, Mode::AckError | Mode::AckStale) {
                                send(
                                    &mut server,
                                    7,
                                    tag,
                                    &if mode == Mode::AckStale {
                                        116u32
                                    } else {
                                        22u32
                                    }
                                    .to_le_bytes(),
                                );
                                continue;
                            }
                            send(&mut server, 119, tag, &16u32.to_le_bytes());
                            if entry_seq != 0 && report.acked >= entry_seq && holding {
                                report.controls_while_held = true;
                                holding = false;
                                if mode == Mode::Short {
                                    let (t, _) = held.remove(0);
                                    send(&mut server, 119, t, &2u32.to_le_bytes());
                                }
                                for (t, count) in held.drain(..).rev() {
                                    send(&mut server, 119, t, &count.to_le_bytes());
                                }
                            }
                        }
                        "upload" => {
                            report.writes += 1;
                            assert_eq!(
                                data,
                                &(offset as u8..offset as u8 + n as u8).collect::<Vec<_>>()
                            );
                            if holding {
                                held.push((tag, n as u32));
                                if held.len() == 8 {
                                    if mode == Mode::Reject {
                                        seq += 1;
                                        journal.extend(resource_event(seq, &begin, 3));
                                    }
                                    seq += 1;
                                    entry_seq = seq;
                                    journal.extend(record(
                                        40,
                                        seq,
                                        &[5, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
                                    ));
                                }
                            } else {
                                send(&mut server, 119, tag, &(n as u32).to_le_bytes());
                            }
                        }
                        "submit" => {
                            assert_eq!(n, 24);
                            assert_eq!(get(data, 16, 4) as usize, tx.len());
                            let id = get(data, 8, 8);
                            let kind = get(&tx, 6, 2) as u16;
                            if mode == Mode::Disconnect && id > 1 {
                                break;
                            }
                            if ((mode == Mode::Retry && id > 1) || mode == Mode::NegotiationRetry)
                                && !retried
                            {
                                retried = true;
                                report.retries += 1;
                                send(&mut server, 7, tag, &11u32.to_le_bytes());
                                continue;
                            }
                            if mode == Mode::Refuse && kind == 256 {
                                seq += 1;
                                journal.extend(record(17, seq, &[1, 0, 0, 0, 0, 0, 0, 0]));
                            } else {
                                let mut body = id.to_le_bytes().to_vec();
                                body.extend(kind.to_le_bytes());
                                body.extend([0; 6]);
                                seq += 1;
                                accepted_seq = seq;
                                journal.extend(record(18, seq, &body));
                                match kind {
                                    256 => {
                                        seq += 1;
                                        journal.extend(record(
                                            16,
                                            seq,
                                            &[1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
                                        ));
                                        let mut body = vec![2, 0, 0, 0, 0, 0, 0, 0];
                                        body.extend(1u64.to_le_bytes());
                                        body.extend(90u64.to_le_bytes());
                                        seq += 1;
                                        journal.extend(record(19, seq, &body));
                                    }
                                    258 => {
                                        begin = tx[32..].to_vec();
                                        seq += 1;
                                        journal.extend(resource_event(seq, &begin, 1));
                                    }
                                    259 | 260 => {
                                        if kind == 259 {
                                            report.ended += 1;
                                            assert_eq!(get(&tx, 56, 8), 128);
                                        } else {
                                            report.cancelled += 1;
                                        }
                                        assert_eq!(get(&tx, 32, 8), 30);
                                        seq += 1;
                                        journal.extend(resource_event(
                                            seq,
                                            &begin,
                                            if kind == 259 { 2 } else { 4 },
                                        ));
                                    }
                                    261 => (),
                                    _ => panic!("unexpected candidate {kind}"),
                                }
                            }
                            send(&mut server, 119, tag, &24u32.to_le_bytes());
                        }
                        name => panic!("write {name}"),
                    }
                }
                120 => {
                    files.remove(&fid);
                    send(&mut server, 121, tag, &[]);
                }
                other => panic!("request {other}"),
            }
            if let Some((tag, offset, count)) = event_read
                && offset < journal.len()
            {
                read_reply(
                    &mut server,
                    tag,
                    &journal[offset..journal.len().min(offset + count)],
                );
                event_read = None;
            }
        }
        report
    });
    (client, peer)
}
fn fixture(mode: Mode) -> (Session, thread::JoinHandle<Report>) {
    let (client, peer) = peer_fixture(mode);
    let bounds = PipelineLimits {
        max_outstanding: 16,
        ..PipelineLimits::default()
    };
    let wire = Pipeline::over(client, bounds, Duration::from_secs(2)).unwrap();
    let offer = LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 1,
        requested_capabilities: 1,
        chords: vec![],
    };
    let mut opening = Opening::direct(wire, offer, 10, 1000).unwrap();
    let end = Instant::now() + Duration::from_secs(3);
    while !opening.is_opened() {
        assert!(Instant::now() < end);
        opening.dispatch(11, 65536).unwrap();
        thread::yield_now();
    }
    (
        Session::from_opened(opening.into_opened().unwrap()).unwrap(),
        peer,
    )
}
fn resource_event(seq: u64, begin: &[u8], state: u16) -> Vec<u8> {
    let mut b = begin[..24].to_vec();
    b.extend(state.to_le_bytes());
    b.extend([0; 6]);
    b.extend(128u64.to_le_bytes());
    record(33, seq, &b)
}
fn until(s: &mut Session, mut done: impl FnMut(&mut Session) -> bool) {
    let end = Instant::now() + Duration::from_secs(3);
    while !done(s) {
        assert!(
            Instant::now() < end,
            "state {:?}, obligations {:?}",
            s.state(),
            s.obligations()
        );
        s.dispatch(12, 65536).unwrap();
        thread::yield_now();
    }
}
fn ready(s: &mut Session) {
    until(s, |s| {
        s.state() == SessionState::Ready && s.wake_deadline().is_none() && s.event().is_some()
    });
    assert_eq!(s.lock().unwrap().1.phase, LockPhase::Unlocked);
    assert!(matches!(s.event().unwrap().1, Event::ObjectPublished(_)));
    s.consume().unwrap();
}
fn begin() -> LockResourceBegin {
    LockResourceBegin {
        transaction: 30,
        resource: LockResourceId {
            id: 21,
            generation: 2,
        },
        width_px: 16,
        height_px: 2,
        slot: 0,
    }
}
#[test]
fn retired_byte_candidates_do_not_spend_submission_identity() {
    let (mut s, peer) = fixture(Mode::Normal);
    ready(&mut s);
    let before = s.latest_submission();
    assert!(s.submit(&Candidate::ResourceBegin(begin())).is_err());
    assert!(
        s.submit(&Candidate::ResourceEnd(LockResourceStep {
            transaction: 30,
            resource: begin().resource,
            total_bytes: Some(128),
        }))
        .is_err()
    );
    assert_eq!(s.latest_submission(), before);
    assert_eq!(s.state(), SessionState::Ready);
    drop(s);
    let report = peer.join().unwrap();
    assert_eq!(report.ended, 0);
    assert_eq!(report.cancelled, 0);
}

#[test]
fn terminal_ack_and_refusal_preserve_custody_and_settle_refusal_ack() {
    for mode in [Mode::AckShort, Mode::AckError, Mode::AckStale, Mode::Refuse] {
        let (mut s, peer) = fixture(mode);
        let end = Instant::now() + Duration::from_secs(3);
        while s.error().is_none() {
            assert!(Instant::now() < end);
            let _ = s.dispatch(12, 65536);
            thread::yield_now();
        }
        assert!(s.fd().is_none());
        if mode == Mode::Refuse {
            assert_eq!(s.state(), SessionState::Refused);
            assert!(s.refusal().is_some());
        } else {
            assert_eq!(
                s.state(),
                if mode == Mode::AckStale {
                    SessionState::Stale
                } else {
                    SessionState::Failed
                }
            );
            assert_eq!(s.latest_submission(), Some((1, Custody::Submitted)));
        }
        let report = peer.join().unwrap();
        assert!(report.acked >= 1);
    }
}
#[test]
fn explicit_retry_and_disconnect_do_not_invent_custody() {
    for mode in [Mode::Retry, Mode::Disconnect] {
        let (mut s, peer) = fixture(mode);
        ready(&mut s);
        let candidate = Candidate::ResourceRetire(LockResourceStep {
            transaction: 40,
            resource: begin().resource,
            total_bytes: None,
        });
        let id = s.submit(&candidate).unwrap();
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            assert!(Instant::now() < end);
            let result = s.dispatch(12, 65536);
            if mode == Mode::Disconnect && result.is_err() {
                break;
            }
            result.unwrap();
            if s.obligations().waiting_for_retry {
                s.retry_submission().unwrap();
            }
            if s.latest_submission() == Some((id, Custody::Submitted)) {
                break;
            }
            thread::yield_now();
        }
        if mode == Mode::Disconnect {
            assert_eq!(
                s.latest_submission(),
                Some((id, Custody::UnknownDisconnected))
            );
        }
        drop(s);
        let report = peer.join().unwrap();
        assert_eq!(report.retries, usize::from(mode == Mode::Retry));
    }
}
#[test]
fn clock_regression_and_zero_budget_do_not_send_and_private_kinds_are_rejected() {
    let (mut s, peer) = fixture(Mode::Normal);
    assert_eq!(s.dispatch(9, 4096), Err(SessionError::ClockRegression));
    s.dispatch(12, 0).unwrap();
    assert_eq!(s.latest_submission(), Some((1, Custody::Staged)));
    ready(&mut s);
    assert!(s.submit(&Candidate::ResourceBegin(begin())).is_err());
    s.close();
    assert_eq!(s.latest_submission(), Some((1, Custody::Submitted)));
    assert!(s.fd().is_none());
    peer.join().unwrap();
}

#[test]
fn connecting_direct_attached_explicit_retry_and_refusal_keep_one_connection() {
    use sophia_lock_client::Connecting;
    for attached in [false, true] {
        for mode in [Mode::Normal, Mode::NegotiationRetry, Mode::Refuse] {
            let (stream, peer) = peer_fixture(mode);
            let offer = LockNegotiate {
                minimum_revision: 1,
                maximum_revision: 1,
                requested_capabilities: 1,
                chords: vec![],
            };
            let limits = PipelineLimits {
                max_outstanding: 6,
                ..PipelineLimits::default()
            };
            let mut c = if attached {
                let mut wire = Pipeline::over(stream, limits, Duration::from_secs(2)).unwrap();
                let (tag, root) = wire.attach(b"lock", b"lock-provider").unwrap();
                wire.wait(tag, Instant::now() + Duration::from_secs(2))
                    .unwrap();
                Connecting::attached(wire, root, offer, 10, 1000).unwrap()
            } else {
                Connecting::direct(stream, limits, offer, 10, 1000).unwrap()
            };
            assert_eq!(c.dispatch(9, 1000), Err(SessionError::ClockRegression));
            c.dispatch(11, 0).unwrap();
            assert!(c.take_session().is_none());
            let end = Instant::now() + Duration::from_secs(3);
            while !c.ready() && c.error().is_none() {
                assert!(Instant::now() < end);
                let _ = c.dispatch(12, 65536);
                if c.obligations().waiting_for_retry {
                    c.retry_submission().unwrap();
                }
                thread::yield_now();
            }
            assert_eq!(c.epoch(), Some(7));
            if mode == Mode::Refuse {
                assert_eq!(c.state(), SessionState::Refused);
                assert!(c.refusal().is_some());
                assert!(c.fd().is_none());
                assert!(c.take_session().is_none());
            } else {
                assert!(c.ready());
                assert!(c.welcome().is_some());
                assert_eq!(c.latest_submission(), Some((1, Custody::Submitted)));
                let mut s = c.take_session().unwrap();
                assert!(c.take_session().is_none());
                ready(&mut s);
                s.close();
            }
            drop(c);
            let report = peer.join().unwrap();
            assert_eq!(report.retries, usize::from(mode == Mode::NegotiationRetry));
        }
    }
}

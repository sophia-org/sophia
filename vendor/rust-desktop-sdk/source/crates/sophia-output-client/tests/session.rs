use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_output_client::{
    Session, SessionError, SessionState, protocol::*, startup::Opening, submission::Custody,
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};
fn limits() -> Vec<u8> {
    // Literal output Limits object, independent of the SDK encoder.
    let mut b = 72u32.to_le_bytes().to_vec();
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(7u64.to_le_bytes());
    b.extend([0; 16]);
    b.extend([
        1, 0, 16, 0, 16, 0, 128, 0, 4, 0, 64, 0, 0, 8, 0, 0, 64, 0, 0, 0, 0, 64, 0, 0, 248, 6, 0,
        0, 224, 46, 0, 0, 208, 7, 0, 0, 0, 16, 0, 0,
    ]);
    b
}
fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .flat_map(|word| {
            assert_eq!(word.len() % 2, 0);
            (0..word.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&word[offset..offset + 2], 16).unwrap())
        })
        .collect()
}

fn literal() -> Vec<u8> {
    let mut bytes = hex("
        0700000000000000 0200000000000000 0100 0100 0100 0000
        0300000000000000 0400000000000000 0700 ff00 0400 0100
        0500000000000000 0000 0000 00000000 44502d31
    ");
    bytes.extend([0; 60]); // The remaining fixed 64-byte label slot.
    bytes.extend(hex("
        0500000000000000 20030000 58020000 60ea0000 0100 0000
        0200000000000000 0600000000000000 00000000 00000000 20030000 58020000 0100 0000
        0300000000000000 0300 0000
    "));
    bytes.extend([0; 36]); // Three unused member slots.
    assert_eq!(bytes.len(), 236);
    bytes
}

fn object() -> Vec<u8> {
    let body = literal();
    let mut bytes = vec![0; 32];
    bytes[..4].copy_from_slice(&268u32.to_le_bytes());
    bytes[4] = 1;
    bytes[6] = 2;
    bytes[8] = 7;
    bytes.extend(body);
    bytes
}
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
    b.extend(55u64.to_le_bytes());
    b
}
fn event(kind: u16, seq: u64, body: &[u8]) -> Vec<u8> {
    let mut b = ((body.len() + 32) as u32).to_le_bytes().to_vec();
    b.extend(1u16.to_le_bytes());
    b.extend(kind.to_le_bytes());
    b.extend(7u64.to_le_bytes());
    b.extend([0; 8]);
    b.extend(seq.to_le_bytes());
    b.extend(body);
    b
}
fn rr(tag: u16, data: &[u8]) -> Vec<u8> {
    let mut b = (data.len() as u32).to_le_bytes().to_vec();
    b.extend(data);
    frame(117, tag, &b)
}
#[derive(Clone, Copy)]
enum PeerFault {
    Ack(u32),
    Disconnect { accepted: bool },
}

fn fixture(refuse: bool) -> (Session, thread::JoinHandle<u64>) {
    fixture_mode(refuse, false, false)
}
fn fixture_mode(refuse: bool, attached: bool, retry: bool) -> (Session, thread::JoinHandle<u64>) {
    fixture_fault(refuse, attached, retry, None)
}
fn fixture_fault(
    refuse: bool,
    attached: bool,
    retry: bool,
    fault: Option<PeerFault>,
) -> (Session, thread::JoinHandle<u64>) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let handle = thread::spawn(move || {
        let mut files = HashMap::<u32, String>::new();
        let mut queued = vec![];
        let mut pending = None;
        let mut candidate: Vec<u8> = vec![];
        let mut acked = 0;
        let mut topology_clunked = false;
        let mut attaches = 0;
        let mut versions = 0;
        let mut proposal_attempts = 0;
        let mut retry_bytes = None;
        let mut next_sequence = 4;
        loop {
            let mut h = [0; 7];
            match peer.read_exact(&mut h) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => panic!("{e}"),
            }
            let mut b = vec![0; u32::from_le_bytes(h[..4].try_into().unwrap()) as usize - 7];
            peer.read_exact(&mut b).unwrap();
            let tag = u16::from_le_bytes(h[5..].try_into().unwrap());
            let fid = u32::from_le_bytes(b[..4].try_into().unwrap());
            match h[4] {
                100 => {
                    versions += 1;
                    assert_eq!(versions, 1);
                    peer.write_all(&frame(101, tag, &b)).unwrap();
                }
                104 => {
                    attaches += 1;
                    assert_eq!(attaches, 1);
                    files.insert(fid, "root".into());
                    peer.write_all(&frame(105, tag, &qid(true))).unwrap();
                }
                110 => {
                    let newfid = u32::from_le_bytes(b[4..8].try_into().unwrap());
                    let name = String::from_utf8(b[12..].to_vec()).unwrap();
                    if name == "transaction" {
                        assert!(
                            !files.values().any(|name| name == "transaction"),
                            "previous transaction must be clunked"
                        );
                        if next_sequence > 4 {
                            assert!(acked >= next_sequence - 2, "Submitted ACK barrier");
                        }
                        candidate.clear();
                    }
                    files.insert(newfid, name);
                    let mut data = vec![1, 0];
                    data.extend(qid(false));
                    peer.write_all(&frame(111, tag, &data)).unwrap();
                }
                12 => {
                    let mut data = qid(false);
                    data.extend(0u32.to_le_bytes());
                    peer.write_all(&frame(13, tag, &data)).unwrap();
                }
                116 => {
                    let at = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
                    let count = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                    if files[&fid] == "events" {
                        pending = Some((tag, count));
                    } else {
                        let data = match files[&fid].as_str() {
                            "api" => b"sophia-output-files version=1\n".to_vec(),
                            "limits" => limits(),
                            "topology" => object(),
                            _ => panic!(),
                        };
                        peer.write_all(&rr(tag, &data[at..data.len().min(at + count.min(13))]))
                            .unwrap();
                    }
                }
                118 => {
                    let data = &b[16..];
                    match files[&fid].as_str() {
                        "transaction" => candidate.extend(data),
                        "ack" => {
                            acked = u64::from_le_bytes(data[8..16].try_into().unwrap());
                            if !refuse && acked >= 3 {
                                assert!(topology_clunked);
                                if let Some(PeerFault::Ack(errno)) = fault {
                                    let response = if errno == 0 {
                                        frame(119, tag, &15u32.to_le_bytes())
                                    } else {
                                        frame(7, tag, &errno.to_le_bytes())
                                    };
                                    peer.write_all(&response).unwrap();
                                    break;
                                }
                            }
                        }
                        "submit" => {
                            assert_eq!(&candidate[16..24], &data[8..16]);
                            let kind = u16::from_le_bytes(candidate[6..8].try_into().unwrap());
                            if kind == 257 {
                                proposal_attempts += 1;
                                if let Some(PeerFault::Disconnect { accepted }) = fault {
                                    if accepted {
                                        let mut body = data[8..16].to_vec();
                                        body.extend(257u16.to_le_bytes());
                                        body.extend([0; 6]);
                                        let bytes = event(18, next_sequence, &body);
                                        let (event_tag, count) =
                                            pending.take().expect("pending event read");
                                        assert!(count >= bytes.len());
                                        peer.write_all(&rr(event_tag, &bytes)).unwrap();
                                    }
                                    break;
                                }

                                assert_eq!(candidate[96..100], (-2i32).to_le_bytes());
                                if retry && proposal_attempts == 1 {
                                    retry_bytes = Some((candidate.clone(), data.to_vec()));
                                    peer.write_all(&frame(7, tag, &11u32.to_le_bytes()))
                                        .unwrap();
                                    continue;
                                }
                                if let Some((bytes, control)) = retry_bytes.take() {
                                    assert_eq!(candidate, bytes);
                                    assert_eq!(data, control);
                                }
                                let mut body = data[8..16].to_vec();
                                body.extend(257u16.to_le_bytes());
                                body.extend([0; 6]);
                                queued.extend(event(18, next_sequence, &body));
                                let mut body = candidate[32..40].to_vec();
                                body.extend(7u64.to_le_bytes());
                                body.extend([2, 0, 0, 0, 0, 0, 0, 0]);
                                queued.extend(event(32, next_sequence + 1, &body));
                                next_sequence += 2;
                            } else {
                                let mut body = 1u64.to_le_bytes().to_vec();
                                body.extend(256u16.to_le_bytes());
                                body.extend([0; 6]);
                                queued.extend(event(18, 1, &body));
                                if refuse {
                                    queued.extend(event(17, 2, &[1, 0, 0, 0, 0, 0, 0, 0]));
                                } else {
                                    let mut body = vec![1, 0, 0, 0, 0, 0, 0, 0];
                                    body.extend(3u64.to_le_bytes());
                                    body.extend([16, 0, 16, 0, 128, 0, 4, 0]);
                                    queued.extend(event(16, 2, &body));
                                    let mut body = vec![2, 0, 0, 0, 0, 0, 0, 0];
                                    body.extend(7u64.to_le_bytes());
                                    body.extend(55u64.to_le_bytes());
                                    queued.extend(event(19, 3, &body));
                                }
                            }
                        }
                        _ => panic!(),
                    }
                    peer.write_all(&frame(119, tag, &(data.len() as u32).to_le_bytes()))
                        .unwrap();
                    if refuse && acked == 2 {
                        break;
                    }
                }
                120 => {
                    if files[&fid] == "topology" {
                        topology_clunked = true;
                    }
                    files.remove(&fid);
                    peer.write_all(&frame(121, tag, &[])).unwrap();
                }
                _ => panic!("{}", h[4]),
            }
            if !queued.is_empty()
                && let Some((tag, count)) = pending.take()
            {
                let n = queued.len().min(count);
                peer.write_all(&rr(tag, &queued[..n])).unwrap();
                queued.drain(..n);
            }
        }
        acked
    });
    let offer = OutputV1ClientHello {
        minimum_revision: if refuse { 2 } else { 1 },
        maximum_revision: if refuse { 2 } else { 1 },
        capabilities: 3,
    };
    if retry {
        use sophia_output_client::Connecting;
        let mut connecting = if attached {
            let mut wire =
                Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
            let (tag, root) = wire.attach(b"output", b"output-authority").unwrap();
            wire.wait(tag, Instant::now() + Duration::from_secs(2))
                .unwrap();
            Connecting::attached(wire, root, offer, 0, 2000).unwrap()
        } else {
            Connecting::direct(client, PipelineLimits::default(), offer, 0, 2000).unwrap()
        };
        let end = Instant::now() + Duration::from_secs(2);
        while !connecting.ready() {
            assert!(Instant::now() < end);
            connecting.dispatch(1, 4096).unwrap();
            thread::yield_now();
        }
        let session = connecting.take_session().unwrap();
        assert!(connecting.take_session().is_none());
        return (session, handle);
    }
    let mut wire =
        Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let mut opening = if attached {
        let (tag, root) = wire.attach(b"output", b"output-authority").unwrap();
        assert!(matches!(
            wire.wait(tag, Instant::now() + Duration::from_secs(2))
                .unwrap(),
            sophia_9p_client::pipeline::Reply::Attach(_)
        ));
        Opening::attached(wire, root, offer, 0, 2000).unwrap()
    } else {
        Opening::direct(wire, offer, 0, 2000).unwrap()
    };
    let end = Instant::now() + Duration::from_secs(2);
    while !opening.is_opened() {
        assert!(Instant::now() < end);
        opening.dispatch(1, 4096).unwrap();
        thread::yield_now();
    }
    (
        Session::from_opened(opening.into_opened().unwrap()).unwrap(),
        handle,
    )
}
#[test]
fn output_session_fetches_before_delivery_and_ack() {
    let (mut s, peer) = fixture(false);
    let end = Instant::now() + Duration::from_secs(2);
    while s.event().is_none() {
        assert!(Instant::now() < end);
        s.dispatch(2, 4096).unwrap();
        thread::yield_now();
    }
    assert_eq!(s.state(), SessionState::Ready);
    assert_eq!(s.custody(1), Some(Custody::Submitted));
    assert_eq!(s.topology().unwrap().snapshot.topology_epoch, 7);
    assert_eq!(s.consumed(), 2);
    assert!(!s.obligations().topology_pending);
    s.consume().unwrap();
    while s.acked() < 3 {
        assert!(Instant::now() < end);
        s.dispatch(3, 4096).unwrap();
        thread::yield_now();
    }
    s.close();
    assert_eq!(s.custody(1), Some(Custody::Submitted));
    assert_eq!(peer.join().unwrap(), 3);
}
#[test]
fn refused_session_waits_for_ack_and_preserves_custody() {
    let (mut s, peer) = fixture(true);
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        assert!(Instant::now() < end);
        if s.dispatch(2, 4096).is_err() {
            break;
        }
        thread::yield_now();
    }
    assert_eq!(s.state(), SessionState::Refused);
    assert!(matches!(s.error(), Some(SessionError::Refused(_))));
    assert_eq!(s.acked(), 2);
    assert_eq!(s.custody(1), Some(Custody::Submitted));
    assert_eq!(peer.join().unwrap(), 2);
}

#[test]
fn zero_budget_and_expiry_do_not_send_queued_negotiation() {
    let (mut s, peer) = fixture(false);
    assert_eq!(s.dispatch(0, 4096), Err(SessionError::ClockRegression));
    s.dispatch(2, 0).unwrap();
    assert_eq!(s.custody(1), Some(Custody::Admitted));
    assert_eq!(s.dispatch(2000, 4096), Err(SessionError::Deadline));
    assert_eq!(s.custody(1), Some(Custody::DroppedUnsent));
    assert!(s.fd().is_none());
    assert_eq!(peer.join().unwrap(), 0);
}

fn proposal() -> OutputV1Proposal {
    OutputV1Proposal {
        connection_epoch: 7,
        candidate: OutputTopologyCandidate {
            base_topology_epoch: 7,
            intent: OutputTopologyIntent::Apply,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(3),
                head_generation: 4,
                mode: DisplayModeId::from_raw(5),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Automatic,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: OutputId::INVALID,
                logical: Rect {
                    x: -2,
                    y: 0,
                    width: 800,
                    height: 600,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Exact,
                }],
            }],
        },
    }
}

#[test]
fn proposals_retry_exact_bytes_and_keep_custody_on_both_startup_paths() {
    for attached in [false, true] {
        for retry in [false, true] {
            let (mut s, peer) = fixture_mode(false, attached, retry);
            let end = Instant::now() + Duration::from_secs(2);
            let mut now = 2;
            while s.event().is_none() {
                assert!(Instant::now() < end);
                s.dispatch(now, 4096).unwrap();
                thread::yield_now();
            }
            s.consume().unwrap();
            while s.acked() < 3 || s.obligations().deadline.is_some() {
                assert!(Instant::now() < end);
                s.dispatch(now, 4096).unwrap();
                thread::yield_now();
            }
            for (domain, sequence) in [(50, 5), (3, 7)] {
                let mut value = proposal();
                let ticket = s
                    .submit(TransactionId::from_raw(domain), &value, 1900)
                    .unwrap();
                value.candidate.groups[0].logical.x = 42;
                while s.event().is_none() {
                    assert!(Instant::now() < end);
                    now += 1;
                    s.dispatch(now, 4096).unwrap();
                    thread::yield_now();
                }
                assert_eq!(s.custody(ticket), Some(Custody::Submitted));
                let (header, sophia_output_client::journal::Event::Outcome(tx, _)) =
                    s.event().unwrap()
                else {
                    panic!()
                };
                assert_eq!(header.sequence, sequence);
                assert_eq!(tx.raw(), domain);
                s.consume().unwrap();
                while s.acked() < sequence || s.obligations().deadline.is_some() {
                    assert!(Instant::now() < end);
                    s.dispatch(now, 4096).unwrap();
                    thread::yield_now();
                }
            }
            assert_eq!(s.next_transaction().unwrap(), 51);
            s.close();
            assert_eq!(peer.join().unwrap(), 7);
        }
    }
}

#[test]
fn ack_faults_preserve_custody_and_terminal_state_on_both_paths() {
    for attached in [false, true] {
        for errno in [0, 22, 116] {
            let (mut s, peer) = fixture_fault(false, attached, false, Some(PeerFault::Ack(errno)));
            let end = Instant::now() + Duration::from_secs(2);
            while s.event().is_none() {
                assert!(Instant::now() < end);
                s.dispatch(2, 4096).unwrap();
                thread::yield_now();
            }
            s.consume().unwrap();
            loop {
                assert!(Instant::now() < end);
                if s.dispatch(3, 4096).is_err() {
                    break;
                }
                thread::yield_now();
            }
            assert_eq!(
                s.state(),
                if errno == 116 {
                    SessionState::Stale
                } else {
                    SessionState::Failed
                }
            );
            assert_eq!(s.remote_error(), (errno != 0).then_some(errno));
            assert_eq!(s.custody(1), Some(Custody::Submitted));
            assert!(s.acked() < 3);
            let state = s.state();
            let error = s.error().cloned();
            s.close();
            assert_eq!(s.state(), state);
            assert_eq!(s.error(), error.as_ref());
            assert!(s.fd().is_none());
            assert!(s.event().is_none());
            assert_eq!(s.wake_deadline(), None);
            assert_eq!(peer.join().unwrap(), 3);
        }
    }
}

#[test]
fn disconnect_does_not_confuse_confirmed_and_uncertain_proposals() {
    for attached in [false, true] {
        for accepted in [false, true] {
            let (mut s, peer) = fixture_fault(
                false,
                attached,
                false,
                Some(PeerFault::Disconnect { accepted }),
            );
            let end = Instant::now() + Duration::from_secs(2);
            while s.event().is_none() {
                assert!(Instant::now() < end);
                s.dispatch(2, 4096).unwrap();
                thread::yield_now();
            }
            s.consume().unwrap();
            while s.acked() < 3 || s.obligations().deadline.is_some() {
                assert!(Instant::now() < end);
                s.dispatch(2, 4096).unwrap();
                thread::yield_now();
            }
            let ticket = s
                .submit(TransactionId::from_raw(1), &proposal(), 1900)
                .unwrap();
            loop {
                assert!(Instant::now() < end);
                if s.dispatch(3, 4096).is_err() {
                    break;
                }
                thread::yield_now();
            }
            let expected = if accepted {
                Custody::Submitted
            } else {
                Custody::UnknownDisconnected
            };
            assert_eq!(s.custody(ticket), Some(expected));
            assert!(s.fd().is_none());
            s.close();
            assert_eq!(s.custody(ticket), Some(expected));
            peer.join().unwrap();
        }
    }
}

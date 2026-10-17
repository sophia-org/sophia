use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_portal_client::{
    protocol::requester::{Event, Limits},
    requester::{
        Custody,
        session::{Error, Session},
        startup::Opened,
    },
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};
fn number(b: &[u8]) -> u64 {
    b.iter()
        .enumerate()
        .map(|(i, v)| u64::from(*v) << (8 * i))
        .sum()
}
fn send(s: &mut UnixStream, kind: u8, tag: u16, b: &[u8]) {
    let mut out = ((b.len() + 7) as u32).to_le_bytes().to_vec();
    out.push(kind);
    out.extend(tag.to_le_bytes());
    out.extend(b);
    if let Err(e) = s.write_all(&out) {
        assert!(matches!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
        ));
    }
}
fn record(kind: u16, size: usize, sequence: u64) -> Vec<u8> {
    let mut b = vec![0; size];
    b[..4].copy_from_slice(&(size as u32).to_le_bytes());
    b[4] = 1;
    b[6..8].copy_from_slice(&kind.to_le_bytes());
    b[8] = 7;
    b[24..32].copy_from_slice(&sequence.to_le_bytes());
    b
}
fn fixture(fault: u8) -> (Opened, thread::JoinHandle<()>) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let job = thread::spawn(move || {
        let mut names = HashMap::new();
        let mut journal = Vec::new();
        let mut staged = Vec::new();
        let mut pending_read: Option<(u16, usize, usize)> = None;
        let mut submit_reply = None;
        let mut versions = 0;
        let mut attaches = 0;
        let mut sequence = 0;
        let mut last_submission = 0;
        let mut granted = Vec::new();
        let mut payload = Vec::new();
        let mut payload_length = 0;
        let mut revoked_write = None;
        let mut refused_end = None;
        loop {
            let mut h = [0; 7];
            if let Err(e) = peer.read_exact(&mut h) {
                assert!(matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ));
                break;
            }
            let mut b = vec![0; number(&h[..4]) as usize - 7];
            peer.read_exact(&mut b).unwrap();
            let tag = number(&h[5..]) as u16;
            match h[4] {
                100 => {
                    versions += 1;
                    assert_eq!(versions, 1);
                    send(&mut peer, 101, tag, &b);
                }
                104 => {
                    attaches += 1;
                    assert_eq!(attaches, 1);
                    let mut q = [0; 13];
                    q[0] = 128;
                    send(&mut peer, 105, tag, &q);
                }
                110 => {
                    if number(&b[8..10]) == 2 {
                        assert_eq!(&b[10..], b"\x06\x00upload\x01\x000");
                        names.insert(number(&b[4..8]), "upload/0".to_owned());
                        let mut q = vec![0; 28];
                        q[0] = 2;
                        q[2] = 128;
                        send(&mut peer, 111, tag, &q);
                        continue;
                    }
                    let name = String::from_utf8(b[12..].to_vec()).unwrap();
                    assert!(["events", "ack", "transaction", "submit"].contains(&name.as_str()));
                    if name == "transaction" {
                        staged.clear();
                    }
                    names.insert(number(&b[4..8]), name);
                    let mut q = [0; 15];
                    q[0] = 1;
                    send(&mut peer, 111, tag, &q);
                }
                12 => {
                    let name = &names[&number(&b[..4])];
                    let flags = number(&b[4..8]);
                    assert_eq!(
                        flags,
                        match name.as_str() {
                            "events" => 0,
                            "transaction" => 2,
                            _ => 1,
                        }
                    );
                    let mut q = [0; 17];
                    q[13] = if name == "events" || name == "transaction" {
                        7
                    } else {
                        24
                    };
                    send(&mut peer, 13, tag, &q);
                }
                116 => {
                    assert_eq!(&names[&number(&b[..4])], "events");
                    assert!(pending_read.is_none());
                    pending_read =
                        Some((tag, number(&b[4..12]) as usize, number(&b[12..16]) as usize));
                }
                118 => {
                    let name = &names[&number(&b[..4])];
                    let count = number(&b[12..16]) as usize;
                    assert_eq!(count, b.len() - 16);
                    match name.as_str() {
                        "upload/0" => {
                            if matches!(fault, 12 | 13) {
                                assert!(payload.is_empty(), "write after revocation");
                            }
                            assert_eq!(number(&b[4..12]) as usize, payload.len());
                            payload.extend_from_slice(&b[16..]);
                            assert!(payload.len() <= payload_length);
                            if fault == 13 {
                                send(&mut peer, 7, tag, &116u32.to_le_bytes());
                            } else if fault == 12 {
                                sequence += 1;
                                let mut event = record(33, 48, sequence);
                                event[32] = 11;
                                event[40] = 3;
                                journal.extend(event);
                                revoked_write = Some(tag);
                            } else {
                                send(&mut peer, 119, tag, &(count as u32).to_le_bytes());
                            }
                        }
                        "transaction" => {
                            assert_eq!(number(&b[4..12]) as usize, staged.len());
                            staged.extend(&b[16..]);
                            send(&mut peer, 119, tag, &(count as u32).to_le_bytes());
                        }
                        "submit" => {
                            assert_eq!(count, 24);
                            let kind = number(&staged[6..8]) as u16;
                            let submission = number(&staged[16..24]);
                            assert_eq!(submission, last_submission + 1);
                            if fault == 14 && kind == 259 {
                                if let Some((bytes, control)) = &refused_end {
                                    assert_eq!(&staged, bytes);
                                    assert_eq!(&b[16..], control);
                                } else {
                                    refused_end = Some((staged.clone(), b[16..].to_vec()));
                                    send(&mut peer, 7, tag, &11u32.to_le_bytes());
                                    continue;
                                }
                            }
                            last_submission = submission;
                            sequence += 1;
                            let mut receipt = record(18, 48, sequence);
                            receipt[32..40].copy_from_slice(
                                &(if fault == 3 {
                                    submission + 1
                                } else {
                                    submission
                                })
                                .to_le_bytes(),
                            );
                            receipt[40..42].copy_from_slice(&kind.to_le_bytes());
                            journal.extend(receipt);
                            sequence += 1;
                            if kind == 257 {
                                assert_eq!(staged.len(), 344);
                                let transfer = number(&staged[32..40]);
                                let denied =
                                    transfer == 0 || fault == 5 || granted.contains(&transfer);
                                let mut decision = record(32, 88, sequence);
                                decision[32..40].copy_from_slice(&transfer.to_le_bytes());
                                decision[40] = if denied { 1 } else { 2 };
                                if !denied {
                                    granted.push(transfer);
                                    decision[41] = 1;
                                    decision[42..44].copy_from_slice(&staged[56..58]);
                                    decision[48..64].copy_from_slice(&staged[40..56]);
                                    decision[64..72].copy_from_slice(&staged[72..80]);
                                    decision[72] = if fault == 6 { 18 } else { 17 };
                                    decision[80..88].copy_from_slice(&staged[80..88]);
                                }
                                journal.extend(decision);
                                if !denied && (fault == 7 || fault == 9) {
                                    sequence += 1;
                                    let mut outcome = record(33, 48, sequence);
                                    outcome[32..40].copy_from_slice(&transfer.to_le_bytes());
                                    outcome[40] = if fault == 7 { 2 } else { 3 };
                                    journal.extend(outcome);
                                }
                            } else if kind == 258 {
                                assert_eq!(staged.len(), 56);
                                assert!(granted.contains(&number(&staged[32..40])));
                                payload_length = number(&staged[48..56]) as usize;
                                payload.clear();
                                sequence -= 1;
                            } else if kind == 259 {
                                assert_eq!(staged.len(), 48);
                                assert_eq!(number(&staged[40..48]) as usize, payload.len());
                                assert_eq!(payload.len(), payload_length);
                                assert!(payload.iter().all(|b| *b == 17));
                                assert!(names.values().any(|name| name == "upload/0"));
                                let mut outcome = record(33, 48, sequence);
                                outcome[32..40].copy_from_slice(&staged[32..40]);
                                outcome[40] = 2;
                                journal.extend(outcome);
                            } else if kind == 260 {
                                assert_eq!(staged.len(), 40);
                                let transfer = number(&staged[32..40]);
                                assert!(granted.contains(&transfer));
                                let mut outcome = record(33, 48, sequence);
                                outcome[32..40].copy_from_slice(&transfer.to_le_bytes());
                                outcome[40] = 3;
                                journal.extend(outcome);
                            } else {
                                assert_eq!(kind, 256);
                                assert_eq!(staged.len(), 40);
                                let mut outcome = record(
                                    if fault == 1 { 17 } else { 16 },
                                    if fault == 1 { 40 } else { 48 },
                                    sequence,
                                );
                                outcome[32] = 1;
                                if fault != 1 {
                                    outcome[40] = 7;
                                }
                                journal.extend(outcome);
                            }
                            if fault == 2 {
                                submit_reply = Some(tag);
                            } else {
                                send(&mut peer, 119, tag, &24u32.to_le_bytes());
                            }
                        }
                        "ack" => {
                            assert_eq!(count, 16);
                            assert_eq!(number(&b[16..24]), 7);
                            send(
                                &mut peer,
                                119,
                                tag,
                                &(if fault == 4 { 15u32 } else { 16 }).to_le_bytes(),
                            );
                            if number(&b[24..32]) == sequence
                                && let Some(write_tag) = revoked_write.take()
                            {
                                send(&mut peer, 7, write_tag, &116u32.to_le_bytes());
                            }
                        }
                        _ => panic!("unexpected write"),
                    }
                }
                120 => {
                    names.remove(&number(&b[..4])).unwrap();
                    send(&mut peer, 121, tag, &[]);
                }
                _ => panic!("unexpected request {}", h[4]),
            }
            if let Some((tag, offset, count)) = pending_read
                && offset < journal.len()
            {
                let end = (offset + count).min(journal.len());
                let mut b = ((end - offset) as u32).to_le_bytes().to_vec();
                b.extend(&journal[offset..end]);
                send(&mut peer, 117, tag, &b);
                pending_read = None;
                if end >= 48
                    && let Some(tag) = submit_reply.take()
                {
                    send(&mut peer, 119, tag, &24u32.to_le_bytes());
                }
            }
        }
        if fault == 12 {
            assert!(revoked_write.is_none());
            assert!(!names.values().any(|name| name == "upload/0"));
        }
    });
    let mut wire = Pipeline::over(
        client,
        PipelineLimits {
            msize: 4096,
            max_outstanding: if fault >= 11 { 4 } else { 3 },
            max_fids: if fault >= 11 { 6 } else { 5 },
            max_buffered_input: 4096 + 22,
            ..PipelineLimits::default()
        },
        Duration::from_secs(2),
    )
    .unwrap();
    let (tag, root) = wire.attach(b"", b"").unwrap();
    wire.wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap();
    (
        Opened {
            pipeline: wire,
            root,
            epoch: 7,
            now: 0,
            limits: Limits {
                max_transfers: 64,
                max_payload_bytes: 65536,
                journal_records: 256,
                journal_bytes: 32768,
                assembly_timeout_ms: 12000,
                ack_progress_timeout_ms: 2000,
                max_retained_transfers: 4096,
                broker_generation: 17,
            },
        },
        job,
    )
}
#[test]
fn negotiation_retains_events_and_requires_ack_before_ready() {
    for fault in [0, 2] {
        let (opened, peer) = fixture(fault);
        let mut s = Session::new(opened, 1, 1, 10000).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        let mut events = 0;
        while !s.ready() {
            assert!(Instant::now() < until);
            s.dispatch(1, 1).unwrap();
            if let Some(e) = s.event() {
                assert!(!s.ready());
                assert_eq!(s.event(), Some(e));
                events += 1;
                assert_eq!(e.sequence, events);
                assert!(matches!(
                    e.event,
                    Event::Submitted { .. } | Event::Negotiated
                ));
                s.consume(1).unwrap();
            }
            match s.acknowledge(1) {
                Ok(()) | Err(Error::State(sophia_portal_client::requester::Error::Busy)) => (),
                e => panic!("{e:?}"),
            }
            thread::yield_now();
        }
        assert_eq!(events, 2);
        assert_eq!(s.custody(), Custody::Submitted);
        s.close();
        peer.join().unwrap();
    }
}
#[test]
fn refusal_wrong_receipt_and_short_ack_close_without_replay() {
    for fault in [1, 3, 4] {
        let (opened, peer) = fixture(fault);
        let mut s = Session::new(opened, 1, 1, 10000).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while s.error().is_none() {
            assert!(Instant::now() < until);
            if s.dispatch(1, 4096).is_err() {
                break;
            }
            if s.event().is_some() {
                let _ = s.consume(1);
            }
            if s.error().is_none() {
                let _ = s.acknowledge(1);
            }
            thread::yield_now();
        }
        assert!(!s.ready() && s.fd().is_none());
        if fault == 1 {
            assert_eq!(s.error(), Some(&Error::Refused));
        }
        peer.join().unwrap();
    }
}

fn pump(s: &mut Session) -> Result<(), Error> {
    s.dispatch(2, 4096)?;
    if s.event().is_some() {
        s.consume(2)?;
    }
    match s.acknowledge(2) {
        Ok(()) | Err(Error::State(sophia_portal_client::requester::Error::Busy)) => Ok(()),
        e => e,
    }
}
#[test]
fn requests_correlate_grants_and_retain_terminal_results_until_ack() {
    use sophia_portal_client::protocol::requester::{TransferKind, TransferRequest};
    use sophia_portal_client::requester::transfer::State;
    for fault in [5, 6, 7, 8, 9, 10] {
        let (opened, peer) = fixture(fault);
        let mut s = Session::new(opened, 1, 1, 10000).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while !s.ready() {
            assert!(Instant::now() < until);
            pump(&mut s).unwrap();
            thread::yield_now();
        }
        let request = TransferRequest {
            transfer: 11,
            source: 2,
            target: 3,
            kind: TransferKind::Clipboard,
            mime: Some("text/plain"),
            byte_size: 3,
            generation: 5,
            deadline_msec: 900,
        };
        let id = s.request(request, 2, 10000).unwrap();
        assert!(s.ready());
        assert!(matches!(
            s.request(request, 2, 10000),
            Err(Error::State(sophia_portal_client::requester::Error::Busy))
        ));
        loop {
            assert!(Instant::now() < until);
            if pump(&mut s).is_err() {
                break;
            }
            let state = s.transfer(id).unwrap().state();
            if (fault == 8 || fault == 10) && state == State::Allowed {
                break;
            }
            if matches!(state, State::Denied | State::Revoked) {
                match s.release_transfer(id, 2) {
                    Ok(()) => {
                        assert!(s.transfer(id).is_none());
                        break;
                    }
                    Err(Error::State(sophia_portal_client::requester::Error::Busy)) => (),
                    e => panic!("{e:?}"),
                }
            }
            thread::yield_now();
        }
        if fault == 6 || fault == 7 {
            assert!(s.error().is_some() && s.fd().is_none());
            assert!(s.transfer(id).unwrap().grant().is_none());
        } else {
            assert!(s.error().is_none());
        }
        if fault == 8 {
            let original = s.transfer(id).unwrap().grant().unwrap();
            let duplicate = loop {
                assert!(Instant::now() < until);
                pump(&mut s).unwrap();
                match s.request(request, 2, 10000) {
                    Ok(id) => break id,
                    Err(Error::State(sophia_portal_client::requester::Error::Busy)) => (),
                    e => panic!("{e:?}"),
                }
            };
            while s.transfer(duplicate).unwrap().state() == State::Pending {
                assert!(Instant::now() < until);
                pump(&mut s).unwrap();
                thread::yield_now();
            }
            assert_eq!(s.transfer(duplicate).unwrap().state(), State::Denied);
            assert_eq!(s.transfer(id).unwrap().grant(), Some(original));
        }
        if fault == 10 {
            loop {
                assert!(Instant::now() < until);
                pump(&mut s).unwrap();
                match s.cancel(id, 2, 10000) {
                    Ok(()) => break,
                    Err(Error::State(sophia_portal_client::requester::Error::Busy)) => (),
                    e => panic!("{e:?}"),
                }
            }
            assert_eq!(s.transfer(id).unwrap().state(), State::Allowed);
            assert!(matches!(
                s.cancel(id, 2, 10000),
                Err(Error::State(sophia_portal_client::requester::Error::Busy))
            ));
            loop {
                assert!(Instant::now() < until);
                pump(&mut s).unwrap();
                if s.transfer(id).unwrap().state() == State::Revoked {
                    match s.release_transfer(id, 2) {
                        Ok(()) => break,
                        Err(Error::State(sophia_portal_client::requester::Error::Busy)) => (),
                        e => panic!("{e:?}"),
                    }
                }
                thread::yield_now();
            }
        }
        s.close();
        peer.join().unwrap();
    }
}

#[test]
fn request_handles_cannot_cross_sessions_with_equal_wire_epochs() {
    use sophia_portal_client::protocol::requester::{TransferKind, TransferRequest};
    let (a, pa) = fixture(8);
    let (b, pb) = fixture(8);
    let mut a = Session::new(a, 1, 1, 10000).unwrap();
    let mut b = Session::new(b, 1, 1, 10000).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while !a.ready() || !b.ready() {
        assert!(Instant::now() < until);
        pump(&mut a).unwrap();
        pump(&mut b).unwrap();
        thread::yield_now();
    }
    let request = TransferRequest {
        transfer: 11,
        source: 2,
        target: 3,
        kind: TransferKind::Clipboard,
        mime: None,
        byte_size: 0,
        generation: 5,
        deadline_msec: 900,
    };
    let aid = a.request(request, 2, 10000).unwrap();
    let bid = b.request(request, 2, 10000).unwrap();
    assert_ne!(aid, bid);
    assert!(a.transfer(bid).is_none() && b.transfer(aid).is_none());
    assert!(matches!(
        a.release_transfer(bid, 2),
        Err(Error::State(
            sophia_portal_client::requester::Error::Invalid
        ))
    ));
    a.close();
    b.close();
    pa.join().unwrap();
    pb.join().unwrap();
}

#[test]
fn begin_upload_end_keeps_writer_open_and_returns_original_payload() {
    upload_completion(false, 0);
}

#[test]
fn cancel_held_upload_waits_for_revocation_and_returns_original_payload() {
    upload_completion(true, 0);
}

#[test]
fn revocation_drains_pending_upload_write_without_replay() {
    upload_completion(false, 1);
}

#[test]
fn stale_write_without_outcome_closes_without_inventing_revocation() {
    upload_completion(false, 2);
}

#[test]
fn end_eagain_requires_explicit_same_submission_retry() {
    upload_completion(false, 3);
}

fn upload_completion(cancel: bool, revocation_order: u8) {
    use sophia_portal_client::protocol::requester::{TransferKind, TransferRequest};
    use sophia_portal_client::requester::transfer::State;
    for length in [0, 65536] {
        let revoke = matches!(revocation_order, 1 | 2);
        if revoke && length == 0 {
            continue;
        }
        let (opened, peer) = fixture(11 + revocation_order);
        let mut s = Session::new(opened, 1, 1, 10000).unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while !s.transaction_ready() {
            assert!(Instant::now() < until);
            pump(&mut s).unwrap();
            thread::yield_now();
        }
        let id = s
            .request(
                TransferRequest {
                    transfer: 11,
                    source: 2,
                    target: 3,
                    kind: TransferKind::Clipboard,
                    mime: None,
                    byte_size: length as u64,
                    generation: 5,
                    deadline_msec: 900,
                },
                2,
                10000,
            )
            .unwrap();
        while !s.transaction_ready() {
            assert!(Instant::now() < until);
            pump(&mut s).unwrap();
            thread::yield_now();
        }
        let mut bytes = vec![17; length];
        let pointer = bytes.as_ptr();
        let capacity = bytes.capacity();
        assert!(s.begin_buffer(id, &mut bytes, 2, 2).is_err());
        assert_eq!(bytes.as_ptr(), pointer);
        assert_eq!(bytes.capacity(), capacity);
        assert_eq!(bytes, vec![17; length]);
        assert_eq!(s.upload_written(id), None);
        s.begin_buffer(id, &mut bytes, 2, 10000).unwrap();
        assert!(bytes.is_empty());
        let mut other = vec![19; 1];
        let other_pointer = other.as_ptr();
        assert!(s.begin_buffer(id, &mut other, 2, 10000).is_err());
        assert_eq!(other.as_ptr(), other_pointer);
        assert_eq!(other, [19]);
        assert!(matches!(
            s.end(id, 2, 10000),
            Err(Error::State(sophia_portal_client::requester::Error::Busy))
        ));
        if revocation_order == 2 {
            loop {
                assert!(Instant::now() < until);
                if let Err(e) = pump(&mut s) {
                    assert!(matches!(
                        e,
                        Error::State(sophia_portal_client::requester::Error::Remote(116))
                    ));
                    break;
                }
                thread::yield_now();
            }
            let transfer = s.transfer(id).unwrap();
            assert_eq!(transfer.state(), State::Allowed);
            assert!(transfer.disconnected());
            assert!(transfer.grant().is_none());
            let returned = s.take_upload(id).unwrap();
            assert_eq!(returned.as_ptr(), pointer);
            assert_eq!(returned.len(), length);
            peer.join().unwrap();
            continue;
        }
        while !revoke && (!s.upload_ready(id) || !s.transaction_ready()) {
            assert!(Instant::now() < until);
            pump(&mut s).unwrap();
            thread::yield_now();
        }
        if !revoke {
            assert_eq!(s.upload_written(id), Some(length));
        }
        if revoke {
            // The peer revokes while the first payload write awaits its reply.
        } else if cancel {
            s.cancel(id, 2, 10000).unwrap();
            assert_eq!(s.transfer(id).unwrap().state(), State::Allowed);
        } else {
            s.end(id, 2, 10000).unwrap();
        }
        if !revoke {
            assert!(matches!(
                s.cancel(id, 2, 10000),
                Err(Error::State(sophia_portal_client::requester::Error::Busy))
            ));
        }
        if revocation_order == 3 {
            while s.custody() != Custody::Queued
                || s.transfer(id).unwrap().state() != State::EndIssued
            {
                assert!(Instant::now() < until);
                pump(&mut s).unwrap();
                thread::yield_now();
            }
            for _ in 0..16 {
                pump(&mut s).unwrap();
                assert_eq!(s.custody(), Custody::Queued);
                assert_eq!(s.transfer(id).unwrap().state(), State::EndIssued);
            }
            s.retry(2).unwrap();
        }
        let returned = loop {
            assert!(Instant::now() < until);
            pump(&mut s).unwrap();
            match s.take_upload(id) {
                Ok(bytes) => break bytes,
                Err(Error::State(sophia_portal_client::requester::Error::Busy)) => (),
                e => panic!("{e:?}"),
            }
            thread::yield_now();
        };
        assert_eq!(returned.as_ptr(), pointer);
        assert_eq!(returned.len(), length);
        assert_eq!(
            s.transfer(id).unwrap().state(),
            if cancel || revoke {
                State::Revoked
            } else {
                State::Completed
            }
        );
        loop {
            assert!(Instant::now() < until);
            pump(&mut s).unwrap();
            match s.release_transfer(id, 2) {
                Ok(()) => break,
                Err(Error::State(sophia_portal_client::requester::Error::Busy)) => (),
                e => panic!("{e:?}"),
            }
        }
        s.close();
        peer.join().unwrap();
    }
}

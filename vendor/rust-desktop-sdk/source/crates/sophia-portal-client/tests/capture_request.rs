use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_portal_client::{
    capture::Custody,
    capture_request::Request,
    protocol::{CaptureRequest, CaptureState},
    startup::Opening,
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

fn send(s: &mut UnixStream, kind: u8, tag: &[u8], body: &[u8]) {
    let mut reply = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    reply.push(kind);
    reply.extend(tag);
    reply.extend(body);
    s.write_all(&reply).unwrap();
}
fn fixture(
    fault: u8,
) -> (
    sophia_portal_client::startup::Opened,
    thread::JoinHandle<usize>,
) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let job = thread::spawn(move || {
        let mut current = String::new();
        let mut serial = 0u64;
        let mut writes = 0;
        let mut snapshots = 0;
        let mut clunks = 0;
        let mut versions = 0;
        let mut attaches = 0;
        loop {
            let mut h = [0; 7];
            match peer.read_exact(&mut h) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => panic!("{e}"),
            }
            let mut b = vec![0; u32::from_le_bytes(h[..4].try_into().unwrap()) as usize - 7];
            peer.read_exact(&mut b).unwrap();
            match h[4] {
                100 => {
                    versions += 1;
                    assert_eq!(versions, 1);
                    send(&mut peer, 101, &h[5..], &b);
                }
                104 => {
                    attaches += 1;
                    assert_eq!(attaches, 1);
                    let mut q = [0; 13];
                    q[0] = 128;
                    send(&mut peer, 105, &h[5..], &q);
                }
                110 => {
                    current = String::from_utf8(b[12..].to_vec()).unwrap();
                    assert!(["api", "capture", "capture-status"].contains(&current.as_str()));
                    if current == "capture-status" {
                        snapshots += 1;
                    }
                    let mut r = [0; 15];
                    r[0] = 1;
                    if fault == 7 && current == "capture" {
                        r[2] = 128;
                    }
                    send(&mut peer, 111, &h[5..], &r);
                }
                12 => {
                    assert_eq!(
                        u32::from_le_bytes(b[4..8].try_into().unwrap()),
                        u32::from(current == "capture")
                    );
                    let mut r = [0; 17];
                    let unit: u32 = if current == "capture" {
                        if fault == 4 { 23 } else { 24 }
                    } else {
                        7
                    };
                    r[13..].copy_from_slice(&unit.to_le_bytes());
                    send(&mut peer, 13, &h[5..], &r);
                }
                118 => {
                    assert_eq!(current, "capture");
                    assert_eq!(b.len(), 40);
                    assert_eq!(&b[4..12], &[0; 8]);
                    assert_eq!(&b[12..16], &24u32.to_le_bytes());
                    assert_eq!(&b[16..24], &1u64.to_le_bytes());
                    serial = u64::from_le_bytes(b[24..32].try_into().unwrap());
                    writes += 1;
                    assert_eq!(serial, 8 + writes as u64);
                    assert_eq!(&b[32..], &7u64.to_le_bytes());
                    snapshots = 0;
                    if fault == 6 {
                        break;
                    }
                    if fault == 1 {
                        send(&mut peer, 7, &h[5..], &11u32.to_le_bytes());
                    } else {
                        send(
                            &mut peer,
                            119,
                            &h[5..],
                            &(if fault == 2 { 23u32 } else { 24 }).to_le_bytes(),
                        );
                    }
                }
                116 => {
                    let bytes = if current == "api" {
                        b"sophia-portal-client version=1 epoch=7 request=output-capture-v1 fd_transfer=none\n".to_vec()
                    } else {
                        assert_eq!(current, "capture-status");
                        [
                            1,
                            if fault == 3 { serial + 1 } else { serial },
                            7,
                            if snapshots == 1 { 1 } else { 3 },
                        ]
                        .into_iter()
                        .flat_map(u64::to_le_bytes)
                        .collect()
                    };
                    let offset = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
                    let count = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                    assert!(count <= 7 && offset <= bytes.len());
                    let data = &bytes[offset..bytes.len().min(offset + count)];
                    let mut r = (data.len() as u32).to_le_bytes().to_vec();
                    r.extend(data);
                    send(&mut peer, 117, &h[5..], &r);
                }
                120 => {
                    clunks += 1;
                    if fault == 5 && current == "capture-status" && snapshots == 2 {
                        send(&mut peer, 7, &h[5..], &5u32.to_le_bytes());
                    } else {
                        send(&mut peer, 121, &h[5..], &[]);
                    }
                }
                _ => panic!("unexpected request {}", h[4]),
            }
        }
        if fault == 0 {
            assert_eq!(clunks, 7);
        }
        writes
    });
    let mut wire =
        Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let (tag, root) = wire.attach(b"", b"").unwrap();
    wire.wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap();
    let mut opening = Opening::from_attached(wire, root, 0, 100).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    while !opening.is_opened() {
        assert!(Instant::now() < until);
        opening.dispatch(1, 4096).unwrap();
        thread::yield_now();
    }
    (opening.into_opened().unwrap(), job)
}
#[test]
fn atomic_capture_fragmented_status_and_explicit_reuse() {
    let (mut opened, job) = fixture(0);
    let mut previous = 8;
    for serial in [9, 10] {
        let mut now = opened.now;
        let mut request = Request::new(
            opened,
            CaptureRequest { serial, output: 7 },
            previous,
            now,
            100,
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while request.outcome().is_none() {
            assert!(Instant::now() < until);
            if let Some(wake) = request.next_wake()
                && wake < 100
                && wake > now
            {
                now = wake;
            }
            request.dispatch(now, 1).unwrap();
            thread::yield_now();
        }
        assert_eq!(request.custody(), Custody::Settled);
        assert_eq!(request.next_wake(), None);
        let (o, outcome, last) = request.finish().unwrap();
        assert_eq!(outcome, CaptureState::Ready);
        assert_eq!(last, serial);
        assert_eq!(o.pipeline.fid_count(), 1);
        assert!(o.pipeline.is_quiescent());
        opened = o;
        previous = last;
    }
    drop(opened);
    assert_eq!(job.join().unwrap(), 2);
}
#[test]
fn failures_preserve_custody_and_never_retry() {
    for fault in 1..=7 {
        let (opened, job) = fixture(fault);
        let mut request = Request::new(
            opened,
            CaptureRequest {
                serial: 9,
                output: 7,
            },
            8,
            1,
            100,
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        let mut now = 1;
        while request.error().is_none() {
            assert!(Instant::now() < until);
            if let Some(wake) = request.next_wake()
                && wake < 100
                && wake > now
            {
                now = wake;
            }
            let _ = request.dispatch(now, 4096);
            thread::yield_now();
        }
        assert!(request.outcome().is_none() && request.fd().is_none());
        assert_eq!(
            request.custody(),
            if fault == 1 {
                Custody::Refused
            } else if fault == 4 || fault == 7 {
                Custody::DroppedUnsent
            } else {
                Custody::Unknown
            }
        );
        assert!(request.dispatch(2, 4096).is_err());
        assert_eq!(job.join().unwrap(), usize::from(fault != 4 && fault != 7));
    }
}

#[test]
fn local_rejection_expiry_and_cancellation_do_not_send_capture() {
    use sophia_portal_client::{capture_request::Error, protocol::RequestCapability};
    for fault in 0..6 {
        let (mut opened, job) = fixture(8);
        let capture = CaptureRequest {
            serial: 9,
            output: 7,
        };
        if fault == 0 {
            opened.info.request = RequestCapability::Unavailable;
            assert!(matches!(
                Request::new(opened, capture, 8, 1, 100),
                Err(Error::Unsupported)
            ));
        } else if fault == 1 {
            assert!(matches!(
                Request::new(opened, capture, 9, 1, 100),
                Err(Error::Invalid)
            ));
        } else if fault == 2 {
            assert!(Request::new(opened, capture, 8, 1, 1).is_err());
        } else if fault == 3 {
            assert!(matches!(
                Request::new(opened, capture, 8, 0, 100),
                Err(Error::ClockRegression)
            ));
        } else {
            let mut request = Request::new(opened, capture, 8, 1, 100).unwrap();
            if fault == 4 {
                assert!(request.dispatch(100, 4096).is_err());
            } else {
                assert_eq!(request.dispatch(0, 4096), Err(Error::ClockRegression));
                assert!(request.error().is_none());
                request.close();
            }
            assert_eq!(request.custody(), Custody::DroppedUnsent);
            assert!(request.fd().is_none());
        }
        assert_eq!(job.join().unwrap(), 0);
    }
}

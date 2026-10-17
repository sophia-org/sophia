use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_portal_client::{
    startup::{Opened, Opening},
    watch::{Error, Watch},
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
fn fixture(fault: u8) -> (Opened, thread::JoinHandle<(usize, usize)>) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let job = thread::spawn(move || {
        let mut name = String::new();
        let (mut versions, mut attaches, mut watches, mut clunks) = (0, 0, 0, 0);
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
                    name = String::from_utf8(b[12..].to_vec()).unwrap();
                    assert!(name == "api" || name == "watch");
                    if name == "watch" {
                        watches += 1;
                    }
                    let mut r = [0; 15];
                    r[0] = 1;
                    if fault == 7 && name == "watch" {
                        r[2] = 128;
                    }
                    send(&mut peer, 111, &h[5..], &r);
                }
                12 => {
                    assert_eq!(&b[4..8], &[0; 4]);
                    if fault == 6 && name == "watch" {
                        send(&mut peer, 7, &h[5..], &116u32.to_le_bytes());
                    } else {
                        send(&mut peer, 13, &h[5..], &[0; 17]);
                    }
                }
                116 => {
                    let offset = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
                    let count = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                    if name == "api" {
                        let api = b"sophia-portal-client version=1 epoch=7 request=unavailable fd_transfer=none\n";
                        let data = &api[offset..api.len().min(offset + count)];
                        let mut r = (data.len() as u32).to_le_bytes().to_vec();
                        r.extend(data);
                        send(&mut peer, 117, &h[5..], &r);
                    } else {
                        assert_eq!((offset, count), (0, 1));
                        match fault {
                            1 => send(&mut peer, 117, &h[5..], &[0; 4]),
                            2 => send(&mut peer, 117, &h[5..], &[1, 0, 0, 0, 42]),
                            3 => send(&mut peer, 7, &h[5..], &5u32.to_le_bytes()),
                            5 => break,
                            _ => send(&mut peer, 7, &h[5..], &116u32.to_le_bytes()),
                        }
                    }
                }
                120 => {
                    if name == "watch" {
                        clunks += 1;
                    }
                    if fault == 4 && name == "watch" {
                        send(&mut peer, 7, &h[5..], &5u32.to_le_bytes());
                    } else {
                        send(&mut peer, 121, &h[5..], &[]);
                    }
                }
                _ => panic!("unexpected operation {}", h[4]),
            }
        }
        (watches, clunks)
    });
    let mut wire =
        Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    // Supplied admission, not a production authentication test.
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
fn explicit_reopen_reuses_root_after_read_or_open_revocation() {
    for fault in [0, 6] {
        let (mut opened, job) = fixture(fault);
        for _ in 0..2 {
            let mut watch = Watch::new(opened, 10, 100).unwrap();
            assert_eq!(watch.dispatch(9, 4096), Err(Error::ClockRegression));
            assert!(watch.error().is_none());
            let until = Instant::now() + Duration::from_secs(2);
            while !watch.revoked() {
                assert!(Instant::now() < until);
                watch.dispatch(10, 1).unwrap();
                thread::yield_now();
            }
            assert_eq!(watch.deadline(), None);
            opened = watch.finish().unwrap();
            assert_eq!(opened.pipeline.fid_count(), 1);
            assert!(opened.pipeline.is_quiescent());
        }
        drop(opened);
        assert_eq!(job.join().unwrap(), (2, 2));
    }
}

#[test]
fn malformed_reads_loss_and_clunk_refusal_never_report_revocation() {
    for fault in [1, 2, 3, 4, 5, 7] {
        let (opened, job) = fixture(fault);
        let mut watch = Watch::new(opened, 10, 100).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while watch.error().is_none() {
            assert!(Instant::now() < until);
            let _ = watch.dispatch(10, 4096);
            thread::yield_now();
        }
        assert!(!watch.revoked() && watch.fd().is_none());
        match fault {
            3 | 4 => assert_eq!(watch.error(), Some(&Error::Remote(5))),
            5 => assert!(matches!(watch.error(), Some(Error::Wire(_)))),
            _ => assert_eq!(watch.error(), Some(&Error::Invalid)),
        }
        assert!(watch.finish().is_err());
        assert_eq!(job.join().unwrap(), (1, usize::from(fault == 4)));
    }
}

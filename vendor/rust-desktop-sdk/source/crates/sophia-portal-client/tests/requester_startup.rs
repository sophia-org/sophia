use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_portal_client::requester::startup::{Error, Opening};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

fn send(s: &mut UnixStream, kind: u8, tag: &[u8], body: &[u8]) {
    s.write_all(&((body.len() + 7) as u32).to_le_bytes())
        .unwrap();
    s.write_all(&[kind]).unwrap();
    s.write_all(tag).unwrap();
    s.write_all(body).unwrap();
}
fn qid(directory: bool) -> Vec<u8> {
    let mut q = vec![0; 13];
    q[0] = if directory { 128 } else { 0 };
    q
}
fn fixture(
    fault: u8,
) -> (
    Pipeline,
    sophia_9p_records::Fid,
    thread::JoinHandle<Vec<String>>,
) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let job = thread::spawn(move || {
        let mut names = vec![];
        let mut current = String::new();
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
                    send(&mut peer, 105, &h[5..], &qid(true));
                }
                110 => {
                    current = String::from_utf8(b[12..].to_vec()).unwrap();
                    assert!(current == "api" || current == "limits");
                    names.push(current.clone());
                    let mut reply = vec![1, 0];
                    reply.extend(qid(fault == 2));
                    send(&mut peer, 111, &h[5..], &reply);
                }
                12 => {
                    assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 0);
                    let mut reply = qid(fault == 3);
                    let unit: u32 = 7;
                    reply.extend(unit.to_le_bytes());
                    send(&mut peer, 13, &h[5..], &reply);
                }
                116 => {
                    let mut info = if current == "api" {
                        format!("sophia-portal-files version={} role=portal-requester epoch=7 fd_transfer=none\n", if fault == 1 { 2 } else { 1 }).into_bytes()
                    } else {
                        let mut v = vec![0; 80];
                        v[..4].copy_from_slice(&80u32.to_le_bytes());
                        v[4] = 1;
                        v[6] = 1;
                        v[8] = if fault == 5 { 8 } else { 7 };
                        v[32] = 1;
                        v[34] = 1;
                        v[44] = 255;
                        for (at, value) in [
                            (36, 64u32),
                            (40, 65536),
                            (48, 256),
                            (52, 32768),
                            (56, 12000),
                            (60, 2000),
                            (64, 344),
                            (68, 4096),
                        ] {
                            v[at..at + 4].copy_from_slice(&value.to_le_bytes());
                        }
                        v[72] = 17;
                        if fault == 6 {
                            v.push(0);
                        }
                        if fault == 7 {
                            v.pop();
                        }
                        v
                    };
                    let info = info.as_mut_slice();
                    let offset = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
                    let count = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                    assert!(count <= 7);
                    let data = &info[offset..info.len().min(offset + count)];
                    let mut reply = (data.len() as u32).to_le_bytes().to_vec();
                    reply.extend(data);
                    send(&mut peer, 117, &h[5..], &reply);
                }
                120 => {
                    if fault == 4 {
                        send(&mut peer, 7, &h[5..], &5u32.to_le_bytes());
                    } else {
                        send(&mut peer, 121, &h[5..], &[]);
                    }
                }
                _ => panic!("unexpected operation {}", h[4]),
            }
        }
        names
    });
    let mut wire =
        Pipeline::over(client, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    // Scripted attachment stands in for an already completed auth conversation.
    let (tag, root) = wire.attach(b"", b"").unwrap();
    wire.wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap();
    (wire, root, job)
}
#[test]
fn startup_preserves_root_and_reads_exact_api_and_limits() {
    let (wire, root, peer) = fixture(0);
    let mut opening = Opening::from_attached(wire, root, 0, 100).unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    while !opening.ready() {
        assert!(Instant::now() < end);
        opening.dispatch(1, 1).unwrap();
        thread::yield_now();
    }
    assert_eq!(opening.deadline(), None);
    opening.dispatch(1000, 512).unwrap();
    let opened = opening.into_opened().unwrap();
    assert_eq!(opened.epoch, 7);
    assert_eq!(opened.pipeline.fid_count(), 1);
    assert!(opened.pipeline.is_quiescent());
    drop(opened);
    assert_eq!(peer.join().unwrap(), ["api", "limits"]);
}
#[test]
fn deadline_directory_version_and_clunk_errors_close() {
    let (wire, _, peer) = fixture(0);
    assert!(matches!(
        Opening::from_attached(wire, sophia_9p_records::Fid::NOFID, 0, 100),
        Err(Error::Invalid)
    ));
    assert!(peer.join().unwrap().is_empty());
    for fault in 1..=7 {
        let (wire, root, peer) = fixture(fault);
        let mut opening = Opening::from_attached(wire, root, 0, 100).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while opening.error().is_none() {
            assert!(Instant::now() < end);
            let _ = opening.dispatch(1, 4096);
            thread::yield_now();
        }
        assert!(!opening.ready() && opening.fd().is_none());
        assert!(opening.into_opened().is_err());
        peer.join().unwrap();
    }
    let (wire, root, peer) = fixture(0);
    let mut opening = Opening::from_attached(wire, root, 10, 20).unwrap();
    assert_eq!(opening.dispatch(9, 512), Err(Error::ClockRegression));
    assert!(opening.error().is_none());
    assert_eq!(opening.dispatch(20, 512), Err(Error::Deadline));
    assert!(peer.join().unwrap().is_empty());
}

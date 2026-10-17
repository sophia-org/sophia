use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_portal_client::{
    frame::Frame,
    protocol::{Api, RequestCapability},
    startup::Opened,
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};
fn send(s: &mut UnixStream, kind: u8, tag: &[u8], body: &[u8]) {
    let mut bytes = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    bytes.push(kind);
    bytes.extend(tag);
    bytes.extend(body);
    s.write_all(&bytes).unwrap();
}
fn qid(dir: bool) -> Vec<u8> {
    let mut b = vec![0; 13];
    b[0] = if dir { 128 } else { 0 };
    b
}
fn fixture(fault: u8) -> (Opened, thread::JoinHandle<()>) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let job = thread::spawn(move || {
        let mut files = HashMap::new();
        let mut metadata_reads = 0;
        let record: Vec<_> = [1u64, 7, 9, 11, 20, 10, 80, 1]
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .collect();
        loop {
            let mut h = [0; 7];
            if let Err(e) = peer.read_exact(&mut h) {
                assert_eq!(e.kind(), std::io::ErrorKind::UnexpectedEof);
                break;
            }
            let mut b = vec![0; u32::from_le_bytes(h[..4].try_into().unwrap()) as usize - 7];
            peer.read_exact(&mut b).unwrap();
            let fid = u32::from_le_bytes(b[..4].try_into().unwrap());
            match h[4] {
                100 => send(&mut peer, 101, &h[5..], &b),
                104 => send(&mut peer, 105, &h[5..], &qid(true)),
                110 => {
                    let name = String::from_utf8(b[12..].to_vec()).unwrap();
                    assert!(name == "frame" || name == "frame-info");
                    files.insert(u32::from_le_bytes(b[4..8].try_into().unwrap()), name);
                    let mut r = vec![1, 0];
                    r.extend(qid(false));
                    send(&mut peer, 111, &h[5..], &r);
                }
                12 => {
                    assert_eq!(&b[4..8], &[0; 4]);
                    let mut r = qid(false);
                    r.extend(128u32.to_le_bytes());
                    send(&mut peer, 13, &h[5..], &r);
                }
                116 => {
                    let offset = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
                    let count = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                    assert!(count <= 128);
                    let data = if files[&fid] == "frame-info" {
                        if offset == 0 {
                            metadata_reads += 1;
                        }
                        if fault == 1 && metadata_reads == 2 {
                            send(&mut peer, 7, &h[5..], &116u32.to_le_bytes());
                            continue;
                        }
                        let mut data = record[offset..record.len().min(offset + count)].to_vec();
                        if fault == 2 && metadata_reads == 2 && offset == 0 {
                            data[24] = 12;
                        }
                        data
                    } else {
                        let length = if fault == 3 { 801 } else { 800 };
                        if fault == 4 && offset == 0 {
                            vec![]
                        } else {
                            vec![17; count.min(length - offset).min(17)]
                        }
                    };
                    let mut r = (data.len() as u32).to_le_bytes().to_vec();
                    r.extend(data);
                    send(&mut peer, 117, &h[5..], &r);
                    if fault == 4 && files[&fid] == "frame" && offset == 0 {
                        break;
                    }
                }
                120 => {
                    if fault == 5 {
                        send(&mut peer, 7, &h[5..], &5u32.to_le_bytes());
                    } else {
                        send(&mut peer, 121, &h[5..], &[]);
                    }
                }
                _ => panic!("unexpected {}", h[4]),
            }
        }
    });
    let mut pipeline = Pipeline::over(
        client,
        PipelineLimits {
            msize: 4096,
            ..PipelineLimits::default()
        },
        Duration::from_secs(2),
    )
    .unwrap();
    let (tag, root) = pipeline.attach(b"", b"").unwrap();
    pipeline
        .wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap();
    (
        Opened {
            pipeline,
            root,
            info: Api {
                epoch: 7,
                request: RequestCapability::Unavailable,
            },
            now: 0,
        },
        job,
    )
}
#[test]
fn retained_frame_requires_eof_metadata_recheck_and_clunks() {
    for fault in 0..=5 {
        let (opened, peer) = fixture(fault);
        let storage = Vec::with_capacity(800);
        let pointer = storage.as_ptr();
        let mut frame = Frame::new(opened, storage, 0, 2000).unwrap();
        let start = Instant::now();
        while frame.result().is_none() && frame.error().is_none() {
            assert!(start.elapsed() < Duration::from_secs(2));
            let _ = frame.dispatch(
                start.elapsed().as_millis() as u64,
                if fault == 0 { 1 } else { 65536 },
            );
            thread::yield_now();
        }
        if fault == 0 {
            let (opened, info, pixels) = frame.finish().unwrap();
            assert_eq!(pixels.as_ptr(), pointer);
            assert_eq!(info.image_bytes, 800);
            assert_eq!(pixels, vec![17; 800]);
            assert_eq!(opened.pipeline.fid_count(), 1);
            let mut again = Frame::new(opened, pixels, frame_time(start), 2000).unwrap();
            while again.result().is_none() {
                again.dispatch(frame_time(start), 65536).unwrap();
                thread::yield_now();
            }
            let (opened, _, pixels) = again.finish().unwrap();
            assert_eq!(pixels.as_ptr(), pointer);
            assert_eq!(pixels, vec![17; 800]);
            drop(opened);
        } else {
            assert!(frame.result().is_none() && frame.fd().is_none());
            drop(frame);
        }
        peer.join().unwrap();
    }
}
fn frame_time(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}
#[test]
fn clock_deadline_and_cancellation_hide_results() {
    for cancel in [false, true] {
        let (opened, peer) = fixture(0);
        let mut frame = Frame::new(opened, vec![], 10, 20).unwrap();
        assert_eq!(
            frame.dispatch(9, 1),
            Err(sophia_portal_client::frame::Error::ClockRegression)
        );
        assert!(frame.error().is_none());
        if cancel {
            frame.close();
        } else {
            assert_eq!(
                frame.dispatch(20, 1),
                Err(sophia_portal_client::frame::Error::Deadline)
            );
        }
        assert!(frame.result().is_none() && frame.fd().is_none());
        peer.join().unwrap();
    }
}

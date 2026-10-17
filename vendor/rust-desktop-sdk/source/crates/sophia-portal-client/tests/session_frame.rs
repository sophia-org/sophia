use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_portal_client::{
    protocol::{Api, RequestCapability},
    session::{Error, Session, WatchEvent},
    snapshot::File,
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
        let mut watch = None;
        let (mut versions, mut attaches, mut flushes) = (0, 0, 0);
        let mut first_reads = Vec::new();
        let mut reversed = false;
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
            let fid = b
                .get(..4)
                .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
                .unwrap_or(u32::MAX);
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
                    let name = String::from_utf8(b[12..].to_vec()).unwrap();
                    assert!(name == "frame" || name == "frame-info" || name == "watch");
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
                108 => {
                    assert_eq!(b, watch.take().unwrap());
                    flushes += 1;
                    send(&mut peer, 109, &h[5..], &[]);
                }
                116 => {
                    let offset = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
                    let count = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                    assert!(count <= 128);
                    if files[&fid] == "watch" {
                        assert_eq!((offset, count), (0, 1));
                        assert!(watch.replace(h[5..].to_vec()).is_none());
                        continue;
                    }
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
                    if fault == 0 && files[&fid] == "frame" && !reversed {
                        first_reads.push((h[5..].to_vec(), r));
                        if first_reads.len() == 4 {
                            for (tag, body) in first_reads.drain(..).rev() {
                                send(&mut peer, 117, &tag, &body);
                            }
                            reversed = true;
                        }
                    } else {
                        send(&mut peer, 117, &h[5..], &r);
                    }
                    if fault == 4 && files[&fid] == "frame" && offset == 0 {
                        break;
                    }
                }
                120 => {
                    if fault == 5 && files[&fid] != "watch" {
                        send(&mut peer, 7, &h[5..], &5u32.to_le_bytes());
                    } else {
                        send(&mut peer, 121, &h[5..], &[]);
                    }
                }
                _ => panic!("unexpected {}", h[4]),
            }
        }
        if fault == 0 {
            assert!(reversed);
            assert_eq!(flushes, 2);
        }
    });
    let mut pipeline = Pipeline::over(
        client,
        PipelineLimits {
            msize: 4096,
            max_outstanding: 6,
            max_fids: 4,
            max_buffered_input: 4 * 4096 + 22,
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
fn shared_frame_keeps_window_cancels_watch_and_reuses_storage() {
    for fault in 0..=5 {
        let (opened, peer) = fixture(fault);
        let mut session = Session::new(opened).unwrap();
        let mut storage = Some(vec![0xa5; 800]);
        let pointer = storage.as_ref().unwrap().as_ptr();
        let until = Instant::now() + Duration::from_secs(2);
        for _ in 0..if fault == 0 { 2 } else { 1 } {
            session.watch(2000).unwrap();
            while !session.watch_waiting() {
                assert!(Instant::now() < until);
                session.dispatch(1, 1).unwrap();
                thread::yield_now();
            }
            let mut image = storage.take().unwrap();
            let original = image.clone();
            let capacity = image.capacity();
            assert_eq!(
                session.frame_buffer(&mut image, 1),
                Err(Error::Frame(sophia_portal_client::frame::Error::Deadline))
            );
            assert_eq!(image, original);
            assert_eq!(image.as_ptr(), pointer);
            assert_eq!(image.capacity(), capacity);
            session.frame_buffer(&mut image, 2000).unwrap();
            assert!(image.is_empty());
            assert_eq!(image.capacity(), 0);
            let mut rejected = vec![0x5a; 800];
            let rejected_pointer = rejected.as_ptr();
            let rejected_capacity = rejected.capacity();
            assert_eq!(session.frame_buffer(&mut rejected, 2000), Err(Error::Busy));
            assert_eq!(rejected, vec![0x5a; 800]);
            assert_eq!(rejected.as_ptr(), rejected_pointer);
            assert_eq!(rejected.capacity(), rejected_capacity);
            assert_eq!(session.snapshot(File::Status, 2000), Err(Error::Busy));
            assert_eq!(session.take_frame_scratch(), Err(Error::Busy));
            let mut cancelled = false;
            while session.error().is_none()
                && (session.frame_result().is_none() || session.watch_event().is_none())
            {
                assert!(Instant::now() < until);
                let _ = session.dispatch(1, if fault == 0 { 1 } else { 65536 });
                if fault == 0 && !cancelled && session.frame_received() > 0 {
                    assert!(session.frame_received() < 800 && session.frame_result().is_none());
                    session.cancel_watch().unwrap();
                    cancelled = true;
                }
                thread::yield_now();
            }
            if fault == 0 {
                assert!(cancelled);
                assert_eq!(session.watch_event(), Some(WatchEvent::Cancelled));
                let (info, pixels) = session.take_frame().unwrap();
                assert_eq!(info.image_bytes, 800);
                assert_eq!(pixels.as_ptr(), pointer);
                assert_eq!(pixels, vec![17; 800]);
                storage = Some(pixels);
                session.release_watch().unwrap();
                assert_eq!(session.deadline(), None);
            } else {
                assert!(session.frame_result().is_none() && session.fd().is_none());
                assert!(session.watch_event().is_none());
                let scratch = session.take_frame_scratch().unwrap();
                assert_eq!(scratch.as_ptr(), pointer);
            }
        }
        if fault == 0 {
            let role = session.finish().unwrap();
            assert_eq!(role.pipeline.fid_count(), 1);
            assert!(role.pipeline.is_quiescent());
            drop(role);
        } else {
            assert!(session.finish().is_err());
        }
        peer.join().unwrap();
    }
}

use rustix::net::*;
use sophia_lock_client::protocol::lock_files::*;
use std::{
    collections::HashMap,
    io::{IoSliceMut, Read, Write},
    os::{
        fd::{AsFd, OwnedFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

pub struct Peer {
    pub stream: UnixStream,
    pub dir: PathBuf,
    pub thread: thread::JoinHandle<usize>,
}
fn n(b: &[u8], at: usize, len: usize) -> u64 {
    b[at..at + len]
        .iter()
        .enumerate()
        .fold(0, |v, (i, b)| v | (u64::from(*b) << (8 * i)))
}
fn send(s: &mut UnixStream, kind: u8, tag: u16, body: &[u8]) {
    let mut b = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    b.push(kind);
    b.extend(tag.to_le_bytes());
    b.extend(body);
    if let Err(error) = s.write_all(&b) {
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
            ),
            "peer write: {error}"
        );
    }
}
fn read(s: &mut UnixStream, tag: u16, b: &[u8]) {
    let mut out = (b.len() as u32).to_le_bytes().to_vec();
    out.extend(b);
    send(s, 117, tag, &out);
}
fn qid() -> Vec<u8> {
    let mut b = vec![0; 5];
    b.extend(90u64.to_le_bytes());
    b
}
fn record(kind: u16, seq: u64, body: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 32];
    b[..4].copy_from_slice(&((32 + body.len()) as u32).to_le_bytes());
    b[4] = 1;
    b[6..8].copy_from_slice(&kind.to_le_bytes());
    b[8] = 7;
    b[24..32].copy_from_slice(&seq.to_le_bytes());
    b.extend(body);
    b
}
fn append(j: &mut Vec<u8>, seq: &mut u64, kind: u16, body: &[u8]) {
    *seq += 1;
    j.extend(record(kind, *seq, body));
}
fn files() -> HashMap<String, Vec<u8>> {
    let mut map = HashMap::new();
    map.insert(
        "api".into(),
        b"sophia-lock-files version=1 epoch=7\n".to_vec(),
    );
    let mut limits = vec![1, 0, 16, 0, 4, 0, 8, 0];
    limits.extend(64u32.to_le_bytes());
    limits.extend(64u32.to_le_bytes());
    limits.extend(1024u64.to_le_bytes());
    limits.extend([32, 0, 1, 0]);
    for v in [64u32, 16384, 12000, 2000, 0] {
        limits.extend(v.to_le_bytes());
    }
    map.insert("limits".into(), record(1, 0, &limits));
    let d = LockImageDiscovery {
        connection_epoch: 7,
        channel_generation: 3,
        endpoint: "image.sock".into(),
        source_kinds: 1,
        max_offers: 4,
        max_packets_per_turn: 1,
        hello_timeout_ms: 1000,
        validation_timeout_ms: 2000,
        validation_chunk_bytes: 65536,
        capture_interest_timeout_ms: 500,
        formats_generation: 1,
        limits_generation: 1,
    };
    map.insert("image-channel".into(), d.encode().unwrap().into_bytes());
    let l = LockImageLimits {
        channel_generation: 3,
        limits_generation: 1,
        max_resource_bytes: 1024,
        max_sealed_backing_bytes: 4096,
        max_dma_source_bytes: 0,
        max_renderer_allocation_bytes: 0,
        max_renderer_entries: 0,
        max_offers: 4,
        max_validation_jobs: 4,
        max_capture_jobs: 0,
        max_admitted_outputs: 16,
        max_format_rows_per_output: 1,
        max_width: 64,
        max_height: 64,
        validation_timeout_ms: 2000,
        capture_interest_timeout_ms: 500,
        validation_chunk_bytes: 65536,
        max_packets_per_turn: 1,
    };
    map.insert(
        "image-limits".into(),
        record(0x7005, 0, &l.encode().unwrap()),
    );
    let f = LockImageFormats {
        channel_generation: 3,
        formats_generation: 1,
        limits_generation: 1,
        rows: vec![],
    };
    map.insert(
        "image-formats".into(),
        record(0x7006, 0, &f.encode().unwrap()),
    );
    map
}
pub fn start(absent: bool, cancel: bool) -> Peer {
    start_case(
        if absent {
            Case::ReadError(2)
        } else {
            Case::Normal
        },
        cancel,
    )
}
#[derive(Clone, Copy)]
pub enum Case {
    Normal,
    ReadError(u32),
    Malformed,
    Oversized,
    ClunkError(u32),
    Contradiction,
    Unavailable,
}
pub fn start_case(case: Case, cancel: bool) -> Peer {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let dir = std::env::temp_dir().join(format!(
        "lock-image-peer-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&dir).unwrap();
    let listener = socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
        None,
    )
    .unwrap();
    bind(
        &listener,
        &SocketAddrUnix::new(dir.join("image.sock")).unwrap(),
    )
    .unwrap();
    listen(&listener, 4).unwrap();
    let (client, mut stream) = UnixStream::pair().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut objects = files();
        if matches!(case, Case::Malformed) {
            objects.insert("image-channel".into(), b"malformed\n".to_vec());
        }
        if matches!(case, Case::Oversized) {
            objects.get_mut("image-formats").unwrap().push(0);
        }
        let mut fids = HashMap::new();
        let mut journal = vec![];
        let mut seq = 0;
        let mut tx = vec![];
        let mut pending = None;
        let mut side: Option<OwnedFd> = None;
        let mut sources = 0;
        let mut image_begin = None;
        let mut cancel_release: Option<(u64, LockImageMetadata)> = None;
        loop {
            assert!(Instant::now() < deadline, "peer deadline");
            let side_fd = side.as_ref().map(AsFd::as_fd).unwrap_or(listener.as_fd());
            let mut polls = [
                rustix::event::PollFd::new(&stream, rustix::event::PollFlags::IN),
                rustix::event::PollFd::new(&side_fd, rustix::event::PollFlags::IN),
            ];
            rustix::event::poll(
                &mut polls,
                Some(&rustix::event::Timespec {
                    tv_sec: 0,
                    tv_nsec: 10_000_000,
                }),
            )
            .unwrap();
            let socket_ready = polls[1].revents().contains(rustix::event::PollFlags::IN);
            let wire_ready = !polls[0].revents().is_empty();
            if socket_ready {
                if let Some(fd) = &side {
                    let mut bytes = [0; 200];
                    let mut control = [std::mem::MaybeUninit::uninit(); 128];
                    let mut ancillary = RecvAncillaryBuffer::new(&mut control);
                    let mut iov = [IoSliceMut::new(&mut bytes)];
                    let got = recvmsg(
                        fd,
                        &mut iov,
                        &mut ancillary,
                        RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
                    )
                    .unwrap();
                    if got.bytes == 0 {
                        side = None;
                    } else {
                        let rights: Vec<_> = ancillary
                            .drain()
                            .flat_map(|m| match m {
                                RecvAncillaryMessage::ScmRights(r) => r.collect::<Vec<_>>(),
                                _ => panic!("unexpected ancillary"),
                            })
                            .collect();
                        match LockImagePacket::decode(&bytes[..got.bytes]).unwrap() {
                            LockImagePacket::Hello { .. } => {
                                assert!(rights.is_empty());
                                append(
                                    &mut journal,
                                    &mut seq,
                                    0x7003,
                                    &LockChannelReady {
                                        channel_generation: 3,
                                        granted_source_kinds: 1,
                                    }
                                    .encode()
                                    .unwrap(),
                                );
                                if matches!(case, Case::Contradiction) {
                                    append(
                                        &mut journal,
                                        &mut seq,
                                        0x7002,
                                        &LockSourceReleased {
                                            offer_id: 99,
                                            resource: 21,
                                            resource_generation: 1,
                                            disposition: LockSourceDisposition::RefusedBeforeUse,
                                        }
                                        .encode()
                                        .unwrap(),
                                    );
                                }
                            }
                            LockImagePacket::Source { metadata: m, .. } => {
                                assert_eq!(rights.len(), 1);
                                assert!(
                                    rustix::io::fcntl_getfd(&rights[0])
                                        .unwrap()
                                        .contains(rustix::io::FdFlags::CLOEXEC)
                                );
                                sources += 1;
                                let status = LockResourceStatus {
                                    transaction: m.offer_id,
                                    resource: LockResourceId {
                                        id: m.resource,
                                        generation: m.resource_generation,
                                    },
                                    status: LockResourceState::Accepted,
                                    reason: 0,
                                    admitted_bytes: m.declared_bytes,
                                };
                                append(&mut journal, &mut seq, 33, &status.encode().unwrap());
                                append(
                                    &mut journal,
                                    &mut seq,
                                    0x7002,
                                    &LockSourceReleased {
                                        offer_id: m.offer_id,
                                        resource: m.resource,
                                        resource_generation: m.resource_generation,
                                        disposition: LockSourceDisposition::Retained,
                                    }
                                    .encode()
                                    .unwrap(),
                                );
                            }
                        }
                    }
                } else {
                    side = Some(
                        accept_with(&listener, SocketFlags::CLOEXEC | SocketFlags::NONBLOCK)
                            .unwrap(),
                    );
                }
            }
            if wire_ready {
                let mut h = [0; 7];
                match stream.read_exact(&mut h) {
                    Ok(()) => (),
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                        ) =>
                    {
                        break;
                    }
                    Err(e) => panic!("{e}"),
                };
                let mut b = vec![0; n(&h, 0, 4) as usize - 7];
                stream.read_exact(&mut b).unwrap();
                let tag = n(&h, 5, 2) as u16;
                let fid = if b.len() >= 4 { n(&b, 0, 4) as u32 } else { 0 };
                match h[4] {
                    100 => send(&mut stream, 101, tag, &b),
                    104 => {
                        fids.insert(fid, "root".to_owned());
                        send(&mut stream, 105, tag, &qid());
                    }
                    110 => {
                        let len = n(&b, 10, 2) as usize;
                        let name = std::str::from_utf8(&b[12..12 + len]).unwrap();
                        if let Case::ReadError(errno) = case
                            && name == "image-channel"
                        {
                            send(&mut stream, 7, tag, &errno.to_le_bytes());
                        } else {
                            fids.insert(n(&b, 4, 4) as u32, name.to_owned());
                            let mut out = vec![1, 0];
                            out.extend(qid());
                            send(&mut stream, 111, tag, &out);
                        }
                    }
                    12 => {
                        if fids[&fid] == "transaction" {
                            tx.clear();
                        }
                        let mut out = qid();
                        out.extend(0u32.to_le_bytes());
                        send(&mut stream, 13, tag, &out);
                    }
                    116 => {
                        let offset = n(&b, 4, 8) as usize;
                        let count = n(&b, 12, 4) as usize;
                        if fids[&fid] == "events" {
                            pending = Some((tag, offset, count));
                        } else {
                            let data = &objects[&fids[&fid]];
                            read(
                                &mut stream,
                                tag,
                                &data[offset..data.len().min(offset + count)],
                            );
                        }
                    }
                    118 => {
                        let data = &b[16..];
                        match fids[&fid].as_str() {
                            "transaction" => {
                                assert_eq!(n(&b, 4, 8) as usize, tx.len());
                                tx.extend(data);
                            }
                            "ack" => {
                                if let Some((sequence, m)) = cancel_release
                                    && n(data, 8, 8) >= sequence
                                {
                                    cancel_release = None;
                                    append(
                                        &mut journal,
                                        &mut seq,
                                        33,
                                        &LockResourceStatus {
                                            transaction: m.offer_id,
                                            resource: LockResourceId {
                                                id: m.resource,
                                                generation: m.resource_generation,
                                            },
                                            status: LockResourceState::Cancelled,
                                            reason: 0,
                                            admitted_bytes: 0,
                                        }
                                        .encode()
                                        .unwrap(),
                                    );
                                    append(
                                        &mut journal,
                                        &mut seq,
                                        0x7002,
                                        &LockSourceReleased {
                                            offer_id: m.offer_id,
                                            resource: m.resource,
                                            resource_generation: m.resource_generation,
                                            disposition:
                                                LockSourceDisposition::CancelledBeforeImport,
                                        }
                                        .encode()
                                        .unwrap(),
                                    );
                                }
                            }
                            "submit" => {
                                let id = n(data, 8, 8);
                                let kind = n(&tx, 6, 2) as u16;
                                let mut receipt = id.to_le_bytes().to_vec();
                                receipt.extend(kind.to_le_bytes());
                                receipt.extend([0; 6]);
                                append(&mut journal, &mut seq, 18, &receipt);
                                match kind {
                                    256 => {
                                        append(
                                            &mut journal,
                                            &mut seq,
                                            16,
                                            &LockNegotiated {
                                                granted_chords: 0,
                                                granted_capabilities: if matches!(
                                                    case,
                                                    Case::Unavailable
                                                ) {
                                                    1
                                                } else {
                                                    5
                                                },
                                            }
                                            .encode_at_revision(2)
                                            .unwrap(),
                                        );
                                    }
                                    0x7000 => {
                                        image_begin =
                                            Some(LockImageMetadata::decode(&tx[32..]).unwrap());
                                        if !cancel {
                                            append(&mut journal, &mut seq, 0x7001, &tx[32..]);
                                        }
                                    }
                                    260 if cancel => {
                                        let m = image_begin.unwrap();
                                        assert_eq!(n(&tx, 32, 8), m.offer_id);
                                        assert_eq!(n(&tx, 40, 8), m.resource);
                                        append(
                                            &mut journal,
                                            &mut seq,
                                            0x7001,
                                            &m.encode().unwrap(),
                                        );
                                        cancel_release = Some((seq, m));
                                    }
                                    _ => panic!("candidate {kind}"),
                                }
                            }
                            _ => panic!("write"),
                        };
                        send(&mut stream, 119, tag, &(data.len() as u32).to_le_bytes());
                    }
                    120 => {
                        let name = fids.remove(&fid);
                        if let Case::ClunkError(errno) = case
                            && name.as_deref() == Some("image-channel")
                        {
                            send(&mut stream, 7, tag, &errno.to_le_bytes());
                        } else {
                            send(&mut stream, 121, tag, &[]);
                        }
                    }
                    _ => panic!("request {}", h[4]),
                }
            }
            if let Some((tag, offset, count)) = pending
                && offset < journal.len()
            {
                read(
                    &mut stream,
                    tag,
                    &journal[offset..journal.len().min(offset + count)],
                );
                pending = None;
            }
        }
        sources
    });
    Peer {
        stream: client,
        dir,
        thread,
    }
}

use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_output_client::{
    protocol::OutputV1ClientHello,
    startup::{Error, Opening},
};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    Api,
    ExcessApi,
    Limits,
    SubmitUnit,
    AckUnit,
    Directory,
}

fn send(s: &mut UnixStream, kind: u8, tag: u16, body: &[u8]) {
    let mut bytes = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    bytes.push(kind);
    bytes.extend(tag.to_le_bytes());
    bytes.extend(body);
    s.write_all(&bytes).unwrap();
}
fn qid(directory: bool) -> Vec<u8> {
    let mut qid = vec![if directory { 0x80 } else { 0 }];
    qid.extend(0u32.to_le_bytes());
    qid.extend(1u64.to_le_bytes());
    qid
}
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
fn fixture(fault: Fault) -> (Pipeline, thread::JoinHandle<Vec<String>>) {
    fixture_limits(fault, PipelineLimits::default())
}
fn fixture_limits(
    fault: Fault,
    bounds: PipelineLimits,
) -> (Pipeline, thread::JoinHandle<Vec<String>>) {
    let (client, mut server) = UnixStream::pair().unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let peer = thread::spawn(move || {
        let mut names = Vec::new();
        let mut versions = 0;
        let mut attaches = 0;
        let mut object = Vec::new();
        let mut current = String::new();
        loop {
            let mut header = [0; 7];
            match server.read_exact(&mut header) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => panic!("peer read: {e}"),
            }
            let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
            let tag = u16::from_le_bytes(header[5..].try_into().unwrap());
            let mut body = vec![0; size - 7];
            server.read_exact(&mut body).unwrap();
            match header[4] {
                100 => {
                    versions += 1;
                    assert_eq!(versions, 1);
                    assert!(names.is_empty());
                    send(&mut server, 101, tag, &body);
                }
                104 => {
                    attaches += 1;
                    assert_eq!(attaches, 1);
                    assert!(names.is_empty());
                    send(&mut server, 105, tag, &qid(true));
                }
                110 => {
                    assert_eq!(u16::from_le_bytes(body[8..10].try_into().unwrap()), 1);
                    let n = u16::from_le_bytes(body[10..12].try_into().unwrap()) as usize;
                    current = String::from_utf8(body[12..12 + n].to_vec()).unwrap();
                    names.push(current.clone());
                    object = match current.as_str() {
                        "api" if fault == Fault::Api => {
                            b"sophia-wm-files version=1 output_transport=current_ipc\n".to_vec()
                        }
                        "api" if fault == Fault::ExcessApi => vec![b'x'; 257],
                        "api" => b"sophia-output-files version=1\n".to_vec(),
                        "limits" => {
                            let mut b = limits();
                            if fault == Fault::Limits {
                                b[48] = 7;
                            }
                            b
                        }
                        "events" | "submit" | "ack" => vec![],
                        _ => panic!("unexpected file {current}"),
                    };
                    let mut b = 1u16.to_le_bytes().to_vec();
                    b.extend(qid(fault == Fault::Directory));
                    send(&mut server, 111, tag, &b);
                }
                12 => {
                    let flags = u32::from_le_bytes(body[4..8].try_into().unwrap());
                    assert_eq!(flags, u32::from(current == "submit" || current == "ack"));
                    let unit: u32 = match current.as_str() {
                        "api" | "limits" => 7,
                        "submit" if fault == Fault::SubmitUnit => 23,
                        "ack" if fault == Fault::AckUnit => 15,
                        _ => 0,
                    };
                    let mut b = qid(false);
                    b.extend(unit.to_le_bytes());
                    send(&mut server, 13, tag, &b);
                }
                116 => {
                    assert!(current == "api" || current == "limits");
                    let offset = u64::from_le_bytes(body[4..12].try_into().unwrap()) as usize;
                    let count = u32::from_le_bytes(body[12..16].try_into().unwrap()) as usize;
                    assert!(count <= 7);
                    let slice = &object[offset..object.len().min(offset + count)];
                    let mut b = (slice.len() as u32).to_le_bytes().to_vec();
                    b.extend(slice);
                    send(&mut server, 117, tag, &b);
                }
                120 => send(&mut server, 121, tag, &[]),
                _ => panic!("unexpected request {}", header[4]),
            }
        }
        names
    });
    let wire = Pipeline::over(client, bounds, Duration::from_secs(2)).unwrap();
    (wire, peer)
}
fn offer() -> OutputV1ClientHello {
    OutputV1ClientHello {
        minimum_revision: 1,
        maximum_revision: 1,
        capabilities: 1,
    }
}
fn drive(opening: &mut Opening) -> Result<(), Error> {
    let end = Instant::now() + Duration::from_secs(2);
    while !opening.is_opened() {
        assert!(Instant::now() < end);
        opening.dispatch(11, 512)?;
        thread::yield_now();
    }
    Ok(())
}

#[test]
fn direct_startup_reads_fragmented_objects_and_retains_only_role_fids() {
    let (wire, peer) = fixture(Fault::None);
    let mut opening = Opening::direct(wire, offer(), 10, 1000).unwrap();
    drive(&mut opening).unwrap();
    let opened = opening.into_opened().unwrap();
    assert_eq!(opened.epoch, 7);
    assert_eq!(opened.pipeline.fid_count(), 4);
    assert_eq!(opened.pipeline.outstanding(), 0);
    assert_eq!(opened.deadline, 1000);
    drop(opened);
    assert_eq!(
        peer.join().unwrap(),
        ["api", "limits", "events", "submit", "ack"]
    );
}

#[test]
fn attached_startup_never_resets_or_reattaches() {
    let (mut wire, peer) = fixture(Fault::None);
    let (tag, root) = wire.attach(b"output", b"output-authority").unwrap();
    assert!(matches!(
        wire.wait(tag, Instant::now() + Duration::from_secs(2))
            .unwrap(),
        Reply::Attach(_)
    ));
    let mut opening = Opening::attached(wire, root, offer(), 10, 1000).unwrap();
    drive(&mut opening).unwrap();
    let opened = opening.into_opened().unwrap();
    assert_eq!(opened.root, root);
    drop(opened);
    assert_eq!(peer.join().unwrap().len(), 5);
}

#[test]
fn malformed_files_and_small_control_units_close_without_fallback() {
    for fault in [
        Fault::Api,
        Fault::ExcessApi,
        Fault::Limits,
        Fault::SubmitUnit,
        Fault::AckUnit,
        Fault::Directory,
    ] {
        let (wire, peer) = fixture(fault);
        let mut opening = Opening::direct(wire, offer(), 10, 1000).unwrap();
        assert!(matches!(drive(&mut opening), Err(Error::Protocol(_))));
        assert!(opening.fd().is_none());
        assert!(!opening.is_opened());
        peer.join().unwrap();
    }
}

#[test]
fn clock_regression_is_nonmutating_and_expiry_sends_nothing() {
    let (wire, peer) = fixture(Fault::None);
    let mut opening = Opening::direct(wire, offer(), 10, 20).unwrap();
    assert_eq!(opening.dispatch(9, 512), Err(Error::ClockRegression));
    assert!(opening.fd().is_some());
    opening.dispatch(11, 0).unwrap();
    assert_eq!(opening.dispatch(20, 512), Err(Error::Deadline));
    assert!(opening.fd().is_none());
    assert!(peer.join().unwrap().is_empty());
}

#[test]
fn missing_observe_refuses_without_io_but_revision_refusal_belongs_to_negotiation() {
    let (wire, peer) = fixture(Fault::None);
    let mut requested = offer();
    requested.capabilities = 0;
    assert!(Opening::direct(wire, requested, 10, 1000).is_err());
    assert!(peer.join().unwrap().is_empty());
    let (wire, peer) = fixture(Fault::None);
    requested = offer();
    requested.minimum_revision = 9;
    requested.maximum_revision = 10;
    let mut opening = Opening::direct(wire, requested, 10, 1000).unwrap();
    drive(&mut opening).unwrap();
    let opened = opening.into_opened().unwrap();
    assert_eq!(opened.offer, requested);
    drop(opened);
    assert_eq!(peer.join().unwrap().len(), 5);
}

#[test]
fn undersized_pipelines_are_refused_before_opening_files() {
    let default = PipelineLimits::default();
    for bounds in [
        PipelineLimits {
            max_outstanding: 7,
            ..default
        },
        PipelineLimits {
            max_fids: 15,
            ..default
        },
        PipelineLimits {
            max_buffered_input: 65536,
            ..default
        },
        PipelineLimits {
            msize: 131072,
            ..default
        },
    ] {
        let (wire, peer) = fixture_limits(Fault::None, bounds);
        assert!(matches!(
            Opening::direct(wire, offer(), 10, 1000),
            Err(Error::Protocol(_))
        ));
        assert!(peer.join().unwrap().is_empty());
    }
}

#[test]
fn attached_startup_rejects_wrong_root_and_unsettled_requests() {
    let (mut wire, peer) = fixture(Fault::None);
    let (tag, root) = wire.attach(b"output", b"output-authority").unwrap();
    wire.wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert!(wire.owns_fid(root));
    assert!(matches!(
        Opening::attached(wire, sophia_9p_records::Fid(999), offer(), 10, 1000),
        Err(Error::Protocol(_))
    ));
    assert!(peer.join().unwrap().is_empty());
    let (mut wire, peer) = fixture(Fault::None);
    let (_, root) = wire.attach(b"output", b"output-authority").unwrap();
    assert!(matches!(
        Opening::attached(wire, root, offer(), 10, 1000),
        Err(Error::Protocol(_))
    ));
    assert!(peer.join().unwrap().is_empty());
}

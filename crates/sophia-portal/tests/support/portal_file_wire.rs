use super::*;
use sophia_9p::unix::Server;
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_9p_client::records::{Fid, Tag};
use std::os::unix::net::UnixStream;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Owner {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
    }
}

#[test]
fn requester_client_reports_recipient_outcome_and_never_uploads_a_denial() {
    use sophia_portal::file_client::PortalFileClient;
    for (allowed, succeeded) in [(true, true), (true, false), (false, false)] {
        let export = PortalFileExport::new(
            Authority {
                revision: 1,
                rights: true,
                allow: allowed,
                live: true,
                now: 100,
                generation: 9,
            },
            PortalFileLimits::defaults(1),
        )
        .unwrap();
        let mut server = Server::new(export, sophia_9p::Limits::default()).unwrap();
        let (owner, stream) = UnixStream::pair().unwrap();
        let connection = server
            .adopt(owner)
            .unwrap_or_else(|_| panic!("adopt requester"));
        server
            .export_mut()
            .admit(PortalRequesterAdmission {
                connection,
                custody: PortalRequesterCustody::SessionXAuthority,
                epoch: 1,
                source: NamespaceId::from_raw(1),
                target: NamespaceId::from_raw(2),
            })
            .unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let effects = Arc::new(Mutex::new(Vec::new()));
        let recorded = effects.clone();
        let _owner = Owner {
            stop,
            thread: Some(std::thread::spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    server.export_mut().expire(Instant::now());
                    server.turn(Some(Duration::from_millis(5))).unwrap();
                    if let Some(work) = server.export_mut().take_execution() {
                        recorded.lock().unwrap().push(work.payload);
                        server
                            .export_mut()
                            .settle_execution(work.grant.transfer, succeeded)
                            .unwrap();
                    }
                }
            })),
        };
        let mut client = PortalFileClient::over(stream, 1, Duration::from_secs(2)).unwrap();
        let PortalFileCandidate::TransferRequest(request) = request(1) else {
            unreachable!()
        };
        let decision = client.request(request).unwrap();
        assert_eq!(
            matches!(decision, PortalBrokerResponseDecision::Allowed(_)),
            allowed
        );
        assert!(effects.lock().unwrap().is_empty());
        let payload = vec![b'x'; 65_536];
        if allowed {
            assert_eq!(
                client.upload(&payload).unwrap(),
                if succeeded {
                    PortalGrantState::Completed
                } else {
                    PortalGrantState::Revoked
                }
            );
            assert_eq!(*effects.lock().unwrap(), [payload]);
            assert!(client.upload(b"duplicate").is_err());
        } else {
            assert!(client.upload(&payload).is_err());
            assert!(effects.lock().unwrap().is_empty());
        }
    }
}

fn reply(client: &mut Pipeline, tag: Tag) -> Reply {
    client
        .wait(tag, Instant::now() + Duration::from_secs(2))
        .unwrap()
}

fn open(client: &mut Pipeline, root: Fid, name: &[u8], flags: u32) -> Fid {
    let (tag, fid) = client.walk(root, &[name]).unwrap();
    assert!(matches!(reply(client, tag), Reply::Walk(_)));
    let tag = client.lopen(fid, flags).unwrap();
    assert!(matches!(reply(client, tag), Reply::Lopen { .. }));
    fid
}

fn submit(client: &mut Pipeline, root: Fid, id: u64, candidate: PortalFileCandidate) {
    let transaction = open(client, root, b"transaction", 2);
    let bytes = encode_portal_file_candidate(1, id, &candidate).unwrap();
    let tag = client.write(transaction, 0, &bytes).unwrap();
    assert!(matches!(reply(client, tag), Reply::Write(n) if n == bytes.len() as u32));
    let submit = open(client, root, b"submit", 1);
    let bytes = encode_portal_file_submit(PortalFileSubmit {
        connection_epoch: 1,
        submission_id: id,
        candidate_bytes: bytes.len() as u32,
    })
    .unwrap();
    let tag = client.write(submit, 0, &bytes).unwrap();
    assert!(matches!(reply(client, tag), Reply::Write(24)));
    for fid in [transaction, submit] {
        let tag = client.clunk(fid).unwrap();
        assert!(matches!(reply(client, tag), Reply::Clunk));
    }
}

fn events(client: &mut Pipeline, root: Fid, events: Fid, offset: &mut u64) -> Vec<PortalFileEvent> {
    let tag = client.read(events, *offset, 4096).unwrap();
    let Reply::Read(bytes) = reply(client, tag) else {
        panic!("expected portal journal")
    };
    *offset += bytes.len() as u64;
    let mut remaining = bytes.as_slice();
    let mut result = Vec::new();
    let mut sequence = 0;
    while !remaining.is_empty() {
        let size = u32::from_le_bytes(remaining[..4].try_into().unwrap()) as usize;
        let (header, event) = decode_portal_file_event(&remaining[..size], 1).unwrap();
        sequence = header.sequence;
        result.push(event);
        remaining = &remaining[size..];
    }
    assert_ne!(sequence, 0);
    let ack = open(client, root, b"ack", 1);
    let bytes = encode_portal_file_ack(PortalFileAck {
        connection_epoch: 1,
        sequence,
    })
    .unwrap();
    let tag = client.write(ack, 0, &bytes).unwrap();
    assert!(matches!(reply(client, tag), Reply::Write(16)));
    let tag = client.clunk(ack).unwrap();
    assert!(matches!(reply(client, tag), Reply::Clunk));
    result
}

#[test]
fn private_requester_wire_refuses_another_socket_with_the_same_kernel_peer() {
    let export = PortalFileExport::new(
        Authority {
            revision: 1,
            rights: true,
            allow: true,
            live: true,
            now: 100,
            generation: 9,
        },
        PortalFileLimits::defaults(1),
    )
    .unwrap();
    let mut server = Server::new(export, sophia_9p::Limits::default()).unwrap();
    // Both socketpairs are created by this process: their kernel credentials
    // are identical, but only one connection receives owner admission.
    let (trusted_owner, trusted) = UnixStream::pair().unwrap();
    let (foreign_owner, foreign) = UnixStream::pair().unwrap();
    let connection = server
        .adopt(trusted_owner)
        .unwrap_or_else(|_| panic!("adopt private connection"));
    let foreign_connection = server
        .adopt(foreign_owner)
        .unwrap_or_else(|_| panic!("adopt foreign connection"));
    assert_ne!(connection, foreign_connection);
    server
        .export_mut()
        .admit(PortalRequesterAdmission {
            connection,
            custody: PortalRequesterCustody::SessionXAuthority,
            epoch: 1,
            source: NamespaceId::from_raw(1),
            target: NamespaceId::from_raw(2),
        })
        .unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let effects = Arc::new(Mutex::new(Vec::new()));
    let recorded = effects.clone();
    let _owner = Owner {
        stop,
        thread: Some(std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                server.export_mut().expire(Instant::now());
                server.turn(Some(Duration::from_millis(5))).unwrap();
                if let Some(work) = server.export_mut().take_execution() {
                    let transfer = work.grant.transfer;
                    recorded.lock().unwrap().push((transfer, work.payload));
                    server
                        .export_mut()
                        .settle_execution(transfer, true)
                        .unwrap();
                }
            }
        })),
    };
    let mut foreign =
        Pipeline::over(foreign, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let (tag, _) = foreign.attach(b"Session", b"portal-requester").unwrap();
    assert!(
        matches!(reply(&mut foreign, tag), Reply::Error(e) if e.0 == 13),
        "same-process credentials must not substitute for private connection custody"
    );
    let mut client =
        Pipeline::over(trusted, PipelineLimits::default(), Duration::from_secs(2)).unwrap();
    let (tag, root) = client.attach(b"untrusted name", b"").unwrap();
    assert!(matches!(reply(&mut client, tag), Reply::Attach(_)));
    let journal = open(&mut client, root, b"events", 0);
    let mut offset = 0;
    submit(
        &mut client,
        root,
        1,
        PortalFileCandidate::Negotiate(PortalNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
        }),
    );
    let records = events(&mut client, root, journal, &mut offset);
    assert!(matches!(
        records.as_slice(),
        [PortalFileEvent::Submitted(_), PortalFileEvent::Negotiated]
    ));
    submit(&mut client, root, 2, request(1));
    let records = events(&mut client, root, journal, &mut offset);
    assert!(
        matches!(records.as_slice(), [PortalFileEvent::Submitted(_), PortalFileEvent::Decision(packet)]
        if matches!(packet.decision, PortalBrokerResponseDecision::Allowed(_)))
    );
    assert!(
        effects.lock().unwrap().is_empty(),
        "Allowed is not execution"
    );
    let payload = b"bounded clipboard bytes";
    submit(
        &mut client,
        root,
        3,
        PortalFileCandidate::PayloadBegin(PortalPayloadBegin {
            transfer: PortalTransferId::from_raw(1),
            total_bytes: payload.len() as u64,
        }),
    );
    assert!(matches!(
        events(&mut client, root, journal, &mut offset).as_slice(),
        [PortalFileEvent::Submitted(_)]
    ));
    let (tag, upload) = client.walk(root, &[b"upload", b"0"]).unwrap();
    assert!(matches!(reply(&mut client, tag), Reply::Walk(_)));
    let tag = client.lopen(upload, 1).unwrap();
    assert!(matches!(reply(&mut client, tag), Reply::Lopen { .. }));
    let tag = client.write(upload, 0, payload).unwrap();
    assert!(matches!(reply(&mut client, tag), Reply::Write(n) if n == payload.len() as u32));
    assert!(
        effects.lock().unwrap().is_empty(),
        "upload is not execution"
    );
    submit(
        &mut client,
        root,
        4,
        PortalFileCandidate::PayloadEnd(PortalPayloadEnd {
            transfer: PortalTransferId::from_raw(1),
            total_bytes: payload.len() as u64,
        }),
    );
    // Submitted and the recipient outcome may occupy separate wire reads.
    let mut records = events(&mut client, root, journal, &mut offset);
    if records.len() == 1 {
        records.extend(events(&mut client, root, journal, &mut offset));
    }
    assert!(matches!(records.as_slice(), [PortalFileEvent::Submitted(_),
        PortalFileEvent::TransferOutcome(outcome)]
        if outcome.transfer == PortalTransferId::from_raw(1)
        && outcome.state == PortalGrantState::Completed));
    let tag = client.clunk(upload).unwrap();
    assert!(matches!(reply(&mut client, tag), Reply::Clunk));
    assert_eq!(
        *effects.lock().unwrap(),
        [(PortalTransferId::from_raw(1), payload.to_vec())]
    );
}

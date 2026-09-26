//! File custody uses the production export, journal and driver-issued permit.
//! Payloads in this fixture deliberately use a tiny test codec: these controls
//! are not scalar-schema interoperability or authenticated launch evidence.
#![cfg(test)]

use super::super::adapter::{PolicyAdapter, PolicyProfileAdmission};
use super::super::{PolicyTransportCommand, PolicyTransportEvent};
use super::owner::{Handle, Node};
use super::*;
use sophia_protocol::{PolicyConfiguration, TransactionId, WmChromePolicy};
use std::sync::mpsc::sync_channel;

struct Codec;
impl PolicyFileCodec for Codec {
    fn decode_candidate(&self, bytes: &[u8], _: u64) -> Result<DecodedFileCandidate, Errno> {
        let record =
            decode_wm_file_record(bytes, WmFileClass::Candidate).map_err(|_| Errno::EINVAL)?;
        if record.header.kind != WmFileKind::Configuration || record.body.len() != 1 {
            return Err(Errno::EINVAL);
        }
        Ok(DecodedFileCandidate {
            event: PolicyAdapterEvent::Configuration {
                transaction: TransactionId::from_raw(700),
                configuration: PolicyConfiguration {
                    connection_epoch: record.header.connection_epoch,
                    generation: 3,
                    actions: Vec::new(),
                    chrome: WmChromePolicy::default(),
                },
            },
            required_capabilities: u64::from(record.body[0]),
        })
    }
    fn submitted_body(&self, id: u64, _: WmFileKind) -> Result<Vec<u8>, Errno> {
        Ok(id.to_le_bytes().to_vec())
    }
}

fn record(epoch: u64, kind: WmFileKind, id: u64, body: &[u8]) -> Vec<u8> {
    encode_wm_file_record(
        WmFileHeader {
            kind,
            connection_epoch: epoch,
            submission_id: id,
            sequence: 0,
        },
        body,
    )
    .unwrap()
}
fn owner(epoch: u64, qids: WmQids) -> WmFiles<Codec> {
    WmFiles::new(
        epoch,
        1,
        record(epoch, WmFileKind::Limits, 0, &[]),
        qids,
        Codec,
    )
    .unwrap()
}
fn submit(epoch: u64, id: u64, size: usize) -> Vec<u8> {
    let mut bytes = epoch.to_le_bytes().to_vec();
    bytes.extend(id.to_le_bytes());
    bytes.extend((size as u32).to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes
}

// Obtain a real permit from the driver's first wait site. The fixture refuses
// its scripted peer after extracting the permit; it adds no permit constructor.
fn configuration_permit() -> PolicyReceivePermit {
    struct Capture(Option<PolicyReceivePermit>);
    impl PolicyAdapter for Capture {
        fn admit(&mut self, _: u64, _: Option<PolicyProfileAdmission>) -> Result<(), String> {
            Ok(())
        }
        fn selected_capabilities(&self) -> u64 {
            1
        }
        fn receive_within(
            &mut self,
            permit: PolicyReceivePermit,
            _: Duration,
        ) -> Result<PolicyAdapterEvent, String> {
            self.0 = Some(permit);
            Err("fixture ends after driver-issued permit".into())
        }
        fn try_receive(
            &mut self,
            _: PolicyReceivePermit,
        ) -> Result<Option<PolicyAdapterEvent>, String> {
            unreachable!()
        }
        fn send(&mut self, _: &PolicyTransportCommand) -> Result<(), String> {
            unreachable!()
        }
        fn disconnect(&mut self) {}
    }
    let mut capture = Capture(None);
    let (_commands, receive) = sync_channel(1);
    let (events, _audit) = sync_channel::<PolicyTransportEvent>(2);
    assert!(
        super::super::driver::run_policy_transport(&mut capture, 9, None, &receive, &events)
            .is_err()
    );
    capture.0.unwrap()
}

#[test]
fn staging_retries_preserve_prefix_and_do_not_renew_expiry() {
    let mut staging = Staging::new(1);
    let now = Instant::now();
    let bytes = record(9, WmFileKind::Configuration, 1, &[0]);
    staging.write(0, &bytes[..3], now).unwrap();
    assert_eq!(staging.write(2, &bytes[2..5], now), Err(Errno::EINVAL));
    assert_eq!(staging.bytes, bytes[..3]);
    assert_eq!(staging.write(4, &[], now), Err(Errno::EINVAL));
    staging
        .write(0, &bytes[..3], now + Duration::from_secs(11))
        .unwrap();
    staging
        .write(3, &bytes[3..], now + Duration::from_secs(11))
        .unwrap();
    assert!(staging.expired(now + Duration::from_secs(12)));
    assert_eq!(
        staging.write(bytes.len() as u64, &[], now + Duration::from_secs(12)),
        Err(Errno::ESTALE)
    );
}

#[test]
fn complete_submit_requires_permit_and_duplicate_does_not_spend_the_next_one() {
    let mut files = owner(9, WmQids::new());
    let mut handle = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    let bytes = record(9, WmFileKind::Configuration, 5, &[1]);
    files
        .write(&Node::Transaction, &mut handle, 0, &bytes)
        .unwrap();
    let request = submit(9, 5, bytes.len());
    assert_eq!(
        files.write(&Node::Submit, &mut Handle::Plain, 0, &request),
        Err(Errno::EAGAIN)
    );
    assert!(files.take_delivery().is_none());
    files.offer(configuration_permit()).unwrap();
    files
        .write(&Node::Submit, &mut Handle::Plain, 0, &request)
        .unwrap();
    let Some(PolicyAdapterEvent::Configuration { transaction, .. }) = files.take_delivery() else {
        panic!("one complete value")
    };
    assert_eq!(transaction, TransactionId::from_raw(700));
    files.offer(configuration_permit()).unwrap();
    files
        .write(&Node::Submit, &mut Handle::Plain, 0, &request)
        .unwrap();
    assert!(files.take_delivery().is_none());
    // Transport ACK releases retained bytes, not the next permit or a domain outcome.
    let ack = [9u64.to_le_bytes(), 1u64.to_le_bytes()].concat();
    files
        .write(&Node::Ack, &mut Handle::Plain, 0, &ack)
        .unwrap();
    assert_eq!(
        files.write(&Node::Submit, &mut Handle::Plain, 0, &request),
        Err(EALREADY)
    );
    let mut next = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    let bytes = record(9, WmFileKind::Configuration, 6, &[1]);
    files
        .write(&Node::Transaction, &mut next, 0, &bytes)
        .unwrap();
    files
        .write(
            &Node::Submit,
            &mut Handle::Plain,
            0,
            &submit(9, 6, bytes.len()),
        )
        .unwrap();
    assert!(matches!(
        files.take_delivery(),
        Some(PolicyAdapterEvent::Configuration { .. })
    ));
}

#[test]
fn unnegotiated_extension_is_refused_before_driver_delivery() {
    let mut files = owner(9, WmQids::new());
    files.offer(configuration_permit()).unwrap();
    let mut handle = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    let bytes = record(9, WmFileKind::Configuration, 1, &[2]);
    files
        .write(&Node::Transaction, &mut handle, 0, &bytes)
        .unwrap();
    assert_eq!(
        files.write(
            &Node::Submit,
            &mut Handle::Plain,
            0,
            &submit(9, 1, bytes.len())
        ),
        Err(Errno::EACCES)
    );
    assert!(files.take_delivery().is_none());
    assert_eq!(
        files.read(&Node::Transaction, &mut handle, 0, 100).unwrap(),
        ReadOutcome::Ready(bytes)
    );
}

#[test]
fn journal_capacity_ack_and_reread_preserve_whole_records() {
    let mut journal = Journal::new(9);
    for _ in 0..64 {
        journal.append(WmFileKind::Submitted, &[0; 8]).unwrap();
    }
    let old = journal.read(0, 100).unwrap();
    assert_eq!(journal.read(0, 100).unwrap(), old);
    assert_eq!(
        journal.append(WmFileKind::Submitted, &[0; 8]),
        Err(Errno::EAGAIN)
    );
    assert_eq!(
        journal.ack(WmFileAck {
            connection_epoch: 8,
            sequence: 1
        }),
        Err(Errno::ESTALE)
    );
    assert_eq!(
        journal.ack(WmFileAck {
            connection_epoch: 9,
            sequence: 65
        }),
        Err(Errno::EINVAL)
    );
    journal
        .ack(WmFileAck {
            connection_epoch: 9,
            sequence: 1,
        })
        .unwrap();
    journal
        .ack(WmFileAck {
            connection_epoch: 9,
            sequence: 1,
        })
        .unwrap();
    assert_eq!(journal.read(0, 1), Err(Errno::ESTALE));
    assert_eq!(journal.append(WmFileKind::Submitted, &[0; 8]), Ok(65));
    assert_eq!(
        journal.read(journal.size(), 1).unwrap(),
        ReadOutcome::Pending
    );
    assert_eq!(journal.read(journal.size() + 1, 1), Err(Errno::EINVAL));
}

#[test]
fn snapshots_pin_metadata_and_qids_continue_across_epochs() {
    let qids = WmQids::new();
    let mut first = owner(9, qids.clone());
    first
        .publish_snapshot(record(9, WmFileKind::Snapshot, 0, &[1]))
        .unwrap();
    let mut pinned = first.open(&Node::Snapshot, OpenFlags(0)).unwrap();
    let old = first.describe(&Node::Snapshot, Some(&pinned));
    first
        .publish_snapshot(record(9, WmFileKind::Snapshot, 0, &[2; 40]))
        .unwrap();
    assert_eq!(first.describe(&Node::Snapshot, Some(&pinned)), old);
    assert!(matches!(
        first.open(&Node::Snapshot, OpenFlags(0)),
        Err(EBUSY)
    ));
    assert_eq!(
        first.read(&Node::Snapshot, &mut pinned, 32, 100).unwrap(),
        ReadOutcome::Ready(vec![1])
    );
    first.release(Node::Snapshot, Some(pinned));
    let current = first.open(&Node::Snapshot, OpenFlags(0)).unwrap();
    assert_ne!(
        first.describe(&Node::Snapshot, Some(&current)).qid_path,
        old.qid_path
    );
    let second = owner(10, qids);
    assert!(
        second.describe(&Node::Root, None).qid_path
            > first.describe(&Node::Snapshot, Some(&current)).qid_path
    );
    let exhausted = WmQids(Arc::new(Mutex::new(u64::MAX - 3)));
    assert!(matches!(
        WmFiles::new(
            11,
            1,
            record(11, WmFileKind::Limits, 0, &[]),
            exhausted.clone(),
            Codec
        ),
        Err(Errno::ENOSPC)
    ));
    assert_eq!(*exhausted.0.lock().unwrap(), u64::MAX - 3);
}

// Minimal independently framed client for the actual adopted-stream reactor.
// It does not reuse the server codec or infer semantic outcomes from Rwrite.
fn rpc(stream: &mut UnixStream, kind: u8, tag: u16, body: &[u8]) -> (u8, Vec<u8>) {
    use std::io::{Read, Write};
    let mut request = ((7 + body.len()) as u32).to_le_bytes().to_vec();
    request.push(kind);
    request.extend(tag.to_le_bytes());
    request.extend(body);
    stream.write_all(&request).unwrap();
    let mut header = [0; 7];
    stream.read_exact(&mut header).unwrap();
    assert_eq!(u16::from_le_bytes(header[5..].try_into().unwrap()), tag);
    let size = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    assert!((7..=65536).contains(&size));
    let mut body = vec![0; size - 7];
    stream.read_exact(&mut body).unwrap();
    (header[4], body)
}

#[test]
fn real_reactor_services_ack_to_release_a_blocked_whole_event_send() {
    let mut files = owner(9, WmQids::new());
    for _ in 0..64 {
        files.append_event(WmFileKind::Submitted, &[0; 8]).unwrap();
    }
    let (server, mut client) = UnixStream::pair().unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut reactor = NinePReactor::adopt(server, files).unwrap();
    let thread = std::thread::spawn(move || {
        reactor.send_event(WmFileKind::Submitted, &[99; 8]).unwrap();
        reactor
    });
    let mut version = 65536u32.to_le_bytes().to_vec();
    version.extend(8u16.to_le_bytes());
    version.extend(b"9P2000.L");
    assert_eq!(rpc(&mut client, 100, u16::MAX, &version).0, 101);
    let mut attach = 1u32.to_le_bytes().to_vec();
    attach.extend(u32::MAX.to_le_bytes());
    attach.extend([0; 4]); // empty uname/aname; neither is authority
    attach.extend(u32::MAX.to_le_bytes());
    assert_eq!(rpc(&mut client, 104, 1, &attach).0, 105);
    let mut walk = 1u32.to_le_bytes().to_vec();
    walk.extend(2u32.to_le_bytes());
    walk.extend(1u16.to_le_bytes());
    walk.extend(3u16.to_le_bytes());
    walk.extend(b"ack");
    assert_eq!(rpc(&mut client, 110, 2, &walk).0, 111);
    let open = [2u32.to_le_bytes(), 1u32.to_le_bytes()].concat();
    assert_eq!(rpc(&mut client, 12, 3, &open).0, 13);
    let mut write = 2u32.to_le_bytes().to_vec();
    write.extend(0u64.to_le_bytes());
    write.extend(16u32.to_le_bytes());
    write.extend(9u64.to_le_bytes());
    write.extend(1u64.to_le_bytes());
    assert_eq!(
        rpc(&mut client, 118, 4, &write),
        (119, 16u32.to_le_bytes().to_vec())
    );
    let mut reactor = thread.join().unwrap();
    assert!(reactor.owner_mut().take_delivery().is_none());
    // ACK released one transport record; the next send occupies it again.
    assert_eq!(
        reactor.owner_mut().append_event(WmFileKind::Submitted, &[]),
        Err(Errno::EAGAIN)
    );
}

#[test]
fn stop_wakes_actual_reactor_waiting_for_ack_credit() {
    let mut files = owner(9, WmQids::new());
    for _ in 0..64 {
        files.append_event(WmFileKind::Submitted, &[]).unwrap();
    }
    let (server, _client) = UnixStream::pair().unwrap();
    let mut reactor = NinePReactor::adopt(server, files).unwrap();
    let stop = reactor.stop_handle();
    let (began, waiting) = sync_channel(1);
    let (done, ended) = sync_channel(1);
    let thread = std::thread::spawn(move || {
        began.send(()).unwrap();
        done.send(reactor.send_event(WmFileKind::Submitted, &[]))
            .unwrap();
    });
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    stop.stop();
    assert!(ended.recv_timeout(Duration::from_secs(1)).unwrap().is_err());
    thread.join().unwrap();
}

struct ArrayCodec;
impl PolicyFileCodec for ArrayCodec {
    fn decode_candidate(&self, bytes: &[u8], selected: u64) -> Result<DecodedFileCandidate, Errno> {
        let value = decode_wm_file_configuration(bytes, selected).map_err(|e| match e {
            WmFilePayloadError::Capabilities { .. } => Errno::EACCES,
            _ => Errno::EINVAL,
        })?;
        Ok(DecodedFileCandidate {
            required_capabilities: 0,
            event: PolicyAdapterEvent::Configuration {
                transaction: value.transaction,
                configuration: value.configuration,
            },
        })
    }
    // Submitted scalar encoding is deliberately still a fixture until the
    // separately owned scalar schema lands. Configuration uses the real codec.
    fn submitted_body(&self, id: u64, _: WmFileKind) -> Result<Vec<u8>, Errno> {
        Ok(id.to_le_bytes().to_vec())
    }
}

#[test]
fn real_array_decoder_refuses_unnegotiated_chrome_before_semantic_delivery() {
    use sophia_protocol::SOPHIA_WM_CAPABILITY_CONFIGURATION;
    let mut files = WmFiles::new(
        9,
        SOPHIA_WM_CAPABILITY_CONFIGURATION,
        record(9, WmFileKind::Limits, 0, &[]),
        WmQids::new(),
        ArrayCodec,
    )
    .unwrap();
    files.offer(configuration_permit()).unwrap();
    let value = WmFileConfiguration {
        transaction: TransactionId::from_raw(700),
        configuration: PolicyConfiguration {
            connection_epoch: 9,
            generation: 3,
            actions: Vec::new(),
            chrome: WmChromePolicy::default(),
        },
    };
    let bytes = encode_wm_file_configuration(
        WmFileHeader {
            kind: WmFileKind::Configuration,
            connection_epoch: 9,
            submission_id: 5,
            sequence: 0,
        },
        &value,
        u64::MAX,
    )
    .unwrap();
    let mut handle = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    files
        .write(&Node::Transaction, &mut handle, 0, &bytes)
        .unwrap();
    assert_eq!(
        files.write(
            &Node::Submit,
            &mut Handle::Plain,
            0,
            &submit(9, 5, bytes.len())
        ),
        Err(Errno::EACCES)
    );
    assert!(files.take_delivery().is_none());
    // Abandoning refused bytes leaves the same permit usable for an admitted candidate.
    files.release(Node::Transaction, Some(handle));
    let mut value = value;
    value.configuration.chrome.focus_ring.enabled = false;
    value.configuration.chrome.focus_ring.width = 0;
    value.configuration.chrome.frame.enabled = false;
    value.configuration.chrome.frame.width = 0;
    let bytes = encode_wm_file_configuration(
        WmFileHeader {
            kind: WmFileKind::Configuration,
            connection_epoch: 9,
            submission_id: 6,
            sequence: 0,
        },
        &value,
        SOPHIA_WM_CAPABILITY_CONFIGURATION,
    )
    .unwrap();
    let mut handle = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    files
        .write(&Node::Transaction, &mut handle, 0, &bytes)
        .unwrap();
    files
        .write(
            &Node::Submit,
            &mut Handle::Plain,
            0,
            &submit(9, 6, bytes.len()),
        )
        .unwrap();
    let Some(PolicyAdapterEvent::Configuration {
        transaction,
        configuration,
    }) = files.take_delivery()
    else {
        panic!("configuration")
    };
    assert_eq!(transaction, value.transaction);
    assert_eq!(configuration, value.configuration);
}

#[test]
fn reactor_receive_timeout_withdraws_permit_without_delivering_fragments() {
    let (server, _client) = UnixStream::pair().unwrap();
    let mut reactor = NinePReactor::adopt(server, owner(9, WmQids::new())).unwrap();
    assert!(
        reactor
            .receive(configuration_permit(), Duration::ZERO)
            .unwrap()
            .is_none()
    );
    // A fresh driver permit can be offered after the poll returned. No second
    // phase owner or orphaned grant remains inside the export.
    reactor.owner_mut().offer(configuration_permit()).unwrap();
}

#[test]
fn no_permit_never_decodes_and_accepted_replay_never_decodes_again() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Counted(Arc<AtomicUsize>);
    impl PolicyFileCodec for Counted {
        fn decode_candidate(
            &self,
            bytes: &[u8],
            selected: u64,
        ) -> Result<DecodedFileCandidate, Errno> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Codec.decode_candidate(bytes, selected)
        }
        fn submitted_body(&self, id: u64, kind: WmFileKind) -> Result<Vec<u8>, Errno> {
            Codec.submitted_body(id, kind)
        }
    }
    let count = Arc::new(AtomicUsize::new(0));
    let mut files = WmFiles::new(
        9,
        1,
        record(9, WmFileKind::Limits, 0, &[]),
        WmQids::new(),
        Counted(count.clone()),
    )
    .unwrap();
    let mut handle = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    let bytes = record(9, WmFileKind::Configuration, 1, &[1]);
    files
        .write(&Node::Transaction, &mut handle, 0, &bytes)
        .unwrap();
    let submit = submit(9, 1, bytes.len());
    assert_eq!(
        files.write(&Node::Submit, &mut Handle::Plain, 0, &submit),
        Err(Errno::EAGAIN)
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    files.offer(configuration_permit()).unwrap();
    files
        .write(&Node::Submit, &mut Handle::Plain, 0, &submit)
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    files
        .write(&Node::Submit, &mut Handle::Plain, 0, &submit)
        .unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn transaction_open_metadata_names_each_staging_object_without_reusing_qids() {
    let mut files = owner(9, WmQids::new());
    let node = files.describe(&Node::Transaction, None);
    let first = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    let first_metadata = files.describe(&Node::Transaction, Some(&first));
    assert_ne!(node.qid_path, first_metadata.qid_path);
    files.release(Node::Transaction, Some(first));
    let second = files.open(&Node::Transaction, OpenFlags(2)).unwrap();
    assert_ne!(
        first_metadata.qid_path,
        files.describe(&Node::Transaction, Some(&second)).qid_path
    );
}

#[test]
fn actual_reactor_send_without_ack_has_a_bounded_deadline() {
    let mut files = owner(9, WmQids::new());
    for _ in 0..64 {
        files.append_event(WmFileKind::Submitted, &[]).unwrap();
    }
    let (server, _client) = UnixStream::pair().unwrap();
    let mut reactor = NinePReactor::adopt(server, files).unwrap();
    let started = Instant::now();
    let error = reactor.send_event(WmFileKind::Submitted, &[]).unwrap_err();
    assert!(error.contains("deadline"), "{error}");
    assert!(started.elapsed() >= SEND_DEADLINE);
    assert!(started.elapsed() < SEND_DEADLINE + Duration::from_secs(2));
}

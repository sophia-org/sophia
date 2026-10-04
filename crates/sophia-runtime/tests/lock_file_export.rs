//! t294: the lock provider's files over the 9P export interface. Only the
//! admitted connection attaches, once; the root vocabulary is fixed; an open
//! lock object keeps the generation it opened; a candidate is staged, then
//! submitted; and an upload writer is fenced from any later binding of its
//! slot.
use sophia_9p::connection::ConnectionId;
use sophia_9p::export::*;
use sophia_9p::{Errno, OpenFlags, ReadOutcome};
use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;

use LockFileNode as Node;

const EPOCH: u64 = 5;
const READ: OpenFlags = OpenFlags(0);
const WRITE: OpenFlags = OpenFlags(1);
const READ_WRITE: OpenFlags = OpenFlags(2);

fn lock(epoch: u64) -> LockObject {
    LockObject {
        lock_epoch: epoch,
        topology_generation: 1,
        phase: if epoch == 0 {
            LockPhase::Unlocked
        } else {
            LockPhase::Locked
        },
        allocations: if epoch == 0 {
            Vec::new()
        } else {
            vec![LockAllocation {
                output_id: 1,
                output_generation: 1,
                allocation_id: 2,
                allocation_generation: 1,
                pixel_width: 2,
                pixel_height: 1,
                scale_numerator: 1,
                scale_denominator: 1,
            }]
        },
    }
}

fn export() -> LockFileExport {
    LockFileExport::new(
        LockFileSettings {
            epoch: EPOCH,
            limits: LockFileLimits {
                max_outputs: 1,
                upload_slots: 2,
                max_chords: 0,
                max_width_px: 8,
                max_height_px: 8,
                max_resource_bytes: 256,
                max_live_resources: 4,
                journal_records: 32,
                journal_bytes: 8192,
                assembly_timeout_ms: 1000,
                ack_progress_timeout_ms: 2000,
            },
            reserved_chords: Vec::new(),
            lock: lock(0),
            lock_qid: 0,
        },
        LockFileQids::default(),
    )
    .unwrap()
}

fn context(connection: u64) -> AttachContext<'static> {
    AttachContext {
        connection: ConnectionId(connection),
        peer: None,
        uname: b"",
        aname: b"",
        n_uname: 0,
    }
}

fn attached() -> LockFileExport {
    let mut export = export();
    export.bind_connection(ConnectionId(1)).unwrap();
    export.attach(&context(1)).unwrap();
    export
}

fn read_all(export: &mut LockFileExport, node: Node, handle: &mut LockFileHandle) -> Vec<u8> {
    match export.read(&node, handle, 0, 1 << 16).unwrap() {
        ReadOutcome::Ready(bytes) => bytes,
        _ => panic!("not ready"),
    }
}

fn submit(export: &mut LockFileExport, submission_id: u64, kind: LockFileKind, body: &[u8]) {
    let candidate = encode_lock_file_record(
        LockFileHeader {
            kind,
            connection_epoch: EPOCH,
            submission_id,
            sequence: 0,
        },
        body,
    )
    .unwrap();
    let mut staging = export.open(&Node::Transaction, READ_WRITE).unwrap();
    assert_eq!(
        export.write(&Node::Transaction, &mut staging, 0, &candidate),
        Ok(candidate.len() as u32)
    );
    let control = encode_lock_file_submit(LockFileSubmit {
        connection_epoch: EPOCH,
        submission_id,
        candidate_bytes: candidate.len() as u32,
    })
    .unwrap();
    let mut handle = export.open(&Node::Submit, WRITE).unwrap();
    export
        .write(&Node::Submit, &mut handle, 0, &control)
        .unwrap();
    // The staged candidate is gone once submitted.
    assert_eq!(
        export.write(&Node::Transaction, &mut staging, 0, &candidate),
        Err(Errno::ESTALE)
    );
    export.release(Node::Transaction, Some(staging));
}

fn negotiate(export: &mut LockFileExport) {
    let body = LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 1,
        requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT,
        chords: Vec::new(),
    }
    .encode()
    .unwrap();
    submit(export, 1, LockFileKind::Negotiate, &body);
}

#[test]
fn only_the_admitted_connection_attaches_and_only_once() {
    let mut export = export();
    assert_eq!(export.attach(&context(1)).unwrap_err(), Errno::EACCES);
    export.bind_connection(ConnectionId(1)).unwrap();
    assert_eq!(
        export.bind_connection(ConnectionId(2)),
        Err(Errno::EACCES),
        "never replaced"
    );
    assert_eq!(export.attach(&context(2)).unwrap_err(), Errno::EACCES);
    export.attach(&context(1)).unwrap();
    assert_eq!(export.attach(&context(1)).unwrap_err(), Errno::EACCES);
}

#[test]
fn the_root_vocabulary_is_fixed() {
    let mut export = attached();
    let mut root = export.open(&Node::Root, READ).unwrap();
    let names: Vec<_> = export
        .readdir(&Node::Root, &mut root, 0, 32)
        .unwrap()
        .into_iter()
        .map(|entry| String::from_utf8(entry.name).unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "api",
            "limits",
            "lock",
            "events",
            "transaction",
            "submit",
            "ack",
            "upload"
        ]
    );
    let uploads = export
        .lookup(&Node::Root, WalkName::Child(b"upload"))
        .unwrap();
    let mut directory = export.open(&uploads, READ).unwrap();
    let slots: Vec<_> = export
        .readdir(&uploads, &mut directory, 0, 32)
        .unwrap()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(slots, [b"0".to_vec(), b"1".to_vec()]);
    assert_eq!(
        export.lookup(&uploads, WalkName::Child(b"2")),
        Err(Errno::ENOENT)
    );
    assert_eq!(
        export.lookup(&Node::Root, WalkName::Child(b"secret")),
        Err(Errno::ENOENT)
    );
    let mut api = export.open(&Node::Api, READ).unwrap();
    assert_eq!(
        read_all(&mut export, Node::Api, &mut api),
        b"sophia-lock-files version=1 epoch=5\n"
    );
    let mut limits = export.open(&Node::Limits, READ).unwrap();
    let bytes = read_all(&mut export, Node::Limits, &mut limits);
    let record = decode_lock_file_record(&bytes, LockFileClass::Object).unwrap();
    assert_eq!(LockFileLimits::decode(record.body).unwrap().upload_slots, 2);
    // Nothing the provider reads is writable, and nothing it writes readable.
    assert_eq!(export.open(&Node::Lock, WRITE).err(), Some(Errno::EACCES));
    assert_eq!(export.open(&Node::Submit, READ).err(), Some(Errno::EACCES));
}

#[test]
fn an_open_lock_object_keeps_the_generation_it_opened() {
    let mut export = attached();
    negotiate(&mut export);
    let mut old = export.open(&Node::Lock, READ).unwrap();
    let old_qid = export.describe(&Node::Lock, Some(&old)).qid_path;
    export.publish_lock(lock(3)).unwrap();
    let mut new = export.open(&Node::Lock, READ).unwrap();
    assert_ne!(export.describe(&Node::Lock, Some(&new)).qid_path, old_qid);
    let decode = |bytes: Vec<u8>| {
        let record = decode_lock_file_record(&bytes, LockFileClass::Object).unwrap();
        LockObject::decode(record.body).unwrap()
    };
    assert_eq!(decode(read_all(&mut export, Node::Lock, &mut old)), lock(0));
    assert_eq!(decode(read_all(&mut export, Node::Lock, &mut new)), lock(3));
}

#[test]
fn one_candidate_is_staged_at_a_time() {
    let mut export = attached();
    let _staging = export.open(&Node::Transaction, READ_WRITE).unwrap();
    assert_eq!(
        export.open(&Node::Transaction, READ_WRITE).err(),
        Some(Errno(16))
    );
    assert_eq!(
        export.open(&Node::Transaction, WRITE).err(),
        Some(Errno::EACCES),
        "read-write only"
    );
}

#[test]
fn an_upload_writer_is_fenced_from_a_later_binding_of_its_slot() {
    let mut export = attached();
    negotiate(&mut export);
    assert_eq!(
        export.open(&Node::Upload(0), WRITE).err(),
        Some(Errno::ESTALE),
        "nothing bound"
    );
    let begin = |id| {
        LockResourceBegin {
            transaction: id,
            resource: LockResourceId { id, generation: 1 },
            width_px: 2,
            height_px: 1,
            slot: 0,
        }
        .encode()
        .unwrap()
    };
    submit(&mut export, 2, LockFileKind::ResourceBegin, &begin(1));
    let mut first = export.open(&Node::Upload(0), WRITE).unwrap();
    assert_eq!(
        export.write(&Node::Upload(0), &mut first, 0, &[1; 4]),
        Ok(4)
    );
    let cancel = LockResourceStep {
        transaction: 1,
        resource: LockResourceId {
            id: 1,
            generation: 1,
        },
        total_bytes: None,
    }
    .encode()
    .unwrap();
    submit(&mut export, 3, LockFileKind::ResourceCancel, &cancel);
    submit(&mut export, 4, LockFileKind::ResourceBegin, &begin(2));
    assert_eq!(
        export.write(&Node::Upload(0), &mut first, 0, &[1; 8]),
        Err(Errno::ESTALE)
    );
    let mut second = export.open(&Node::Upload(0), WRITE).unwrap();
    assert_eq!(
        export.write(&Node::Upload(0), &mut second, 0, &[2; 8]),
        Ok(8)
    );
}

#[test]
fn an_upload_received_in_place_counts_only_once_accepted() {
    let mut export = attached();
    negotiate(&mut export);
    let begin = |id| {
        LockResourceBegin {
            transaction: id,
            resource: LockResourceId { id, generation: 1 },
            width_px: 2,
            height_px: 1,
            slot: 0,
        }
        .encode()
        .unwrap()
    };
    submit(&mut export, 2, LockFileKind::ResourceBegin, &begin(1));
    let mut upload = export.open(&Node::Upload(0), WRITE).unwrap();
    let node = Node::Upload(0);
    // Other files are never received in place.
    let mut ack = export.open(&Node::Ack, WRITE).unwrap();
    assert_eq!(
        export.write_destination(&Node::Ack, &mut ack, 0, 16, 0),
        Ok(None)
    );
    // Only at the cursor, and only within the declared size.
    assert_eq!(
        export.write_destination(&node, &mut upload, 4, 4, 0),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        export.write_destination(&node, &mut upload, 0, 9, 0),
        Err(Errno::EINVAL)
    );
    let first = export
        .write_destination(&node, &mut upload, 0, 8, 0)
        .unwrap()
        .unwrap();
    assert_eq!(first.len(), 8);
    first[..4].fill(5);
    let rest = export
        .write_destination(&node, &mut upload, 0, 8, 4)
        .unwrap()
        .unwrap();
    assert_eq!(rest.len(), 4);
    rest.fill(6);
    // Nothing counts yet: the cursor has not moved.
    assert_eq!(
        export.write(&node, &mut upload, 8, &[0]),
        Err(Errno::EINVAL)
    );
    assert_eq!(export.write_received(&node, &mut upload, 0, 8), Ok(8));
    // Accepted once: the cursor has moved past it.
    assert_eq!(
        export.write_received(&node, &mut upload, 0, 8),
        Err(Errno::EINVAL)
    );
    // A later binding of the slot fences the old writer.
    let cancel = LockResourceStep {
        transaction: 1,
        resource: LockResourceId {
            id: 1,
            generation: 1,
        },
        total_bytes: None,
    }
    .encode()
    .unwrap();
    submit(&mut export, 3, LockFileKind::ResourceCancel, &cancel);
    submit(&mut export, 4, LockFileKind::ResourceBegin, &begin(2));
    assert_eq!(
        export.write_destination(&node, &mut upload, 0, 8, 0),
        Err(Errno::ESTALE)
    );
    assert_eq!(
        export.write_received(&node, &mut upload, 0, 8),
        Err(Errno::ESTALE)
    );
}

#[test]
fn events_are_read_and_acknowledged_through_the_files() {
    let mut export = attached();
    negotiate(&mut export);
    let mut events = export.open(&Node::Events, READ).unwrap();
    let bytes = read_all(&mut export, Node::Events, &mut events);
    let mut kinds = Vec::new();
    let mut rest = bytes.as_slice();
    while !rest.is_empty() {
        let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        kinds.push(
            decode_lock_file_record(&rest[..size], LockFileClass::Event)
                .unwrap()
                .header
                .kind,
        );
        rest = &rest[size..];
    }
    assert_eq!(
        kinds,
        [
            LockFileKind::Submitted,
            LockFileKind::Negotiated,
            LockFileKind::ObjectPublished
        ]
    );
    let ack = encode_lock_file_ack(LockFileAck {
        connection_epoch: EPOCH,
        sequence: 3,
    })
    .unwrap();
    let mut handle = export.open(&Node::Ack, WRITE).unwrap();
    export.write(&Node::Ack, &mut handle, 0, &ack).unwrap();
    assert_eq!(export.custody().position().records, 0);
    let foreign = encode_lock_file_ack(LockFileAck {
        connection_epoch: EPOCH + 1,
        sequence: 3,
    })
    .unwrap();
    assert_eq!(
        export.write(&Node::Ack, &mut handle, 0, &foreign),
        Err(Errno::ESTALE)
    );
}

#[test]
fn a_revoked_export_answers_nothing() {
    let mut export = attached();
    export.revoke();
    assert_eq!(export.open(&Node::Api, READ).err(), Some(Errno::ESTALE));
    assert_eq!(export.bind_connection(ConnectionId(3)), Err(Errno::EACCES));
}

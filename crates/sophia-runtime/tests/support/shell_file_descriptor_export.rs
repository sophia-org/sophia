//! Export custody and immutable-object tests. These exercise the production
//! export directly; protected role admission and owner routing are separate.
use super::*;
use sophia_protocol::*;

const EPOCH: u64 = 41;
const BASE: u64 = 3;

fn export(role: &str, caps: u64) -> ShellFiles {
    let mut export = ShellFiles::awaiting_negotiation(
        EPOCH,
        role,
        false,
        JournalBounds {
            bytes: 131072,
            reserve_bytes: 22528,
        },
        100,
        Instant::now(),
    );
    export.complete_negotiation(None, caps).unwrap();
    export
}

fn objects(generation: u64) -> Vec<ShellDescriptorRecord> {
    vec![
        ShellDescriptorRecord::Descriptors(ShellV1DescriptorSnapshot {
            connection_epoch: EPOCH,
            snapshot_generation: generation,
            output: OutputId::from_raw(3),
            output_generation: 4,
            broker_epoch: 5,
            broker_revocation_epoch: 6,
            descriptors: vec![],
        }),
        ShellDescriptorRecord::Tabs(ShellTabSnapshot {
            connection_epoch: EPOCH,
            generation,
            groups: vec![],
        }),
        ShellDescriptorRecord::Shortcuts(ShellShortcutCatalog {
            connection_epoch: EPOCH,
            generation,
            entries: vec![],
        }),
    ]
}

fn record(record: ShellDescriptorRecord) -> ShellFileDescriptorRecord {
    ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(71),
        record,
    }
}

fn publish(export: &mut ShellFiles, value: ShellDescriptorRecord) -> (ShellFileKind, Vec<u8>) {
    let (kind, body) = encode_shell_file_descriptor_body(&record(value)).unwrap();
    assert_eq!(export.publish_object(kind, &body, false), Ok(true));
    (kind, body)
}

fn node(kind: ShellFileKind) -> Node {
    match kind {
        ShellFileKind::Descriptors => Node::Descriptors,
        ShellFileKind::Tabs => Node::Tabs,
        ShellFileKind::Shortcuts => Node::Shortcuts,
        _ => panic!("not a descriptor feed"),
    }
}

#[test]
fn descriptor_names_require_the_server_role_and_their_own_capability() {
    for (bit, name, kind) in [
        (0, b"descriptors".as_slice(), ShellFileKind::Descriptors),
        (2, b"tabs", ShellFileKind::Tabs),
        (3, b"shortcuts", ShellFileKind::Shortcuts),
    ] {
        for role in ["bar", "descriptor"] {
            for selected in [false, true] {
                let caps = if selected { BASE | (1 << bit) } else { 0 };
                let export = export(role, caps);
                assert_eq!(
                    export.root_entries().iter().any(|(n, _)| *n == name),
                    role == "descriptor" && selected
                );
                assert_eq!(
                    export.supports_descriptor_kind(kind),
                    role == "descriptor" && selected
                );
            }
        }
    }
    let export = export("descriptor", BASE | (1 << 5) | (1 << 9));
    let names: Vec<_> = export
        .root_entries()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(names.contains(&b"catalog".as_slice()));
    assert!(names.contains(&b"indicators".as_slice()));
    for absent in [
        b"limits".as_slice(),
        b"outputs",
        b"upload",
        b"tabs",
        b"shortcuts",
    ] {
        assert!(!names.contains(&absent));
    }
}

#[test]
fn descriptor_object_pins_survive_republication_without_qid_aliases() {
    for (old, new) in objects(1).into_iter().zip(objects(2)) {
        let mut export = export("descriptor", BASE | 4 | 8);
        let (kind, _) = publish(&mut export, old);
        let node = node(kind);
        let mut pin = export.open(&node, OpenFlags(0)).unwrap();
        let old_qid = export.describe(&node, Some(&pin)).qid_path;
        assert!(old_qid >= 132);
        let old_bytes = export.read(&node, &mut pin, 0, 65536).unwrap();
        publish(&mut export, new);
        assert_eq!(export.read(&node, &mut pin, 0, 65536).unwrap(), old_bytes);
        assert_eq!(export.describe(&node, Some(&pin)).qid_path, old_qid);
        assert!(export.describe(&node, None).qid_path > old_qid);
        assert!(matches!(export.open(&node, OpenFlags(0)), Err(EBUSY)));
        export.release(node, Some(pin));
        let mut latest = export.open(&node, OpenFlags(0)).unwrap();
        assert_ne!(
            export.read(&node, &mut latest, 0, 65536).unwrap(),
            old_bytes
        );
        export.release(node, Some(latest));
    }
}

#[test]
fn refused_publication_spends_no_qid_or_journal_capacity() {
    let (kind, mut body) =
        encode_shell_file_descriptor_body(&record(objects(1).remove(0))).unwrap();
    let mut bar = export("bar", BASE);
    let qid = bar.next_qid();
    assert_eq!(bar.publish_object(kind, &body, false), Err(Errno::EACCES));
    assert_eq!(bar.next_qid(), qid);
    assert_eq!(bar.journal.size(), 0);
    let mut descriptor = export("descriptor", BASE);
    // Domain epoch follows the domain transaction in the native body.
    body[8..16].copy_from_slice(&(EPOCH + 1).to_le_bytes());
    assert_eq!(
        descriptor.publish_object(kind, &body, false),
        Err(Errno::EINVAL)
    );
    assert_eq!(descriptor.next_qid(), qid);
    assert_eq!(descriptor.journal.size(), 0);
    body[8..16].copy_from_slice(&EPOCH.to_le_bytes());
    let submitted = encode_shell_file_submitted_body(ShellFileSubmitted {
        submission_id: 1,
        candidate_kind: ShellFileKind::Negotiate,
    })
    .unwrap();
    while descriptor
        .journal
        .append(ShellFileKind::Submitted, &submitted, false)
        .is_ok()
    {}
    let size = descriptor.journal.size();
    assert_eq!(descriptor.publish_object(kind, &body, false), Ok(false));
    assert_eq!(descriptor.next_qid(), qid);
    assert_eq!(descriptor.journal.size(), size);
    assert!(descriptor.object_slot(kind).unwrap().current.is_none());
}

fn candidate() -> ShellFileDescriptorRecord {
    record(ShellDescriptorRecord::TabsCandidate(ShellTabCandidate {
        connection_epoch: EPOCH,
        snapshot_generation: 2,
        candidate_generation: 3,
        groups: (1..=1024).collect(),
    }))
}

fn stage(export: &mut ShellFiles, value: &ShellFileDescriptorRecord) -> (Handle, Vec<u8>) {
    let kind = shell_file_descriptor_kind(&value.record);
    let bytes = encode_shell_file_descriptor(
        ShellFileHeader {
            kind,
            connection_epoch: EPOCH,
            submission_id: 1,
            sequence: 0,
        },
        value,
    )
    .unwrap();
    let mut handle = export.open(&Node::Transaction, OpenFlags(2)).unwrap();
    export
        .write(&Node::Transaction, &mut handle, 0, &bytes)
        .unwrap();
    let submit = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: EPOCH,
        submission_id: 1,
        candidate_bytes: bytes.len() as u32,
    })
    .unwrap();
    (handle, submit)
}

#[test]
fn whole_candidate_custody_is_once_only_and_blocks_reopen_until_ack() {
    let mut export = export("descriptor", BASE | 4);
    let expected = candidate();
    let (handle, submit) = stage(&mut export, &expected);
    export.submit(&submit).unwrap();
    export.submit(&submit).unwrap();
    assert_eq!(export.inbound.len(), 1);
    let Inbound::Descriptor(actual) = export.take_inbound().unwrap() else {
        panic!("typed descriptor custody")
    };
    assert_eq!(*actual, expected);
    assert!(export.inbound_is_empty());
    assert_eq!(export.journal.records(), 1);
    export.release(Node::Transaction, Some(handle));
    assert!(matches!(
        export.open(&Node::Transaction, OpenFlags(2)),
        Err(EBUSY)
    ));
    export
        .write(
            &Node::Ack,
            &mut Handle::Plain,
            0,
            &encode_shell_file_ack(ShellFileAck {
                connection_epoch: EPOCH,
                sequence: 1,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(export.open(&Node::Transaction, OpenFlags(2)).is_ok());
}

#[test]
fn candidate_authority_is_checked_before_custody() {
    for (role, caps) in [("bar", BASE | 4), ("descriptor", BASE)] {
        let mut export = export(role, caps);
        let (_, submit) = stage(&mut export, &candidate());
        assert_eq!(export.submit(&submit), Err(Errno::EACCES));
        assert!(export.inbound_is_empty());
        assert_eq!(export.journal.records(), 0);
        assert_eq!(export.submission_watermark, 0);
    }
}

#[test]
fn backpressure_refuses_custody_and_allows_same_id_after_capacity_returns() {
    let mut export = export("descriptor", BASE | 4);
    let expected = candidate();
    export
        .inbound
        .extend((0..INBOUND_RECORDS).map(|_| Inbound::Descriptor(Box::new(expected.clone()))));
    let (_, submit) = stage(&mut export, &expected);
    assert_eq!(export.submit(&submit), Err(Errno::EAGAIN));
    assert_eq!(export.inbound.len(), INBOUND_RECORDS);
    assert_eq!(export.submission_watermark, 0);
    assert_eq!(export.journal.records(), 0);
    export.take_inbound().unwrap();
    export.submit(&submit).unwrap();
    assert_eq!(export.inbound.len(), INBOUND_RECORDS);
    assert_eq!(export.submission_watermark, 1);
    assert_eq!(export.journal.records(), 1);
}

#[test]
fn descriptor_events_validate_role_epoch_and_shape_before_journaling() {
    let value = record(ShellDescriptorRecord::DescriptorOutcome(
        ShellV1CandidateOutcome {
            connection_epoch: EPOCH,
            candidate_generation: 3,
            presentation_epoch: 0,
            kind: ShellV1CandidateOutcomeKind::Prepared,
        },
    ));
    let (kind, mut body) = encode_shell_file_descriptor_body(&value).unwrap();
    let mut bar = export("bar", BASE);
    assert_eq!(bar.append_event(kind, &body, true), Err(Errno::EACCES));
    assert_eq!(bar.journal.records(), 0);
    let mut descriptor = export("descriptor", BASE);
    body[8..16].copy_from_slice(&(EPOCH + 1).to_le_bytes());
    assert_eq!(
        descriptor.append_event(kind, &body, true),
        Err(Errno::EINVAL)
    );
    assert_eq!(descriptor.journal.records(), 0);
    body[8..16].copy_from_slice(&EPOCH.to_le_bytes());
    body.push(0);
    assert_eq!(
        descriptor.append_event(kind, &body, true),
        Err(Errno::EINVAL)
    );
    assert_eq!(descriptor.journal.records(), 0);
    body.pop();
    assert_eq!(descriptor.append_event(kind, &body, true), Ok(1));
    assert_eq!(descriptor.journal.records(), 1);
}

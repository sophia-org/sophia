//! Native descriptor role on the real SDK pipeline against the scripted 9P
//! peer. This is client evidence; production export and C independence are
//! separate gates.
#[allow(dead_code)]
#[path = "support/file_wire_peer.rs"]
mod file_wire_peer;

use file_wire_peer::{EPOCH, Object, Peer, Profile, WAIT};
use sophia_shell_client::*;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const BASE: u64 = 3;
const METADATA: u64 =
    BASE | (1 << 2) | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 9) | (1 << 10);
fn profile(caps: u64) -> Profile {
    Profile {
        role: "descriptor",
        revision: 8,
        capabilities: caps,
        limits_published: caps & (1 << 7) != 0,
    }
}
fn connect_result(
    profile: Profile,
    required: u64,
) -> (Result<ShellConnection, ShellClientError>, Peer) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "sdk-descriptor-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let peer = std::thread::spawn(move || Peer::handshake(listener, profile));
    let result = ShellConnection::connect_files(
        &path,
        ShellClientOptions {
            minimum_revision: 1,
            maximum_revision: profile.revision,
            required_capabilities: required,
            handshake_timeout: WAIT,
        },
    );
    let peer = peer.join().unwrap();
    std::fs::remove_file(path).unwrap();
    (result, peer)
}
fn connect(caps: u64) -> (ShellConnection, Peer) {
    let (result, peer) = connect_result(profile(caps), caps & !2);
    (result.unwrap(), peer)
}
fn drive(
    c: &mut ShellConnection,
    p: &mut Peer,
    mut done: impl FnMut(&mut ShellConnection, &mut Peer) -> bool,
) -> Result<(), ShellClientError> {
    let end = Instant::now() + WAIT;
    loop {
        p.pump();
        c.poll_io()?;
        p.pump();
        if done(c, p) {
            return Ok(());
        }
        assert!(Instant::now() < end, "descriptor exchange stalled");
    }
}
fn fail(c: &mut ShellConnection, p: &mut Peer) -> ShellClientError {
    drive(c, p, |_, _| false).unwrap_err()
}
fn take(c: &mut ShellConnection, p: &mut Peer) -> DescriptorObservation {
    let mut result = None;
    drive(c, p, |c, _| {
        result = c.take_descriptor_observation().unwrap();
        result.is_some()
    })
    .unwrap();
    result.unwrap()
}
fn descriptor(slot: u16) -> ShellV1Descriptor {
    ShellV1Descriptor {
        slot,
        generation: 4,
        label: Some(DisplayLabel {
            text: "L".repeat(128),
            redacted: false,
        }),
        trust_level: TrustLevel::Trusted,
        attention: AttentionState::None,
        action: ToplevelActionCapabilityRef {
            token: 5,
            issuer_epoch: 6,
            issuer_revocation_epoch: 7,
            recipient_epoch: EPOCH,
            target_slot: slot,
            target_generation: 4,
        },
    }
}
fn snapshots() -> Vec<(&'static str, ShellDescriptorRecord)> {
    vec![
        (
            "descriptors",
            ShellDescriptorRecord::Descriptors(ShellV1DescriptorSnapshot {
                connection_epoch: EPOCH,
                snapshot_generation: 9,
                output: OutputId::from_raw(3),
                output_generation: 4,
                broker_epoch: 6,
                broker_revocation_epoch: 7,
                descriptors: (1..=16).map(descriptor).collect(),
            }),
        ),
        (
            "tabs",
            ShellDescriptorRecord::Tabs(ShellTabSnapshot {
                connection_epoch: EPOCH,
                generation: 9,
                groups: (1..=1024)
                    .map(|n| ShellTabGroup {
                        slot: u64::from(n),
                        output: OutputId::from_raw(3),
                        focused: false,
                        selected_slot: Some(n * 2 - 1),
                        entries: vec![descriptor(n * 2 - 1), descriptor(n * 2)],
                    })
                    .collect(),
            }),
        ),
        (
            "shortcuts",
            ShellDescriptorRecord::Shortcuts(ShellShortcutCatalog {
                connection_epoch: EPOCH,
                generation: 9,
                entries: (1..=256)
                    .map(|slot| ShellShortcut {
                        slot,
                        chord: "K".repeat(64),
                        action: "A".repeat(128),
                        label: Some("L".repeat(128)),
                        group: Some("G".repeat(64)),
                    })
                    .collect(),
            }),
        ),
    ]
}
fn publish(
    p: &mut Peer,
    node: &str,
    record: ShellDescriptorRecord,
    generation: u64,
    qid: u64,
) -> ShellFileDescriptorRecord {
    let value = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(71),
        record,
    };
    let kind = shell_file_descriptor_kind(&value.record);
    let bytes = encode_shell_file_descriptor(
        ShellFileHeader {
            kind,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: 0,
        },
        &value,
    )
    .unwrap();
    p.objects.insert(
        node.into(),
        Object {
            qid,
            bytes,
            chunk: 4096,
        },
    );
    let announcement = p.published(kind, generation, qid);
    p.push(announcement);
    value
}
fn activation() -> ShellLauncherActivation {
    ShellLauncherActivation {
        connection_epoch: EPOCH,
        catalog_generation: 2,
        request_generation: 3,
        candidate_generation: 4,
        presentation_epoch: 5,
        activation: 6,
        slot: 7,
    }
}
fn events() -> Vec<ShellDescriptorRecord> {
    use ShellDescriptorRecord as R;
    vec![
        R::DescriptorOutcome(ShellV1CandidateOutcome {
            connection_epoch: EPOCH,
            candidate_generation: 2,
            presentation_epoch: 3,
            kind: ShellV1CandidateOutcomeKind::Presented,
        }),
        R::DescriptorActivation(ShellV1Activation {
            connection_epoch: EPOCH,
            candidate_generation: 2,
            presentation_epoch: 3,
            activation: 4,
            action: descriptor(1).action,
        }),
        R::ReferenceRequest(ShellReferenceRequest {
            connection_epoch: EPOCH,
            catalog_generation: 2,
            request_generation: 3,
            output: OutputId::from_raw(4),
            output_generation: 5,
            presentation_epoch: 0,
            operation: ShellReferenceOperation::Startup,
        }),
        R::ReferenceOutcome(ShellReferenceOutcome {
            connection_epoch: EPOCH,
            catalog_generation: 2,
            request_generation: 3,
            candidate_generation: 4,
            presentation_epoch: 5,
            page: 0,
            pages: 1,
            kind: ShellV1CandidateOutcomeKind::Presented,
        }),
        R::LauncherRequest(ShellLauncherRequest {
            connection_epoch: EPOCH,
            catalog_generation: 2,
            request_generation: 3,
            output: OutputId::from_raw(4),
            output_generation: 5,
            presentation_epoch: 0,
            operation: ShellLauncherOperation::Open,
            query: "query".into(),
        }),
        R::LauncherOutcome(ShellLauncherOutcome {
            connection_epoch: EPOCH,
            request_generation: 3,
            candidate_generation: 4,
            presentation_epoch: 5,
            kind: ShellV1CandidateOutcomeKind::Presented,
        }),
        R::LauncherActivation(activation()),
        R::LaunchOutcome(ShellLaunchOutcome {
            activation: activation(),
            status: ShellLaunchStatus::Started,
        }),
    ]
}
fn candidates() -> Vec<ShellDescriptorRecord> {
    use ShellDescriptorRecord as R;
    vec![
        R::DescriptorCandidate(ShellV1Candidate {
            connection_epoch: EPOCH,
            snapshot_generation: 2,
            candidate_generation: 3,
            output: OutputId::from_raw(4),
            visible: false,
            selected_slot: None,
            reservation: None,
            entries: vec![],
        }),
        R::DescriptorActivationAck(ShellV1ActivationAck {
            connection_epoch: EPOCH,
            activation: 4,
            disposition: ShellV1ActivationDisposition::Consumed,
        }),
        R::TabsCandidate(ShellTabCandidate {
            connection_epoch: EPOCH,
            snapshot_generation: 2,
            candidate_generation: 3,
            groups: (1..=1024).collect(),
        }),
        R::ReferenceCandidate(ShellReferenceCandidate {
            connection_epoch: EPOCH,
            catalog_generation: 2,
            request_generation: 3,
            candidate_generation: 4,
            output: OutputId::from_raw(5),
            visible: false,
            page: 0,
            style: ShellReferenceStyle {
                body_size: 12,
                title_size: 16,
                padding: 4,
                row_gap: 2,
                key_gap: 3,
                column_gap: 8,
                border: 1,
                margin: 6,
                columns: 2,
                colors: [0xff223344; 6],
                title: "Title".into(),
            },
            entries: vec![],
        }),
        R::LauncherCandidate(ShellLauncherCandidate {
            connection_epoch: EPOCH,
            catalog_generation: 2,
            request_generation: 3,
            candidate_generation: 4,
            output: OutputId::from_raw(5),
            visible: true,
            selected: 0,
            entries: vec![],
            font_size: 14,
            colors: [0xff223344; 4],
        }),
        R::LauncherActivationAck(ShellLauncherActivationAck {
            activation: activation(),
            consumed: false,
        }),
    ]
}

#[test]
fn metadata_bootstrap_acks_custody_and_never_walks_limits() {
    for revision in 1..=8 {
        let mut p = profile(BASE);
        p.revision = revision;
        let (c, peer) = connect_result(p, 1);
        let mut c = c.unwrap();
        assert_eq!(c.welcome().capabilities, BASE);
        assert_eq!(
            peer.acks.iter().map(|a| a.sequence).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(peer.walk_count("limits"), 0);
        assert!(c.take_content().unwrap().is_none());
    }
}

#[test]
fn combined_descriptor_and_content_waits_for_the_exact_limits() {
    let (mut c, p) = connect(METADATA | (1 << 7) | (1 << 8));
    assert_eq!(p.walk_count("limits"), 1);
    let (_, ShellContentRecord::Limits(limits)) = c.take_content().unwrap().unwrap() else {
        panic!("Limits before ready")
    };
    assert_eq!(limits.grant.connection_epoch, EPOCH);
    assert!(c.take_descriptor_observation().unwrap().is_none());
}

#[test]
fn descriptor_negotiation_refuses_incoherent_grants_before_any_content_walk() {
    let cases = [
        (profile(BASE | 4), 1),                     // unsolicited optional bit
        (profile(BASE | (1 << 4)), 1 | (1 << 4)),   // missing shortcuts
        (profile(BASE | (1 << 6)), 1 | (1 << 6)),   // missing catalog
        (profile(BASE | (1 << 8)), 1 | (1 << 8)),   // missing content
        (profile(BASE | (1 << 10)), 1 | (1 << 10)), // missing indicators
        (profile(BASE | (1 << 11)), 1 | (1 << 11)), // native profile is separate
        (profile(BASE | (1 << 12)), 1 | (1 << 12)), // persistent profile is separate
        (
            Profile {
                revision: 1,
                ..profile(BASE | 4)
            },
            1 | 4,
        ),
        (
            Profile {
                limits_published: true,
                ..profile(BASE)
            },
            1,
        ),
        (
            Profile {
                limits_published: false,
                ..profile(BASE | (1 << 7))
            },
            1 | (1 << 7),
        ),
        (profile(BASE), 0), // descriptor role requires explicit bit 0
    ];
    for (p, required) in cases {
        let (c, peer) = connect_result(p, required);
        assert!(c.is_err(), "{p:?}");
        assert_eq!(peer.walk_count("limits"), 0);
    }
}

#[test]
fn complete_snapshot_is_fetched_before_the_covering_ack() {
    for (node, record) in snapshots() {
        let (mut c, mut p) = connect(METADATA);
        p.reset_logs();
        p.hold_object_reads = true;
        let expected = publish(&mut p, node, record, 9, 88);
        drive(&mut c, &mut p, |_, p| {
            p.opens.iter().any(|(n, _)| n == node)
        })
        .unwrap();
        for _ in 0..8 {
            p.pump();
            c.poll_io().unwrap();
        }
        assert!(c.take_descriptor_observation().unwrap().is_none());
        assert!(p.acks.iter().all(|a| a.sequence < 3));
        p.hold_object_reads = false;
        p.release_object_reads();
        let DescriptorObservation::Record(actual) = take(&mut c, &mut p) else {
            panic!("descriptor snapshot")
        };
        assert_eq!(actual, expected);
        drive(&mut c, &mut p, |_, p| {
            p.acks.iter().any(|a| a.sequence == 3)
        })
        .unwrap();
        assert!(p.object_reads.len() > 1);
        assert_eq!(p.object_reads.last().unwrap().returned, 0);
        assert_eq!(p.acks.last().unwrap().after_reads, p.object_reads.len());
    }
}

#[test]
fn descriptor_events_preserve_order_and_transaction_identity() {
    let (mut c, mut p) = connect(METADATA);
    let expected = events();
    for (i, record) in expected.iter().enumerate() {
        let value = ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(100 + i as u64),
            record: record.clone(),
        };
        let (kind, body) = encode_shell_file_descriptor_body(&value).unwrap();
        let bytes = p.event(kind, &body);
        p.push(bytes);
    }
    for (i, record) in expected.into_iter().enumerate() {
        let DescriptorObservation::Record(value) = take(&mut c, &mut p) else {
            panic!("event")
        };
        assert_eq!(value.transaction.raw(), 100 + i as u64);
        assert_eq!(value.record, record);
    }
}

#[test]
fn every_candidate_uses_existing_submitted_custody_and_native_bytes() {
    let (mut c, mut p) = connect(METADATA);
    p.reset_logs();
    for (i, mut record) in candidates().into_iter().enumerate() {
        if let ShellDescriptorRecord::ReferenceCandidate(candidate) = &mut record {
            candidate.entries = (1..=256)
                .map(|slot| ShellReferenceEntry {
                    slot,
                    key: "K".repeat(64),
                    label: "L".repeat(128),
                })
                .collect();
        }
        let value = ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(100 + i as u64),
            record,
        };
        let kind = shell_file_descriptor_kind(&value.record);
        let admission = c.enqueue_descriptor_tracked(&value).unwrap();
        assert_eq!(admission.count, 1);
        assert_eq!(c.custody(admission.first), Some(Custody::Queued));
        drive(&mut c, &mut p, |_, p| !p.submits.is_empty()).unwrap();
        let held = p.submits.pop_front().unwrap();
        let submit = decode_shell_file_submit(&held.data).unwrap();
        assert_eq!(
            decode_shell_file_descriptor(p.transactions.last().unwrap(), kind).unwrap(),
            value
        );
        assert_eq!(c.custody(admission.first), Some(Custody::InFlight));
        p.answer_write(held.tag, held.data.len() as u32);
        let event = p.submitted(submit.submission_id, kind);
        p.push(event);
        drive(&mut c, &mut p, |c, _| {
            c.custody(admission.first) == Some(Custody::Submitted)
        })
        .unwrap();
    }
}

#[test]
fn plain_catalog_needs_neither_launcher_nor_content_and_refuses_persistent_identities() {
    for persistent in [false, true] {
        let (mut c, mut p) = connect(BASE | (1 << 5));
        assert_eq!(p.walk_count("limits"), 0);
        let catalog = ShellApplicationCatalog {
            connection_epoch: EPOCH,
            generation: 9,
            entries: vec![ShellApplicationDescriptor {
                slot: 1,
                available: true,
                label: "App".into(),
                keywords: "app".into(),
            }],
        };
        let identities = if persistent {
            [(1, "desktop:app.desktop".to_owned())].into()
        } else {
            Default::default()
        };
        let bytes = encode_shell_file_catalog(
            ShellFileHeader {
                kind: ShellFileKind::Catalog,
                connection_epoch: EPOCH,
                submission_id: 0,
                sequence: 0,
            },
            &ShellFileCatalog {
                transaction: TransactionId::from_raw(71),
                catalog: ShellPersistentCatalog {
                    catalog: catalog.clone(),
                    identities,
                },
            },
        )
        .unwrap();
        p.objects.insert(
            "catalog".into(),
            Object {
                qid: 88,
                bytes,
                chunk: 37,
            },
        );
        let event = p.published(ShellFileKind::Catalog, 9, 88);
        p.push(event);
        if persistent {
            assert_eq!(
                fail(&mut c, &mut p),
                ShellClientError::Protocol("catalog identity or profile")
            );
        } else {
            let DescriptorObservation::Catalog(tx, actual) = take(&mut c, &mut p) else {
                panic!("plain catalog")
            };
            assert_eq!(tx.raw(), 71);
            assert_eq!(actual, catalog);
        }
    }
}

#[test]
fn indicators_and_their_activation_work_without_content_admission() {
    let (mut c, mut p) = connect(BASE | (1 << 9) | (1 << 10));
    let snapshot = ShellIndicatorSnapshot {
        connection_epoch: EPOCH,
        generation: 9,
        active_output: Some(OutputId::from_raw(3)),
        statuses: vec![ShellOutputStatus {
            output: OutputId::from_raw(3),
            focus_bits: 1,
            layout: "tile".into(),
        }],
        indicators: vec![ShellIndicator {
            output: OutputId::from_raw(3),
            indicator: 4,
            action: 5,
            slot: 1,
            state_bits: 1,
            label: "one".into(),
        }],
    };
    let bytes = encode_shell_file_indicators(
        ShellFileHeader {
            kind: ShellFileKind::Indicators,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: 0,
        },
        &ShellFileIndicators {
            transaction: TransactionId::from_raw(71),
            snapshot: snapshot.clone(),
        },
    )
    .unwrap();
    p.objects.insert(
        "indicators".into(),
        Object {
            qid: 88,
            bytes,
            chunk: 23,
        },
    );
    let event = p.published(ShellFileKind::Indicators, 9, 88);
    p.push(event);
    let mut result = None;
    drive(&mut c, &mut p, |c, _| {
        result = c.take_indicators().unwrap();
        result.is_some()
    })
    .unwrap();
    assert_eq!(result.unwrap().1, snapshot);
    assert_eq!(p.walk_count("limits"), 0);
    let value = ShellIndicatorActivation {
        connection_epoch: EPOCH,
        snapshot_generation: 9,
        output: OutputId::from_raw(3),
        indicator: 4,
        action: 5,
        event_id: 6,
    };
    let admitted = c
        .enqueue_indicator_activation_tracked(TransactionId::from_raw(72), &value)
        .unwrap();
    drive(&mut c, &mut p, |_, p| !p.submits.is_empty()).unwrap();
    let held = p.submits.pop_front().unwrap();
    let submission = decode_shell_file_submit(&held.data).unwrap();
    assert_eq!(
        decode_shell_file_indicator_activate(p.transactions.last().unwrap())
            .unwrap()
            .activation,
        value
    );
    p.answer_write(held.tag, held.data.len() as u32);
    let event = p.submitted(submission.submission_id, ShellFileKind::IndicatorActivate);
    p.push(event);
    drive(&mut c, &mut p, |c, _| {
        c.custody(admitted.first) == Some(Custody::Submitted)
    })
    .unwrap();
}

#[test]
fn descriptor_bulk_saturation_leaves_ack_capacity_and_no_ticket_on_refusal() {
    let (mut c, _p) = connect(METADATA);
    let value = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(8),
        record: candidates().remove(0),
    };
    let mut last = None;
    for _ in 0..32 {
        last = Some(c.enqueue_descriptor_tracked(&value).unwrap().first);
    }
    assert_eq!(
        c.enqueue_descriptor_tracked(&value),
        Err(ShellClientError::QueueSaturated)
    );
    let ack = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(9),
        record: candidates().remove(1),
    };
    let admission = c.enqueue_descriptor_tracked(&ack).unwrap();
    assert_eq!(admission.first.0, last.unwrap().0 + 1);
    assert_eq!(c.custody(admission.first), Some(Custody::Queued));
}

#[test]
fn metadata_role_rejects_content_and_wrong_direction_before_any_submission() {
    let (mut c, mut p) = connect(BASE);
    let event = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(8),
        record: events().remove(0),
    };
    assert_eq!(
        c.enqueue_descriptor_tracked(&event),
        Err(ShellClientError::WrongDirection)
    );
    // Even a structurally invalid content request is rejected by capability first.
    assert_eq!(
        c.enqueue_content(
            TransactionId::from_raw(8),
            &ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant: ContentGrant {
                    connection_epoch: EPOCH,
                    content_grant_epoch: 1
                },
                resource: ContentResourceId {
                    id: 1,
                    generation: 1
                },
                total_bytes: 0,
                chunk_count: 0,
            })
        ),
        Err(ShellClientError::MissingCapability)
    );
    c.poll_io().unwrap();
    p.pump();
    assert!(p.transactions.is_empty());
    assert!(p.submits.is_empty());
}

#[test]
fn superseded_snapshots_keep_one_decoded_value_and_release_only_after_latest_fetch() {
    let (mut c, mut p) = connect(BASE);
    p.reset_logs();
    let (node, mut record) = snapshots().remove(0);
    publish(&mut p, node, record.clone(), 9, 88);
    let ShellDescriptorRecord::Descriptors(ref mut snapshot) = record else {
        unreachable!()
    };
    snapshot.snapshot_generation = 10;
    let latest = publish(&mut p, node, record, 10, 89);
    let DescriptorObservation::Record(actual) = take(&mut c, &mut p) else {
        panic!("snapshot")
    };
    assert_eq!(actual, latest);
    drive(&mut c, &mut p, |_, p| {
        p.acks.iter().any(|a| a.sequence == 4)
    })
    .unwrap();
    assert_eq!(p.walk_count(node), 1);
    assert!(c.take_descriptor_observation().unwrap().is_none());
    assert!(
        p.acks
            .iter()
            .all(|a| a.sequence < 3 || a.after_reads == p.object_reads.len())
    );
}

#[test]
fn a_bar_with_bit_zero_cannot_send_or_receive_descriptor_records() {
    let p = Profile {
        role: "bar",
        revision: 6,
        capabilities: 1 | (1 << 7),
        limits_published: false,
    };
    let (c, mut peer) = connect_result(p, p.capabilities);
    let mut c = c.unwrap();
    assert_eq!(
        c.enqueue_descriptor_tracked(&ShellFileDescriptorRecord {
            transaction: TransactionId::from_raw(8),
            record: candidates().remove(0)
        }),
        Err(ShellClientError::MissingCapability)
    );
    let (node, value) = snapshots().remove(0);
    publish(&mut peer, node, value, 9, 88);
    assert_eq!(
        fail(&mut c, &mut peer),
        ShellClientError::Protocol("announced object kind")
    );
    assert_eq!(peer.walk_count(node), 0);
}

#[test]
fn absent_family_is_refused_locally_and_before_object_fetch() {
    for (node, value) in snapshots().into_iter().skip(1) {
        let (mut c, mut p) = connect(BASE);
        publish(&mut p, node, value, 9, 88);
        assert_eq!(
            fail(&mut c, &mut p),
            ShellClientError::Protocol("announced object kind")
        );
        assert_eq!(p.walk_count(node), 0);
    }
    let (mut c, mut p) = connect(BASE);
    for record in candidates().into_iter().skip(2) {
        assert_eq!(
            c.enqueue_descriptor_tracked(&ShellFileDescriptorRecord {
                transaction: TransactionId::from_raw(8),
                record
            }),
            Err(ShellClientError::MissingCapability)
        );
    }
    let value = ShellFileDescriptorRecord {
        transaction: TransactionId::from_raw(8),
        record: events().remove(2),
    };
    let (kind, body) = encode_shell_file_descriptor_body(&value).unwrap();
    let bytes = p.event(kind, &body);
    p.push(bytes);
    assert_eq!(fail(&mut c, &mut p), ShellClientError::MissingCapability);
}

#[test]
fn malformed_snapshot_never_releases_its_ack_hold() {
    for wrong_epoch in [false, true] {
        let (mut c, mut p) = connect(BASE);
        p.reset_logs();
        let (node, record) = snapshots().remove(0);
        publish(&mut p, node, record, 9, 88);
        let bytes = &mut p.objects.get_mut(node).unwrap().bytes;
        let (at, value) = if wrong_epoch {
            (40, EPOCH + 1)
        } else {
            (48, 10)
        };
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        let error = fail(&mut c, &mut p);
        if wrong_epoch {
            assert!(matches!(error, ShellClientError::FileCodec(_)));
        } else {
            assert_eq!(
                error,
                ShellClientError::Protocol(
                    "snapshot object generation differs from its announcement"
                )
            );
        }
        assert_eq!(c.poll_io(), Err(error));
        assert!(c.take_descriptor_observation().unwrap().is_none());
        assert!(p.acks.iter().all(|a| a.sequence < 3));
    }
}

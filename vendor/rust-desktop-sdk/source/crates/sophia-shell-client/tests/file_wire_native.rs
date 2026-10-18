//! Independent scripted 9P peer; no Session implementation or live desktop.
#[allow(dead_code)]
#[path = "support/file_wire_peer.rs"]
mod peer;
use peer::{CONTENT_GRANT_EPOCH, EPOCH, Object, Peer, Profile, WAIT};
use sophia_shell_client::*;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const CAPS: u64 = (1 << 5) | (1 << 7) | (1 << 8) | (1 << 11);
#[path = "support/native_lifecycle.rs"]
mod lifecycle;
#[path = "support/native_session.rs"]
mod session;

#[test]
fn delayed_native_batch_keeps_credits_on_error_and_submits_both_records() {
    let (mut c, mut p) = ready();
    let token = c.reserve_capacity(2, 4096).unwrap();
    c.dispatch(Instant::now(), 0).unwrap();
    let invalid = record(ShellNativeLauncherRecord::Focus(binding()));
    assert!(
        c.commit_reserved_shell_batch(&token, &[ShellBatchItem::Native(&invalid)])
            .is_err()
    );
    let activate = ShellFileNativeLauncherRecord {
        transaction: tx(50),
        record: ShellNativeLauncherRecord::Activate(activation()),
    };
    let ack = ShellFileNativeLauncherRecord {
        transaction: tx(51),
        record: ShellNativeLauncherRecord::InputAck(ack()),
    };
    let a = c
        .commit_reserved_shell_batch(
            &token,
            &[
                ShellBatchItem::Native(&activate),
                ShellBatchItem::Native(&ack),
            ],
        )
        .unwrap();
    assert_eq!(a.first, Ticket(1));
    assert_eq!(a.count, 2);
    assert!(c.cancel_reservation(&token).is_err());
    assert_eq!(
        decode_shell_file_native_launcher_transaction(
            &commit(&mut c, &mut p, a.first),
            ShellFileKind::NativeActivate
        )
        .unwrap(),
        activate
    );
    assert_eq!(
        decode_shell_file_native_launcher_transaction(
            &commit(&mut c, &mut p, Ticket(2)),
            ShellFileKind::NativeInputAck
        )
        .unwrap(),
        ack
    );
}

#[test]
fn native_prepared_batch_drop_and_delayed_size_failure_admit_nothing() {
    let (mut c, mut p) = ready();
    let ack = record(ShellNativeLauncherRecord::InputAck(ack()));
    drop(
        c.reserve_shell_batch(&[ShellBatchItem::Native(&ack)])
            .unwrap(),
    );
    let token = c.reserve_capacity(1, 1).unwrap();
    assert!(
        c.commit_reserved_shell_batch(&token, &[ShellBatchItem::Native(&ack)])
            .is_err()
    );
    c.cancel_reservation(&token).unwrap();
    let a = c
        .enqueue_shell_batch(&[ShellBatchItem::Native(&ack)])
        .unwrap();
    assert_eq!(a.first, Ticket(1));
    commit(&mut c, &mut p, a.first);
    assert_eq!(p.transactions.len(), 1);
}

#[test]
fn allocation_and_whole_native_candidate_reach_submitted() {
    let (mut c, mut p) = ready();
    let b = binding();
    let allocation = record(ShellNativeLauncherRecord::AllocationRequest(
        NativeLauncherAllocationRequest {
            grant: b.grant,
            opening: b.opening,
            output: b.output,
            request_id: 12,
            prior: ContentAllocationId::default(),
            operation: 1,
            edge: 2,
            desired_width: 100,
            desired_height: 50,
            margins: ContentMargins::default(),
        },
    ));
    let admitted = c.enqueue_native_launcher_tracked(&allocation).unwrap();
    let bytes = commit(&mut c, &mut p, admitted.first);
    assert_eq!(
        decode_shell_file_native_launcher_transaction(
            &bytes,
            ShellFileKind::NativeAllocationRequest
        )
        .unwrap(),
        allocation
    );
    let candidate = ShellFileNativeCandidate {
        transaction: tx(40),
        candidate: NativeContentCandidate {
            candidate: ContentCandidate {
                grant: b.grant,
                candidate_generation: b.candidate_generation,
                output: b.output,
                facts_generation: 1,
                pacing_permit: 1,
                interaction_generation: b.interaction_generation,
                surfaces: vec![ContentSurface {
                    allocation: b.allocation,
                    scale_generation: 1,
                    role: 3,
                    edge: 2,
                    margins: ContentMargins::default(),
                    reservation_extent: 0,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                placements: vec![ContentPlacement {
                    resource: ContentResourceId {
                        id: 1,
                        generation: 1,
                    },
                    surface_index: 0,
                    destination_x_px: 0,
                    destination_y_px: 0,
                }],
                targets: vec![],
            },
            opening: b.opening,
            catalog_generation: b.catalog_generation,
            state_revision: 1,
            selected: 0,
            rows: vec![],
        },
    };
    let mut invalid = candidate.clone();
    invalid.candidate.selected = 1;
    assert!(c.enqueue_native_candidate_tracked(&invalid).is_err());
    let admitted = c.enqueue_native_candidate_tracked(&candidate).unwrap();
    assert_eq!(admitted.first, Ticket(2));
    let bytes = commit(&mut c, &mut p, admitted.first);
    assert_eq!(
        decode_shell_file_native_candidate(&bytes).unwrap(),
        candidate
    );
}

#[test]
fn saturated_response_admits_neither_record() {
    let (mut c, _p) = ready();
    for n in 1..=63 {
        c.enqueue_native_launcher_tracked(&ShellFileNativeLauncherRecord {
            transaction: tx(n),
            record: ShellNativeLauncherRecord::InputAck(ack()),
        })
        .unwrap();
    }
    assert_eq!(
        c.enqueue_native_input_response(tx(50), ack(), Some((tx(49), activation()))),
        Err(ShellClientError::QueueSaturated)
    );
    assert_eq!(
        c.custody_result(Ticket(64)),
        Err(CustodyLookupError::Unissued)
    );
    assert_eq!(
        c.enqueue_native_input_response(tx(50), ack(), None)
            .unwrap()
            .first,
        Ticket(64)
    );
}

#[test]
fn malformed_native_input_and_foreign_epoch_fail_closed() {
    for bad_epoch in [false, true] {
        let (mut c, mut p) = ready();
        c.take_native_launcher_observation().unwrap();
        let value = record(ShellNativeLauncherRecord::Input(NativeLauncherInput {
            event: ack().event,
            issued_mono_usec: 1000,
            kind: NativeLauncherInputKind::Accept,
            text: String::new(),
        }));
        let mut body = encode_shell_file_native_input_body(&value).unwrap();
        if !bad_epoch {
            *body.last_mut().unwrap() = 1;
        }
        let event = p.event_in(
            if bad_epoch { EPOCH + 1 } else { EPOCH },
            ShellFileKind::NativeInput,
            &body,
        );
        p.push(event);
        assert!(drive(&mut c, &mut p, |_, _| false).is_err());
        assert!(c.io_interest().is_err());
        assert!(!matches!(c.take_native_launcher_observation(), Ok(Some(_))));
    }
}
fn profile() -> Profile {
    Profile {
        role: "launcher",
        revision: 7,
        capabilities: CAPS,
        limits_published: true,
    }
}
fn connect(p: Profile, required: u64) -> (Result<ShellConnection, ShellClientError>, Peer) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "sdk-native-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let worker = std::thread::spawn(move || Peer::handshake(listener, p));
    let result = ShellConnection::connect_files(
        &path,
        ShellClientOptions {
            minimum_revision: 1,
            maximum_revision: if required & IMAGE_CAPABILITY_SEALED != 0 {
                9
            } else {
                8
            },
            required_capabilities: required,
            handshake_timeout: WAIT,
        },
    );
    let peer = worker.join().unwrap();
    std::fs::remove_file(path).unwrap();
    (result, peer)
}
fn ready() -> (ShellConnection, Peer) {
    let (c, p) = connect(profile(), CAPS);
    (c.unwrap(), p)
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
        assert!(Instant::now() < end, "native exchange stalled");
    }
}
fn tx(n: u64) -> TransactionId {
    TransactionId::from_raw(n)
}
fn binding() -> NativeLauncherBinding {
    NativeLauncherBinding {
        grant: ContentGrant {
            connection_epoch: EPOCH,
            content_grant_epoch: CONTENT_GRANT_EPOCH,
        },
        opening: 1,
        output: ContentOutputId {
            id: 2,
            generation: 3,
        },
        allocation: ContentAllocationId {
            id: 4,
            generation: 5,
        },
        catalog_generation: 6,
        candidate_generation: 7,
        presentation_epoch: 8,
        interaction_generation: 9,
        state_revision: 1,
        focus_lease: 10,
    }
}
fn ack() -> NativeLauncherInputAck {
    NativeLauncherInputAck {
        event: NativeLauncherEvent {
            binding: binding(),
            event_id: 11,
            state_revision: 1,
        },
        disposition: 1,
    }
}
fn activation() -> NativeLauncherActivation {
    NativeLauncherActivation {
        event: ack().event,
        cause: 1,
        slot: 1,
    }
}
fn record(record: ShellNativeLauncherRecord) -> ShellFileNativeLauncherRecord {
    ShellFileNativeLauncherRecord {
        transaction: tx(30),
        record,
    }
}
fn push(p: &mut Peer, record: ShellNativeLauncherRecord) {
    let value = self::record(record);
    let (kind, body) = if matches!(value.record, ShellNativeLauncherRecord::Input(_)) {
        (
            ShellFileKind::NativeInput,
            encode_shell_file_native_input_body(&value).unwrap(),
        )
    } else {
        encode_shell_file_native_launcher_transaction_body(&value).unwrap()
    };
    let bytes = p.event(kind, &body);
    p.push(bytes);
}
fn commit(c: &mut ShellConnection, p: &mut Peer, ticket: Ticket) -> Vec<u8> {
    drive(c, p, |_, p| !p.submits.is_empty()).unwrap();
    let held = p.submits.pop_front().unwrap();
    let submit = decode_shell_file_submit(&held.data).unwrap();
    let bytes = p.transactions.last().unwrap().clone();
    let header = decode_shell_file_record(&bytes, ShellFileClass::Candidate)
        .unwrap()
        .header;
    assert_eq!(header.submission_id, submit.submission_id);
    p.answer_write(held.tag, SHELL_FILE_SUBMIT_BYTES as u32);
    let event = p.submitted(submit.submission_id, header.kind);
    p.push(event);
    drive(c, p, |c, _| c.custody(ticket) == Some(Custody::Submitted)).unwrap();
    bytes
}

#[test]
fn requires_exact_native_profile_and_limits() {
    for (p, required) in [
        (
            Profile {
                revision: 6,
                ..profile()
            },
            CAPS,
        ),
        (
            Profile {
                capabilities: CAPS | 2,
                ..profile()
            },
            CAPS,
        ),
        (
            Profile {
                capabilities: CAPS & !(1 << 8),
                ..profile()
            },
            CAPS & !(1 << 8),
        ),
        (
            Profile {
                limits_published: false,
                ..profile()
            },
            CAPS,
        ),
        (
            Profile {
                role: "bar",
                ..profile()
            },
            CAPS,
        ),
        (profile(), CAPS & !(1 << 8)),
    ] {
        assert!(connect(p, required).0.is_err(), "{p:?}");
    }
    assert_eq!(ready().0.welcome().selected_revision, 7);
}

#[test]
fn isb_launcher_keeps_the_exact_role_mask_and_limits() {
    let caps = CAPS | IMAGE_CAPABILITY_SEALED;
    let image = Profile {
        revision: 9,
        capabilities: caps,
        ..profile()
    };
    assert_eq!(
        connect(image, caps).0.unwrap().welcome().selected_revision,
        9
    );
    for (p, required) in [
        (
            Profile {
                revision: 7,
                ..image
            },
            caps,
        ),
        (
            Profile {
                capabilities: caps | 2,
                ..image
            },
            caps,
        ),
        (
            Profile {
                capabilities: caps & !(1 << 8),
                ..image
            },
            caps & !(1 << 8),
        ),
        (
            Profile {
                limits_published: false,
                ..image
            },
            caps,
        ),
        (
            Profile {
                role: "bar",
                ..image
            },
            caps,
        ),
    ] {
        assert!(connect(p, required).0.is_err(), "{p:?}");
    }
}

#[test]
fn native_events_keep_order_and_require_the_profile() {
    let b = binding();
    let records = vec![
        ShellNativeLauncherRecord::Opening(NativeLauncherOpening {
            grant: b.grant,
            opening: b.opening,
            output: b.output,
            catalog_generation: b.catalog_generation,
            state_revision: 1,
        }),
        ShellNativeLauncherRecord::Focus(b),
        ShellNativeLauncherRecord::Input(NativeLauncherInput {
            event: ack().event,
            issued_mono_usec: 1000,
            kind: NativeLauncherInputKind::Accept,
            text: String::new(),
        }),
        ShellNativeLauncherRecord::ActivationOutcome(NativeLauncherActivationOutcome {
            activation: activation(),
            status: 1,
            reason: 0,
        }),
        ShellNativeLauncherRecord::FocusRevoked(NativeLauncherFocusRevoked {
            binding: b,
            reason: 1,
        }),
        ShellNativeLauncherRecord::Closed(NativeLauncherClosed {
            grant: b.grant,
            opening: b.opening,
            reason: 1,
        }),
    ];
    let (mut c, mut p) = ready();
    assert!(matches!(
        c.take_native_launcher_observation().unwrap(),
        Some(NativeLauncherObservation::Content(
            _,
            ShellContentRecord::Limits(_)
        ))
    ));
    for r in &records {
        push(&mut p, r.clone());
    }
    let mut seen = Vec::new();
    drive(&mut c, &mut p, |c, _| {
        while let Some(value) = c.take_native_launcher_observation().unwrap() {
            let NativeLauncherObservation::Record(value) = value else {
                panic!("wrong family")
            };
            assert_eq!(value.transaction, tx(30));
            seen.push(value.record);
        }
        seen.len() == records.len()
    })
    .unwrap();
    assert_eq!(seen, records);
    let (c, mut p) = connect(
        Profile {
            role: "bar",
            revision: 6,
            capabilities: 1,
            limits_published: false,
        },
        1,
    );
    let mut c = c.unwrap();
    assert_eq!(
        c.enqueue_native_launcher_tracked(&record(ShellNativeLauncherRecord::InputAck(ack()))),
        Err(ShellClientError::MissingCapability)
    );
    push(&mut p, ShellNativeLauncherRecord::Focus(b));
    assert_eq!(
        drive(&mut c, &mut p, |_, _| false),
        Err(ShellClientError::MissingCapability)
    );
}

#[test]
fn response_is_atomic_cancellable_and_keeps_separate_custody() {
    let (mut c, mut p) = ready();
    drop(
        c.reserve_native_input_response(tx(31), ack(), Some((tx(30), activation())))
            .unwrap(),
    );
    assert_eq!(
        c.custody_result(Ticket(1)),
        Err(CustodyLookupError::Unissued)
    );
    let mut invalid = ack();
    invalid.disposition = 0;
    assert!(
        c.enqueue_native_input_response(tx(31), invalid, Some((tx(30), activation())))
            .is_err()
    );
    let mut wrong = activation();
    wrong.event.event_id += 1;
    assert!(
        c.enqueue_native_input_response(tx(31), ack(), Some((tx(30), wrong)))
            .is_err()
    );
    assert!(
        c.enqueue_native_input_response(tx(30), ack(), Some((tx(30), activation())))
            .is_err()
    );
    assert_eq!(
        c.enqueue_native_launcher_tracked(&record(ShellNativeLauncherRecord::Focus(binding()))),
        Err(ShellClientError::WrongDirection)
    );
    let admitted = c
        .enqueue_native_input_response(tx(31), ack(), Some((tx(30), activation())))
        .unwrap();
    assert_eq!(
        admitted,
        Admission {
            first: Ticket(1),
            count: 2
        }
    );
    let bytes = commit(&mut c, &mut p, Ticket(1));
    assert_eq!(
        decode_shell_file_native_launcher_transaction(&bytes, ShellFileKind::NativeActivate)
            .unwrap(),
        record(ShellNativeLauncherRecord::Activate(activation()))
    );
    drive(&mut c, &mut p, |_, p| !p.submits.is_empty()).unwrap();
    let value = decode_shell_file_native_launcher_transaction(
        p.transactions.last().unwrap(),
        ShellFileKind::NativeInputAck,
    )
    .unwrap();
    assert_eq!(value.transaction, tx(31));
    assert_eq!(value.record, ShellNativeLauncherRecord::InputAck(ack()));
    p.close();
    let _ = c.poll_io();
    assert_eq!(c.custody(Ticket(1)), Some(Custody::Submitted));
    assert_eq!(c.custody(Ticket(2)), Some(Custody::Unknown));
}

#[test]
fn native_catalog_is_fetched_before_later_events() {
    let (mut c, mut p) = ready();
    c.take_native_launcher_observation().unwrap();
    let catalog = ShellApplicationCatalog {
        connection_epoch: EPOCH,
        generation: 6,
        entries: vec![],
    };
    let bytes = encode_shell_file_catalog(
        ShellFileHeader {
            kind: ShellFileKind::Catalog,
            connection_epoch: EPOCH,
            submission_id: 0,
            sequence: 0,
        },
        &ShellFileCatalog {
            transaction: tx(20),
            catalog: ShellPersistentCatalog {
                catalog: catalog.clone(),
                identities: Default::default(),
            },
        },
    )
    .unwrap();
    p.objects.insert(
        "catalog".into(),
        Object {
            qid: 88,
            bytes,
            chunk: 7,
        },
    );
    let event = p.published(ShellFileKind::Catalog, 6, 88);
    p.push(event);
    push(&mut p, ShellNativeLauncherRecord::Focus(binding()));
    let mut seen = Vec::new();
    drive(&mut c, &mut p, |c, _| {
        while let Some(v) = c.take_native_launcher_observation().unwrap() {
            seen.push(v);
        }
        seen.len() == 3
    })
    .unwrap();
    assert!(
        matches!(&seen[0], NativeLauncherObservation::Published(v) if v.object == ShellFileKind::Catalog && v.generation == 6)
    );
    assert!(
        matches!(&seen[1], NativeLauncherObservation::Catalog(t, v) if *t == tx(20) && *v == catalog)
    );
    assert!(
        matches!(&seen[2], NativeLauncherObservation::Record(v) if v.record == ShellNativeLauncherRecord::Focus(binding()))
    );
}

#[test]
fn local_close_preserves_custody_without_io_or_replay() {
    for submit_sent in [false, true] {
        let (mut c, mut p) = ready();
        let record = record(ShellNativeLauncherRecord::InputAck(ack()));
        let first = c.enqueue_native_launcher_tracked(&record).unwrap();
        let second = c.enqueue_native_launcher_tracked(&record).unwrap();
        let reservation = c.reserve_capacity(1, 1024).unwrap();
        if submit_sent {
            drive(&mut c, &mut p, |_, p| !p.submits.is_empty()).unwrap();
        }
        let requests = p.transactions.len();
        c.close();
        c.close();
        assert_eq!(
            c.custody(first.first),
            Some(if submit_sent {
                Custody::Unknown
            } else {
                Custody::DroppedUnsent
            })
        );
        assert_eq!(c.custody(second.first), Some(Custody::DroppedUnsent));
        assert!(c.cancel_reservation(&reservation).is_err());
        assert!(c.enqueue_native_launcher_tracked(&record).is_err());
        assert!(c.poll_io().is_err());
        assert!(c.io_interest().is_err());
        assert!(c.wake_deadline().is_none());
        assert_eq!(p.transactions.len(), requests);
        // Initial decoded Limits remain available despite local close.
        assert!(c.take_native_launcher_observation().unwrap().is_some());
        assert!(matches!(
            c.take_native_launcher_observation(),
            Err(ShellClientError::PeerClosed)
        ));
    }
}

#[test]
fn local_close_keeps_observed_submitted() {
    let (mut c, mut p) = ready();
    let record = record(ShellNativeLauncherRecord::InputAck(ack()));
    let a = c.enqueue_native_launcher_tracked(&record).unwrap();
    commit(&mut c, &mut p, a.first);
    c.close();
    assert_eq!(c.custody(a.first), Some(Custody::Submitted));
}

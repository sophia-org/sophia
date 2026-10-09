#![cfg(target_os = "linux")]
//! t294: the lock provider's worker. Session's commands reach a negotiated
//! provider as events, what the provider submits reaches Session as service
//! events, and a departed provider is reported once by epoch.
#[path = "support/shell_file_peer.rs"]
mod raw_peer;

use std::time::Duration;

use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;

fn limits() -> LockFileLimits {
    LockFileLimits {
        max_outputs: 1,
        upload_slots: 1,
        max_chords: 1,
        max_width_px: 16,
        max_height_px: 16,
        max_resource_bytes: 1024,
        max_live_resources: 2,
        journal_records: 32,
        journal_bytes: 8192,
        assembly_timeout_ms: 2000,
        ack_progress_timeout_ms: 2000,
    }
}

fn locked(epoch: u64) -> LockObject {
    LockObject {
        lock_epoch: epoch,
        topology_generation: 1,
        phase: LockPhase::Locked,
        allocations: vec![LockAllocation {
            output_id: 1,
            output_generation: 1,
            allocation_id: 2,
            allocation_generation: 1,
            pixel_width: 4,
            pixel_height: 4,
            scale_numerator: 1,
            scale_denominator: 1,
        }],
    }
}

const SUPER_B: LockChordRequest = LockChordRequest {
    keysym: 0x62,
    modifiers: 0b1000,
};

fn service(label: &str) -> (LockFileService, std::path::PathBuf) {
    service_from(label, 11)
}

fn service_from(label: &str, first_epoch: u64) -> (LockFileService, std::path::PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("lock-file-service-{label}-{}", std::process::id()));
    let mut transport = LockFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        first_epoch,
        limits(),
    )
    .unwrap();
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let path = transport.socket_path().to_owned();
    (
        LockFileService::spawn(transport, locked(4), Vec::new()).unwrap(),
        path,
    )
}

fn submit(peer: &mut raw_peer::Peer, epoch: u64, id: u64, kind: LockFileKind, body: &[u8]) {
    let candidate = encode_lock_file_record(
        LockFileHeader {
            kind,
            connection_epoch: epoch,
            submission_id: id,
            sequence: 0,
        },
        body,
    )
    .unwrap();
    peer.open(5, b"transaction", 2);
    assert_eq!(peer.write(5, &candidate).0, 119);
    let control = encode_lock_file_submit(LockFileSubmit {
        connection_epoch: epoch,
        submission_id: id,
        candidate_bytes: candidate.len() as u32,
    })
    .unwrap();
    assert_eq!(peer.write(3, &control).0, 119);
    peer.clunk(5);
}

/// Reads events from `offset` until one of `kind` arrives.
fn wait_for(peer: &mut raw_peer::Peer, offset: &mut u64, kind: LockFileKind) -> Vec<u8> {
    for _ in 0..2000 {
        let bytes = peer.read(2, *offset);
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
            *offset += size as u64;
            let record = decode_lock_file_record(&rest[..size], LockFileClass::Event).unwrap();
            if record.header.kind == kind {
                return record.body.to_vec();
            }
            rest = &rest[size..];
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("no {kind:?} event");
}

fn event(service: &LockFileService) -> LockFileServiceEvent {
    service.event_timeout(Duration::from_secs(5)).unwrap()
}

/// A late Session receipt for an image custody revoked during topology
/// publication is stale work, not a reason to stop the provider service.
#[test]
fn a_revoked_candidates_late_outcome_does_not_stop_the_service() {
    let (service, path) = service("late-topology-outcome");
    let mut peer = raw_peer::Peer::connect(&path);
    peer.setup();
    submit(
        &mut peer,
        11,
        1,
        LockFileKind::Negotiate,
        &LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
            requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT,
            chords: Vec::new(),
        }
        .encode()
        .unwrap(),
    );
    assert!(matches!(
        event(&service),
        LockFileServiceEvent::Connected {
            connection_epoch: 11,
            ..
        }
    ));
    let mut offset = 0;
    wait_for(&mut peer, &mut offset, LockFileKind::ObjectPublished);
    let resource = LockResourceId {
        id: 1,
        generation: 1,
    };
    submit(
        &mut peer,
        11,
        2,
        LockFileKind::ResourceBegin,
        &LockResourceBegin {
            transaction: 100,
            resource,
            width_px: 4,
            height_px: 4,
            slot: 0,
        }
        .encode()
        .unwrap(),
    );
    assert_eq!(peer.open_path(8, &[b"upload", b"0"], 1).0, 13);
    assert_eq!(peer.write(8, &[7; 64]).0, 119);
    peer.clunk(8);
    submit(
        &mut peer,
        11,
        3,
        LockFileKind::ResourceEnd,
        &LockResourceStep {
            transaction: 100,
            resource,
            total_bytes: Some(64),
        }
        .encode()
        .unwrap(),
    );
    assert!(matches!(
        event(&service),
        LockFileServiceEvent::Inbound {
            connection_epoch: 11,
            inbound: LockInbound::ResourceReady { .. },
        }
    ));
    submit(
        &mut peer,
        11,
        4,
        LockFileKind::FrameDemand,
        &LockFrameDemand {
            transaction: 101,
            lock_epoch: 4,
            allocation_id: 2,
            allocation_generation: 1,
            demand_id: 1,
        }
        .encode()
        .unwrap(),
    );
    assert!(matches!(
        event(&service),
        LockFileServiceEvent::Inbound {
            connection_epoch: 11,
            inbound: LockInbound::Demand(_),
        }
    ));
    service
        .command(LockFileServiceCommand::Permit {
            allocation_id: 2,
            demand_id: 1,
            expires_after: Duration::from_millis(250),
        })
        .unwrap();
    let permit =
        LockFramePermit::decode(&wait_for(&mut peer, &mut offset, LockFileKind::FramePermit))
            .unwrap();
    let candidate = LockCandidate {
        transaction: 102,
        lock_epoch: 4,
        output_id: 1,
        output_generation: 1,
        allocation_id: 2,
        allocation_generation: 1,
        candidate_generation: 1,
        pacing_permit: permit.pacing_permit,
        resource,
    };
    submit(
        &mut peer,
        11,
        5,
        LockFileKind::Candidate,
        &candidate.encode().unwrap(),
    );
    match event(&service) {
        LockFileServiceEvent::Inbound {
            connection_epoch: 11,
            inbound:
                LockInbound::Candidate {
                    candidate: received,
                    ..
                },
        } => assert_eq!(received, candidate),
        other => panic!("candidate was not admitted: {other:?}"),
    }
    let mut withdrawn = locked(4);
    withdrawn.topology_generation = 2;
    withdrawn.allocations.clear();
    service
        .command(LockFileServiceCommand::PublishLock(withdrawn))
        .unwrap();
    let revoked = LockCandidateOutcome::decode(&wait_for(
        &mut peer,
        &mut offset,
        LockFileKind::CandidateOutcome,
    ))
    .unwrap();
    assert_eq!(revoked.transaction, candidate.transaction);
    assert_eq!(revoked.status, LockCandidateStatus::Revoked);
    wait_for(&mut peer, &mut offset, LockFileKind::ObjectPublished);
    service
        .command(LockFileServiceCommand::Outcome(LockCandidateOutcome {
            status: LockCandidateStatus::Presented,
            reason: 0,
            ..revoked
        }))
        .unwrap();
    // A later valid command is a barrier through the same worker queue:
    // enqueue success alone would not prove it survived the stale outcome.
    let marker = LockEntry {
        lock_epoch: 4,
        entry: LockEntryKind::Insert,
        empty_after: false,
    };
    service
        .command(LockFileServiceCommand::Entry(marker))
        .unwrap();
    let seen = LockEntry::decode(&wait_for(&mut peer, &mut offset, LockFileKind::Entry)).unwrap();
    assert_eq!(seen, marker);
}

#[test]
fn session_and_the_provider_reach_each_other_through_the_worker() {
    let (service, path) = service("round-trip");
    let mut peer = raw_peer::Peer::connect(&path);
    peer.setup();
    let negotiate = LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 1,
        requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT | LOCK_FILE_CAPABILITY_CHORDS,
        chords: vec![SUPER_B],
    }
    .encode()
    .unwrap();
    submit(&mut peer, 11, 1, LockFileKind::Negotiate, &negotiate);
    match event(&service) {
        LockFileServiceEvent::Connected {
            connection_epoch: 11,
            chords,
        } => assert_eq!(chords, [SUPER_B]),
        other => panic!("{other:?}"),
    }
    let mut offset = 0;
    wait_for(&mut peer, &mut offset, LockFileKind::Negotiated);

    // Session publishes a new lock and reports an edit and a chord.
    service
        .command(LockFileServiceCommand::PublishLock(locked(5)))
        .unwrap();
    wait_for(&mut peer, &mut offset, LockFileKind::ObjectPublished);
    peer.open(7, b"lock", 0);
    let lock = peer.read(7, 0);
    let lock = decode_lock_file_record(&lock, LockFileClass::Object).unwrap();
    assert_eq!(LockObject::decode(lock.body).unwrap().lock_epoch, 5);
    service
        .command(LockFileServiceCommand::Entry(LockEntry {
            lock_epoch: 5,
            entry: LockEntryKind::Insert,
            empty_after: false,
        }))
        .unwrap();
    let entry = wait_for(&mut peer, &mut offset, LockFileKind::Entry);
    assert_eq!(
        LockEntry::decode(&entry).unwrap().entry,
        LockEntryKind::Insert
    );
    service
        .command(LockFileServiceCommand::Chord(LockChord {
            lock_epoch: 5,
            chord: 0,
        }))
        .unwrap();
    wait_for(&mut peer, &mut offset, LockFileKind::Chord);

    // The provider asks for a frame; Session answers with a permit.
    let demand = LockFrameDemand {
        transaction: 9,
        lock_epoch: 5,
        allocation_id: 2,
        allocation_generation: 1,
        demand_id: 3,
    };
    submit(
        &mut peer,
        11,
        2,
        LockFileKind::FrameDemand,
        &demand.encode().unwrap(),
    );
    match event(&service) {
        LockFileServiceEvent::Inbound {
            connection_epoch: 11,
            inbound: LockInbound::Demand(received),
        } => assert_eq!(received, demand),
        other => panic!("{other:?}"),
    }
    service
        .command(LockFileServiceCommand::Permit {
            allocation_id: 2,
            demand_id: 3,
            expires_after: Duration::from_millis(100),
        })
        .unwrap();
    let permit = wait_for(&mut peer, &mut offset, LockFileKind::FramePermit);
    assert_eq!(LockFramePermit::decode(&permit).unwrap().demand_id, 3);

    // The provider leaves: Session hears once, by epoch.
    drop(peer);
    match event(&service) {
        LockFileServiceEvent::Disconnected {
            connection_epoch: 11,
        } => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_replacement_is_admitted_under_the_next_epoch_with_the_newest_lock() {
    let (service, path) = service("replacement");
    let mut first = raw_peer::Peer::connect(&path);
    first.setup();
    service
        .command(LockFileServiceCommand::PublishLock(locked(6)))
        .unwrap();
    service
        .command(LockFileServiceCommand::ReplaceSupervisedPid(
            std::process::id(),
        ))
        .unwrap();
    match event(&service) {
        LockFileServiceEvent::Disconnected {
            connection_epoch: 11,
        } => {}
        other => panic!("{other:?}"),
    }
    drop(first);
    let mut second = raw_peer::Peer::connect(&path);
    second.setup();
    second.open(6, b"api", 0);
    assert_eq!(second.read(6, 0), b"sophia-lock-files version=1 epoch=12\n");
    second.open(7, b"lock", 0);
    let lock = second.read(7, 0);
    let lock = decode_lock_file_record(&lock, LockFileClass::Object).unwrap();
    assert_eq!(LockObject::decode(lock.body).unwrap().lock_epoch, 6);
}

/// t294: a provider retired while it may still run, as when Session follows
/// a new render device. Its connection ends, `Retired` follows its last event
/// and names the next epoch, and the successor is admitted under that epoch,
/// so no identity the retired provider used can recur.
#[test]
fn a_retired_provider_is_followed_by_a_successor_under_the_next_epoch() {
    let (service, path) = service("retired-successor");
    let mut first = raw_peer::Peer::connect(&path);
    first.setup();
    service
        .command(LockFileServiceCommand::RetireSupervisedProcess)
        .unwrap();
    match event(&service) {
        LockFileServiceEvent::Disconnected {
            connection_epoch: 11,
        } => {}
        other => panic!("{other:?}"),
    }
    match event(&service) {
        LockFileServiceEvent::Retired { next_epoch: 12 } => {}
        other => panic!("{other:?}"),
    }
    drop(first);
    service
        .command(LockFileServiceCommand::ReplaceSupervisedPid(
            std::process::id(),
        ))
        .unwrap();
    let mut second = raw_peer::Peer::connect(&path);
    second.setup();
    second.open(6, b"api", 0);
    assert_eq!(second.read(6, 0), b"sophia-lock-files version=1 epoch=12\n");
}

/// t294: epochs spent by reconnects before a retirement stay spent, and a
/// retirement with nobody connected still marks the end of the retired
/// process.
#[test]
fn a_retirement_follows_earlier_reconnects_and_needs_no_connection() {
    let (service, path) = service("retired-after-reconnect");
    let mut first = raw_peer::Peer::connect(&path);
    first.setup();
    service
        .command(LockFileServiceCommand::ReplaceSupervisedPid(
            std::process::id(),
        ))
        .unwrap();
    assert!(matches!(
        event(&service),
        LockFileServiceEvent::Disconnected {
            connection_epoch: 11
        }
    ));
    drop(first);
    let mut second = raw_peer::Peer::connect(&path);
    second.setup();
    service
        .command(LockFileServiceCommand::RetireSupervisedProcess)
        .unwrap();
    match event(&service) {
        LockFileServiceEvent::Disconnected {
            connection_epoch: 12,
        } => {}
        other => panic!("{other:?}"),
    }
    match event(&service) {
        LockFileServiceEvent::Retired { next_epoch: 13 } => {}
        other => panic!("{other:?}"),
    }
    drop(second);

    let (service, _path) = service_from("retired-unconnected", 11);
    service
        .command(LockFileServiceCommand::RetireSupervisedProcess)
        .unwrap();
    match event(&service) {
        LockFileServiceEvent::Retired { next_epoch: 11 } => {}
        other => panic!("{other:?}"),
    }
}

/// t294: the connection counter is checked. A retirement reports the last
/// epoch the counter can give, and a successor past it is refused rather
/// than given a wrapped epoch.
#[test]
fn an_exhausted_connection_counter_refuses_the_successor() {
    let (service, path) = service_from("retired-exhausted", u64::MAX - 1);
    let mut first = raw_peer::Peer::connect(&path);
    first.setup();
    first.open(6, b"api", 0);
    assert_eq!(
        first.read(6, 0),
        format!("sophia-lock-files version=1 epoch={}\n", u64::MAX - 1).as_bytes()
    );
    service
        .command(LockFileServiceCommand::RetireSupervisedProcess)
        .unwrap();
    assert!(matches!(
        event(&service),
        LockFileServiceEvent::Disconnected { .. }
    ));
    match event(&service) {
        LockFileServiceEvent::Retired {
            next_epoch: u64::MAX,
        } => {}
        other => panic!("{other:?}"),
    }
    drop(first);
    service
        .command(LockFileServiceCommand::ReplaceSupervisedPid(
            std::process::id(),
        ))
        .unwrap();
    match event(&service) {
        LockFileServiceEvent::ConnectionRejected { .. } => {}
        other => panic!("{other:?}"),
    }
}

/// t294: until a replacement is authorized, the retired process is not
/// answered, however long it takes to exit.
#[test]
fn a_retired_provider_cannot_connect_again() {
    let (service, path) = service("retired-refused");
    let mut first = raw_peer::Peer::connect(&path);
    first.setup();
    service
        .command(LockFileServiceCommand::RetireSupervisedProcess)
        .unwrap();
    assert!(matches!(
        event(&service),
        LockFileServiceEvent::Disconnected { .. }
    ));
    assert!(matches!(
        event(&service),
        LockFileServiceEvent::Retired { .. }
    ));
    let mut late = raw_peer::Peer::connect(&path);
    let version = [
        65536u32.to_le_bytes().as_slice(),
        &8u16.to_le_bytes(),
        b"9P2000.L",
    ]
    .concat();
    assert!(
        late.rpc(100, &version).is_err(),
        "the retired process was answered"
    );
}

#[test]
fn commands_for_a_provider_that_has_not_negotiated_cost_nothing() {
    let (service, path) = service("silent");
    let mut peer = raw_peer::Peer::connect(&path);
    peer.setup();
    for _ in 0..3 {
        service
            .command(LockFileServiceCommand::Entry(LockEntry {
                lock_epoch: 4,
                entry: LockEntryKind::Insert,
                empty_after: false,
            }))
            .unwrap();
    }
    service
        .command(LockFileServiceCommand::PublishLock(locked(5)))
        .unwrap();
    // Nothing failed: the only news is the silent provider losing the role
    // at its negotiation deadline.
    match event(&service) {
        LockFileServiceEvent::Disconnected {
            connection_epoch: 11,
        } => {}
        other => panic!("{other:?}"),
    }
    drop(peer);
}

#[test]
fn an_idle_service_waits_until_a_command_and_drop_interrupts_the_wait() {
    let (service, _path) = service("idle-readiness");
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while service.stats().waits == 0 {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let before = service.stats();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(service.stats().passes, before.passes, "idle worker polled");
    // No socket is readable. A command must wake the same blocked worker.
    service
        .command(LockFileServiceCommand::PublishLock(locked(5)))
        .unwrap();
    while service.stats().passes == before.passes {
        assert!(std::time::Instant::now() < deadline, "command wake lost");
        std::thread::yield_now();
    }
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(service);
        done.send(()).unwrap();
    });
    finished
        .recv_timeout(Duration::from_secs(2))
        .expect("stop wake lost");
}

#[test]
fn a_full_handoff_ignores_readable_peer_until_the_owner_drains() {
    let (service, path) = service("full-readiness");
    let (sent, readable) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut peer = raw_peer::Peer::connect(&path);
        peer.setup();
        let negotiate = LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
            requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT,
            chords: vec![],
        };
        submit(
            &mut peer,
            11,
            1,
            LockFileKind::Negotiate,
            &negotiate.encode().unwrap(),
        );
        let mut offset = 0;
        for id in 0..48_u64 {
            // Acknowledge every fetched journal page: it must be the owner
            // handoff, not the provider's journal, that applies backpressure.
            let bytes = peer.read(2, offset);
            offset += bytes.len() as u64;
            let mut rest = bytes.as_slice();
            let mut sequence = 0;
            while !rest.is_empty() {
                let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
                sequence = decode_lock_file_record(&rest[..size], LockFileClass::Event)
                    .unwrap()
                    .header
                    .sequence;
                rest = &rest[size..];
            }
            assert_eq!(
                peer.write(
                    4,
                    &encode_lock_file_ack(LockFileAck {
                        connection_epoch: 11,
                        sequence,
                    })
                    .unwrap()
                )
                .0,
                119
            );
            let demand = LockFrameDemand {
                transaction: id + 10,
                lock_epoch: 4,
                allocation_id: 2,
                allocation_generation: 1,
                demand_id: id + 1,
            };
            submit(
                &mut peer,
                11,
                id + 2,
                LockFileKind::FrameDemand,
                &demand.encode().unwrap(),
            );
            if id == 31 {
                // The next event read is already on the socket when we
                // announce it. The owner intentionally leaves its queue full.
                let body = [
                    2u32.to_le_bytes().as_slice(),
                    &offset.to_le_bytes(),
                    &65500u32.to_le_bytes(),
                ]
                .concat();
                let (_, bytes) = peer
                    .rpc_with_prefix(116, &body, 23, || {
                        sent.send(()).unwrap();
                    })
                    .unwrap();
                // Leave the journal offset alone: an identical read is legal.
                assert!(!bytes.is_empty());
            }
        }
        peer
    });
    assert!(matches!(
        event(&service),
        LockFileServiceEvent::Connected { .. }
    ));
    readable.recv_timeout(Duration::from_secs(5)).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while service.stats().handoff_waits == 0 {
        assert!(std::time::Instant::now() < deadline, "handoff never filled");
        std::thread::yield_now();
    }
    let before = service.stats().passes;
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        service.stats().passes,
        before,
        "readable peer spun on full handoff"
    );
    for expected in 1..=48 {
        match event(&service) {
            LockFileServiceEvent::Inbound {
                inbound: LockInbound::Demand(d),
                ..
            } => assert_eq!(d.demand_id, expected),
            other => panic!("unexpected event: {other:?}"),
        }
    }
    drop(peer.join().unwrap());
}

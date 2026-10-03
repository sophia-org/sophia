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
    let directory =
        std::env::temp_dir().join(format!("lock-file-service-{label}-{}", std::process::id()));
    let mut transport = LockFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        11,
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

#![cfg(test)]
//! Transition-only controls: a worker that ends fails its live epoch
//! closed and is never polled or reserved again; later offers are refused
//! with reason 9. No real worker panic is proven here. The thread cannot be
//! ended safely from a test, so these inject the failure into the owner's
//! one reaction path, which every detection site (a failed completion take,
//! a closed submit) calls. Nothing here, or in service drop, waits on a
//! dead worker's jobs.
use super::*;
use crate::lock_files::{LockFileSettings, LockImageReadiness, LockImageSettings};
use sophia_9p::ReadOutcome;
use sophia_protocol::lock_files::*;

const GENERATION: u64 = 5;

fn limits() -> LockFileLimits {
    LockFileLimits {
        max_outputs: 4,
        upload_slots: 2,
        max_chords: 2,
        max_width_px: 64,
        max_height_px: 64,
        max_resource_bytes: 64 * 64 * 4,
        max_live_resources: 3,
        journal_records: 16,
        journal_bytes: 4096,
        assembly_timeout_ms: 1000,
        ack_progress_timeout_ms: 2000,
    }
}

fn settings(epoch: u64) -> LockImageSettings {
    LockImageSettings {
        discovery: LockImageDiscovery {
            connection_epoch: epoch,
            channel_generation: GENERATION,
            endpoint: "image.sock".into(),
            source_kinds: 1,
            max_offers: 2,
            max_packets_per_turn: 8,
            hello_timeout_ms: 1000,
            validation_timeout_ms: 100,
            validation_chunk_bytes: 65536,
            capture_interest_timeout_ms: 200,
            formats_generation: 3,
            limits_generation: 3,
        },
        limits: LockImageLimits {
            channel_generation: GENERATION,
            limits_generation: 3,
            max_resource_bytes: 64 * 64 * 4,
            max_sealed_backing_bytes: 2 * 64 * 64 * 4,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: 2,
            max_validation_jobs: 2,
            max_capture_jobs: 0,
            max_admitted_outputs: 1,
            max_format_rows_per_output: 1,
            max_width: 64,
            max_height: 64,
            validation_timeout_ms: 100,
            capture_interest_timeout_ms: 200,
            validation_chunk_bytes: 65536,
            max_packets_per_turn: 8,
        },
        formats: LockImageFormats {
            channel_generation: GENERATION,
            formats_generation: 3,
            limits_generation: 3,
            rows: Vec::new(),
        },
    }
}

fn submit(custody: &mut LockFileCustody, epoch: u64, id: u64, kind: LockFileKind, body: &[u8]) {
    let revision = if kind == LockFileKind::Negotiate {
        1
    } else {
        2
    };
    let header = LockFileHeader {
        kind,
        connection_epoch: epoch,
        submission_id: id,
        sequence: 0,
    };
    let bytes = encode_lock_file_record_at_revision(header, body, revision).unwrap();
    custody.submit(&bytes, Instant::now()).unwrap();
}

/// A negotiated revision 2 epoch with a ready channel and one pending
/// offer, ID 1.
fn offered(epoch: u64) -> LockFileCustody {
    let mut custody = LockFileCustody::new(LockFileSettings {
        epoch,
        limits: limits(),
        reserved_chords: Vec::new(),
        lock: LockObject {
            lock_epoch: 4,
            topology_generation: 3,
            phase: LockPhase::Locked,
            allocations: vec![LockAllocation {
                output_id: 1,
                output_generation: 1,
                allocation_id: 11,
                allocation_generation: 1,
                pixel_width: 4,
                pixel_height: 2,
                scale_numerator: 1,
                scale_denominator: 1,
            }],
        },
        lock_qid: 90,
    })
    .unwrap();
    custody.configure_image(settings(epoch)).unwrap();
    let negotiate = LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 2,
        requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT | LOCK_FILE_CAPABILITY_SEALED_MEMORY,
        chords: Vec::new(),
    };
    submit(
        &mut custody,
        epoch,
        1,
        LockFileKind::Negotiate,
        &negotiate.encode().unwrap(),
    );
    custody.image_channel_ready(GENERATION, 1).unwrap();
    assert!(matches!(
        custody.image_readiness(),
        LockImageReadiness::Ready { .. }
    ));
    let metadata = LockImageMetadata {
        offer_id: 1,
        resource: 21,
        resource_generation: 1,
        admission_epoch: 4,
        output_id: 1,
        allocation_id: 11,
        topology_generation: 3,
        source_kind: LOCK_IMAGE_SOURCE_SEALED_MEMORY,
        source_format: LOCK_IMAGE_SEALED_FORMAT,
        width: 4,
        height: 2,
        stride: 16,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 32,
        modifier: 0,
    };
    submit(
        &mut custody,
        epoch,
        2,
        LockFileKind::ImageBegin,
        &metadata.encode().unwrap(),
    );
    custody
}

/// The journal's statuses as (transaction, state, reason).
fn statuses(custody: &mut LockFileCustody) -> Vec<(u64, LockResourceState, u16)> {
    let ReadOutcome::Ready(bytes) = custody.read(0, 1 << 16).unwrap() else {
        panic!("journal read not ready");
    };
    let mut statuses = Vec::new();
    let mut rest = bytes.as_slice();
    while !rest.is_empty() {
        let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        let record =
            decode_lock_file_record_at_revision(&rest[..size], LockFileClass::Event, 2).unwrap();
        if record.header.kind == LockFileKind::ResourceStatus {
            let status = LockResourceStatus::decode(record.body).unwrap();
            statuses.push((status.transaction, status.status, status.reason));
        }
        rest = &rest[size..];
    }
    statuses
}

fn worker() -> ImageWorker {
    ImageWorker::new(2, 64 * 64 * 4, 65536).unwrap()
}

#[test]
fn a_live_worker_admits_and_its_readiness_is_polled_while_jobs_are_owed() {
    let mut worker = worker();
    let mut custody = offered(7);
    assert!(worker.readiness().is_none());
    worker.serve(&mut custody, Instant::now());
    assert_eq!(worker.pending(), 1);
    assert!(worker.readiness().is_some());
    assert!(statuses(&mut custody).is_empty());
}

#[test]
fn a_dead_worker_fails_the_live_epoch_closed_and_refuses_later_offers() {
    let mut worker = worker();
    let mut admitted = offered(7);
    worker.serve(&mut admitted, Instant::now());
    assert_eq!(worker.pending(), 1);
    // Detected during the epoch: it ends, and its owed job never returns.
    worker.fail(Some(&mut admitted));
    assert!(admitted.is_revoked());
    assert!(worker.readiness().is_none(), "a dead worker is not polled");
    // The epoch's end still returns the unsent reservation.
    worker.end_epoch();
    assert_eq!(worker.pending(), 0);
    worker.drain_ended();
    // The next epoch's offer is refused with reason 9, never reserved.
    let mut next = offered(8);
    let now = Instant::now();
    assert!(worker.serve(&mut next, now));
    assert_eq!(
        statuses(&mut next),
        [(
            1,
            LockResourceState::Rejected,
            LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE
        )]
    );
    assert_eq!(worker.pending(), 0);
    assert!(worker.readiness().is_none());
    // Nothing further to do: another turn journals nothing.
    assert!(!worker.serve(&mut next, now));
}

#[test]
fn a_dead_worker_found_between_connections_needs_no_epoch() {
    let mut worker = worker();
    worker.fail(None);
    worker.drain_ended();
    worker.end_epoch();
    assert_eq!(worker.pending(), 0);
    assert!(worker.readiness().is_none());
}

use super::*;
use crate::{
    ContentAllocationSnapshot, OwnedCopyImageRequest, OwnedRingGeometry, OwnedSlotIdentity,
};
use sophia_protocol::*;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

fn identity() -> Identity {
    Identity {
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        },
        channel: 1,
        ring: 1,
        generation: 1,
    }
}

fn request() -> OwnedCopyImageRequest {
    OwnedCopyImageRequest {
        slot: OwnedSlotIdentity {
            offer: 1,
            slot: 0,
            generation: 2,
        },
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        topology_generation: 1,
        width: 1,
        height: 1,
    }
}

fn registry() -> ContentEpochRegistry {
    let grant = identity().grant;
    let request = request();
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    let mut limits = ContentLimits::prototype(grant);
    limits.max_staging_bytes = 8 << 20;
    limits.max_resident_bytes = 8 << 20;
    limits.max_retiring_bytes = 8 << 20;
    registry.admit(limits).unwrap();
    let allocations = registry.allocations_mut(grant).unwrap();
    allocations
        .publish_outputs(
            TransactionId::from_raw(1),
            1,
            vec![ContentOutputFactsEntry {
                output: request.output,
                local_width: 1,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    allocations
        .request(
            TransactionId::from_raw(2),
            ContentAllocationRequest {
                grant,
                output: request.output,
                allocation_request_id: 1,
                operation: 1,
                role: 1,
                edge: 1,
                prior: ContentAllocationId::default(),
                parent: ContentAllocationId::default(),
                parent_presentation_epoch: 0,
                anchor_parent_rect: ContentPixelRect::default(),
                desired_width: 1,
                desired_height: 1,
                margins: ContentMargins::default(),
            },
            &[],
            0,
        )
        .unwrap();
    allocations
        .grant(
            1,
            ContentAllocationSnapshot {
                native_opening: None,
                output: request.output,
                allocation: request.allocation,
                scale_generation: 1,
                scale_numerator: 1,
                scale_denominator: 1,
                role: 1,
                edge: 1,
                margins: ContentMargins::default(),
                logical: ContentLogicalRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                pixel: ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                parent: ContentAllocationId::default(),
                anchor_parent_rect: ContentPixelRect::default(),
                allowed_reservation_extent: 1,
            },
            &[],
        )
        .unwrap();
    while allocations.take_event().is_some() {}
    registry
}

fn ring(registry: &mut ContentEpochRegistry) -> OwnedRing {
    let geometry = OwnedRingGeometry::new(1, 4).unwrap();
    let source = File::from(
        rustix::fs::memfd_create("owned-owner-test", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    source.set_len(geometry.source_bytes() as u64).unwrap();
    source.write_all_at(&2_u64.to_le_bytes(), 0).unwrap();
    let reservation = registry
        .reserve_owned_ring_source(identity().grant, geometry)
        .unwrap();
    OwnedRing::admit(source, reservation).unwrap()
}

fn transport() -> ShellComponentTransport {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "owned-owner-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    ShellComponentTransport::bind_for_supervised_uid(directory, rustix::process::geteuid().as_raw())
        .unwrap()
}

fn admitted(
    registry: &mut ContentEpochRegistry,
    worker: &mut IsbValidationWorker,
) -> (OwnedCopyOwner, u64) {
    let mut owner = OwnedCopyOwner {
        active: Some(ActiveRing {
            identity: identity(),
            ring: ring(registry),
        }),
        ..OwnedCopyOwner::default()
    };
    let job = owner
        .jobs
        .reserve(
            registry,
            worker,
            &owner.active.as_ref().unwrap().ring,
            request(),
            0,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    owner.work[0] = Some(Work::Working {
        identity: identity(),
        job,
        withdrawn: false,
        handed_off: true,
    });
    (owner, job)
}

fn complete(owner: &mut OwnedCopyOwner, worker: &mut IsbValidationWorker) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(completion) = worker.take_completed().unwrap() {
            assert!(completion.result.is_ok());
            assert!(owner.receive(completion).is_ok());
            return;
        }
        assert!(Instant::now() < deadline, "bounded worker completion");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn disconnected_owner_drops_ring_but_read_ticket_keeps_source_charge() {
    let mut registry = registry();
    let mut worker = IsbValidationWorker::new(1, 4).unwrap();
    let (mut owner, _) = admitted(&mut registry, &mut worker);
    let ticket = owner
        .active
        .as_mut()
        .unwrap()
        .ring
        .offer(request().slot)
        .unwrap();
    let charge = registry.accounting().source_bytes;
    assert!(charge > 0);
    owner
        .service(
            &mut transport(),
            &mut registry,
            &mut worker,
            1,
            Instant::now(),
            0,
        )
        .unwrap();
    assert!(owner.active.is_none());
    assert_eq!(owner.pending(), 0);
    assert_eq!(registry.accounting().source_bytes, charge);
    assert_eq!(
        registry
            .resources(identity().grant)
            .unwrap()
            .usage()
            .backing,
        0
    );
    drop(ticket);
    assert_eq!(registry.accounting().source_bytes, 0);
}

#[test]
fn successful_completion_cannot_authorize_acceptance_after_channel_loss() {
    let mut registry = registry();
    let mut worker = IsbValidationWorker::new(1, 4).unwrap();
    let (mut owner, job) = admitted(&mut registry, &mut worker);
    let ticket = owner
        .active
        .as_mut()
        .unwrap()
        .ring
        .offer(request().slot)
        .unwrap();
    assert!(owner.jobs.submit(&mut worker, job, ticket).is_ok());
    complete(&mut owner, &mut worker);
    assert_eq!(owner.pending(), 1);
    owner
        .service(
            &mut transport(),
            &mut registry,
            &mut worker,
            1,
            Instant::now(),
            0,
        )
        .unwrap();
    assert_eq!(owner.pending(), 0);
    assert_eq!(registry.accounting().source_bytes, 0);
    let resources = registry.resources_mut(identity().grant).unwrap();
    assert!(
        resources
            .lease(identity().grant, request().resource)
            .is_err()
    );
    assert!(matches!(resources.take_event().unwrap().record,
        ShellContentRecord::ResourceStatus(status) if status.reason == ContentReason::Cancelled as u16));
}

#[test]
fn old_settlement_cannot_release_a_successor_rings_matching_slot() {
    let mut registry = registry();
    let mut worker = IsbValidationWorker::new(1, 4).unwrap();
    let (mut owner, job) = admitted(&mut registry, &mut worker);
    let ticket = owner
        .active
        .as_mut()
        .unwrap()
        .ring
        .offer(request().slot)
        .unwrap();
    assert!(owner.jobs.submit(&mut worker, job, ticket).is_ok());
    complete(&mut owner, &mut worker);
    owner.reconcile(&mut registry, 1);
    let successor = Identity {
        generation: 2,
        ..identity()
    };
    let mut next_ring = ring(&mut registry);
    drop(next_ring.offer(request().slot).unwrap());
    owner.active = Some(ActiveRing {
        identity: successor,
        ring: next_ring,
    });
    owner
        .publish_releases(&mut transport(), Some(successor))
        .unwrap();
    assert_eq!(owner.pending(), 0);
    assert_eq!(
        owner.active.as_ref().unwrap().ring.pending_release(0),
        Some(request().slot)
    );
}

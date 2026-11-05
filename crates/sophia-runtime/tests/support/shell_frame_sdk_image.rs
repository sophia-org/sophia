//! Real SDK source registration and copied-image fallback; no device renderer.
use super::*;
use sophia_shell_client::{
    ImageAdapterError, OwnedRingSlotFill, ProducerRoleBounds, RingOfferState, RingUpload,
    ShellImageConfig,
};

pub(super) fn supply_allocation(t: &mut Transport, f: &fixture::Fixture) {
    let a = f.allocations[0].clone();
    let store = t
        .transport
        .content_epochs
        .allocations_mut(fixture::grant())
        .unwrap();
    store
        .publish_outputs(
            fixture::tx(1),
            6,
            vec![ContentOutputFactsEntry {
                output: a.output,
                local_width: 64,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: a.scale_generation,
            }],
        )
        .unwrap();
    store
        .request(
            fixture::tx(2),
            ContentAllocationRequest {
                grant: fixture::grant(),
                output: a.output,
                allocation_request_id: 1,
                operation: 1,
                role: a.role,
                edge: a.edge,
                prior: ContentAllocationId::default(),
                parent: a.parent,
                parent_presentation_epoch: 0,
                anchor_parent_rect: a.anchor_parent_rect,
                desired_width: 64,
                desired_height: 32,
                margins: a.margins,
            },
            &[],
            0,
        )
        .unwrap();
    store.grant(1, a, &[]).unwrap();
    while store.take_event().is_some() {}
}

pub(super) fn enable(
    client: &mut ShellConnection,
    directory: std::path::PathBuf,
    key: ShellFrameStreamKey,
) {
    client
        .enable_image(
            ShellImageConfig {
                endpoint_dir: directory,
                bounds: ProducerRoleBounds {
                    max_resource_bytes: 1 << 20,
                    max_width: 1920,
                    max_height: 1024,
                    max_admitted_outputs: 1,
                    transfer_slots: 4,
                },
                transfer_timeout_ms: 2000,
                startup_deadline_ms: 5000,
                formats_capacity: 4096,
                ring_slots: 2,
                ring_capacity: 8,
                content_grant_epoch: key.grant.content_grant_epoch,
            },
            Instant::now(),
            0,
        )
        .unwrap();
    let mut fill = acquire(client);
    client.image_abort_ring_fill(&mut fill).unwrap();
}

fn acquire(client: &mut ShellConnection) -> OwnedRingSlotFill {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        client.poll_io().unwrap();
        assert!(Instant::now() < until, "ring registration timed out");
        if client.image_adapter().unwrap().ring_identity().is_none() {
            std::thread::yield_now();
            continue;
        }
        match client.image_acquire_ring_fill(8, Instant::now()) {
            Ok(fill) => return fill,
            Err(ImageAdapterError::Again | ImageAdapterError::Busy) => {}
            Err(error) => panic!("acquire copied source: {error:?}"),
        }
        assert!(Instant::now() < until, "ring registration timed out");
        std::thread::yield_now();
    }
}

fn upload(key: ShellFrameStreamKey, resource: u64) -> RingUpload {
    RingUpload {
        resource,
        resource_generation: 1,
        allocation: key.allocation.id,
        allocation_generation: key.allocation.generation,
        output: key.output.id,
        output_generation: key.output.generation,
        topology_generation: key.facts_generation,
        width: 2,
        height: 1,
    }
}

pub(super) fn submit(
    client: &mut ShellConnection,
    stream: &mut FrameStream,
    record: &mut ShellFrameRecord,
    key: ShellFrameStreamKey,
) -> u64 {
    let mut fill = acquire(client);
    let mut fill = std::thread::spawn(move || {
        fill.pixels_mut().unwrap().fill(41);
        fill
    })
    .join()
    .unwrap();
    let (_, offer) = stream
        .submit_ring_fill(client, record, &mut fill, upload(key, 10), Instant::now())
        .unwrap();
    assert!(fill.pixels_mut().is_err());
    offer
}

pub(super) fn settle(client: &mut ShellConnection, offer: u64) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        client.poll_io().unwrap();
        if client
            .image_ring_offer(offer)
            .is_some_and(|i| i.state == RingOfferState::Settled)
        {
            return;
        }
        while client.take_content().unwrap().is_some() {}
        assert!(
            Instant::now() < until,
            "exact source release timed out: {:?}",
            client.image_ring_offer(offer)
        );
        std::thread::yield_now();
    }
}

pub(super) fn fallback(
    client: &mut ShellConnection,
    stream: &FrameStream,
    record: &mut ShellFrameRecord,
    key: ShellFrameStreamKey,
) {
    let mut fill = acquire(client);
    fill.pixels_mut().unwrap().fill(73);
    let offer = stream
        .commit_legacy_ring_fill(client, &mut fill, upload(key, 11), Instant::now())
        .unwrap();
    settle(client, offer);
    let ShellFrameRecord::Commit(frame) = record else {
        unreachable!()
    };
    frame.candidate.placements[0].resource = fixture::resource(11);
}

pub(super) fn assert_pixels(epochs: &crate::ContentEpochRegistry, generation: u64) {
    let resource = fixture::resource(if generation == 1 { 10 } else { 11 });
    let lease = epochs
        .resources(fixture::grant())
        .unwrap()
        .lease(fixture::grant(), resource)
        .unwrap();
    assert_eq!(
        lease.cpu_bytes().unwrap(),
        &[if generation == 1 { 41 } else { 73 }; 8]
    );
}

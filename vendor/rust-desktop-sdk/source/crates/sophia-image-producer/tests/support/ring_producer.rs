//! RingProducer scenarios, mounted as a child of the ring module
//! so private custody state (counters, slot generations) is
//! reachable for exhaustion controls.
use super::*;

const IDENTITY: RingIdentity = RingIdentity {
    connection_epoch: 7,
    content_grant_epoch: 3,
    channel_generation: 9,
    ring: 1,
    generation: 1,
};

fn event(kind: RingEventKind) -> RingEvent {
    RingEvent {
        kind,
        identity: IDENTITY,
        sequence: 5,
        offer_id: 0,
        slot: 0,
        slot_generation: 0,
        resource: 0,
        resource_generation: 0,
        reason: 0,
        changed_generation: false,
    }
}

fn upload() -> RingUpload {
    RingUpload {
        resource: 11,
        resource_generation: 1,
        allocation: 21,
        allocation_generation: 1,
        output: 31,
        output_generation: 1,
        topology_generation: 41,
        width: 2,
        height: 1,
    }
}

fn ready() -> RingProducer {
    ready_slots(0)
}

fn ready_slots(slots: u8) -> RingProducer {
    let mut producer = RingProducer::new(
        RingProducerConfig {
            identity: IDENTITY,
            slots,
            capacity: 0,
            max_resource_bytes: 4096,
            transfer_timeout_ms: 2000,
        },
        0,
    )
    .unwrap();
    assert_eq!(producer.registration().unwrap().len(), 72);
    assert!(producer.registration().is_none());
    producer
        .ring_event(event(RingEventKind::RingAdmitted), 1)
        .unwrap();
    assert!(producer.source_packet().is_some());
    producer.source_sent().unwrap();
    producer
        .ring_event(event(RingEventKind::RingReady), 2)
        .unwrap();
    producer
}

fn released(offer_id: u64, generation: u64) -> RingEvent {
    RingEvent {
        offer_id,
        slot_generation: generation,
        resource: 11,
        resource_generation: 1,
        ..event(RingEventKind::SlotReleased)
    }
}

fn released_with_reason(offer_id: u64, generation: u64, reason: u16) -> RingEvent {
    RingEvent {
        reason,
        ..released(offer_id, generation)
    }
}

#[test]
fn settled_only_after_status_and_exact_release() {
    let mut producer = ready();
    let pixels = [9u8; 8];
    let (offer, bytes) = producer.upload(upload(), &pixels, 3).unwrap();
    assert_eq!(bytes.len(), 152);
    // Status alone does not settle.
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 4)
        .unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    producer.ring_event(released(offer, 2), 5).unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Settled
    );
    // The freed slot is reusable with the next even generation.
    let (offer2, _) = producer.upload(upload(), &pixels, 6).unwrap();
    assert_eq!(offer2, 2);
    // Status alone does not settle either; cancel refuses adoption.
    producer
        .resource_status(11, 1, ResourceStatusKind::Cancelled, 7)
        .unwrap();
    assert_eq!(
        producer.offer(offer2).unwrap().state,
        RingOfferState::Pending
    );
    producer
        .ring_event(released_with_reason(offer2, 4, 11), 8)
        .unwrap();
    assert_eq!(
        producer.offer(offer2).unwrap().state,
        RingOfferState::Refused
    );
}

#[test]
fn failed_control_reply_status_does_not_spend_settlement() {
    let mut producer = ready();
    let (offer, _) = producer.upload(upload(), &[9u8; 8], 3).unwrap();
    // A failed ResourceRetire reply shares the ResourceStatus kind
    // but is not store settlement: it neither settles nor frees the
    // slot, and the genuine final status supersedes it.
    producer
        .resource_status(11, 1, ResourceStatusKind::Rejected, 4)
        .unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    assert_eq!(
        producer.upload(upload(), &[9u8; 8], 5),
        Err(ProducerError::Busy),
        "the slot stays in custody until the exact release"
    );
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 6)
        .unwrap();
    producer.ring_event(released(offer, 2), 7).unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Settled
    );
    assert!(!producer.closed());
}

#[test]
fn failed_reply_after_acceptance_cannot_poison_success_release() {
    let mut producer = ready();
    let (offer, _) = producer.upload(upload(), &[9u8; 8], 3).unwrap();
    // The genuine final status arrives first; a failed control reply
    // (a second retire, a stale cancel) follows it. Neither first nor
    // last status governs: the successful release selects the
    // compatible Accepted evidence.
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 4)
        .unwrap();
    producer
        .resource_status(11, 1, ResourceStatusKind::Rejected, 5)
        .unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Pending
    );
    producer.ring_event(released(offer, 2), 6).unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Settled
    );
    // Terminal info reports the selected evidence, not the last
    // observed (failed-reply) status.
    assert_eq!(
        producer.offer(offer).unwrap().status,
        Some(ResourceStatusKind::Accepted)
    );
    assert!(!producer.closed());
    // A status after settlement is ignored, not a fault.
    producer
        .resource_status(11, 1, ResourceStatusKind::Rejected, 7)
        .unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Settled
    );
}

#[test]
fn cancelled_release_with_changed_generation_still_refuses() {
    let mut producer = ready();
    let (offer, _) = producer.upload(upload(), &[9u8; 8], 3).unwrap();
    // The runtime sets changed_generation from its read check
    // independently of the stopped reason: a cancelled offer may
    // carry it. It requires Cancelled evidence, is never adoptable,
    // and its terminal info reports the chosen Cancelled status.
    producer
        .resource_status(11, 1, ResourceStatusKind::Cancelled, 4)
        .unwrap();
    let changed = RingEvent {
        changed_generation: true,
        ..released_with_reason(offer, 2, 11)
    };
    producer.ring_event(changed, 5).unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Refused
    );
    assert_eq!(
        producer.offer(offer).unwrap().status,
        Some(ResourceStatusKind::Cancelled)
    );
    // The slot freed normally for the next offer.
    let (offer2, _) = producer.upload(upload(), &[9u8; 8], 6).unwrap();
    assert_eq!(offer2, 2);
}

#[test]
fn release_without_compatible_evidence_fails_the_ring() {
    // A refusal release with only Accepted observed has no compatible
    // evidence to select.
    let mut producer = ready();
    let (offer, _) = producer.upload(upload(), &[9u8; 8], 3).unwrap();
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 4)
        .unwrap();
    assert_eq!(
        producer.ring_event(released_with_reason(offer, 2, 1), 5),
        Err(ProducerError::Closed)
    );
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Unknown
    );
    // A success release with only Rejected observed likewise faults
    // instead of guessing an outcome.
    let mut producer = ready();
    let (offer, _) = producer.upload(upload(), &[9u8; 8], 3).unwrap();
    producer
        .resource_status(11, 1, ResourceStatusKind::Rejected, 4)
        .unwrap();
    assert_eq!(
        producer.ring_event(released(offer, 2), 5),
        Err(ProducerError::Closed)
    );
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Unknown
    );
    // The runtime pins kind to reason: a cancellation release
    // (reason 11) with only Rejected observed has no compatible
    // evidence, and a refusal release with only Cancelled neither.
    let mut producer = ready();
    let (offer, _) = producer.upload(upload(), &[9u8; 8], 3).unwrap();
    producer
        .resource_status(11, 1, ResourceStatusKind::Rejected, 4)
        .unwrap();
    assert_eq!(
        producer.ring_event(released_with_reason(offer, 2, 11), 5),
        Err(ProducerError::Closed)
    );
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Unknown
    );
    let mut producer = ready();
    let (offer, _) = producer.upload(upload(), &[9u8; 8], 3).unwrap();
    producer
        .resource_status(11, 1, ResourceStatusKind::Cancelled, 4)
        .unwrap();
    assert_eq!(
        producer.ring_event(released_with_reason(offer, 2, 3), 5),
        Err(ProducerError::Closed)
    );
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Unknown
    );
}

#[test]
fn direct_fill_commits_without_a_copy() {
    let mut producer = ready();
    let (offer, bytes) = {
        let mut fill = producer.acquire_slot(3).unwrap();
        assert_eq!(fill.slot(), 0);
        fill.pixels_mut()[..8].copy_from_slice(&[4u8; 8]);
        // While acquired, the slot is not acquirable again.
        fill.commit(upload(), 8, 4).unwrap()
    };
    assert_eq!(bytes.len(), 152);
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 5)
        .unwrap();
    producer.ring_event(released(offer, 2), 6).unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Settled
    );
    // An aborted acquisition frees its slot without spending it.
    let mut producer = ready();
    {
        let _fill = producer.acquire_slot(3).unwrap();
    }
    let (offer, _) = producer.upload(upload(), &[1u8; 8], 4).unwrap();
    assert_eq!(offer, 1);
}

#[test]
fn over_capacity_and_refusal_rules() {
    let mut producer = ready();
    let big = vec![0u8; 4096 + 4];
    let mut big_upload = upload();
    big_upload.width = 1025;
    big_upload.height = 1;
    assert_eq!(
        producer.upload(big_upload, &big, 3),
        Err(ProducerError::Refused)
    );
    let (offer, _) = producer.upload(upload(), &[0u8; 8], 4).unwrap();
    producer.begin_refused(offer, 5).unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Refused
    );
    producer.journal_lost();
    assert!(producer.closed());
}

#[test]
fn generation_exhaustion_closes_without_wrapping() {
    let mut producer = ready();
    producer.slots[0].generation = u64::MAX - 1;
    assert_eq!(
        producer.upload(upload(), &[1u8; 8], 3),
        Err(ProducerError::Exhausted)
    );
    assert!(producer.closed());
    assert_eq!(producer.cause(), CloseCause::Exhausted);
    assert!(producer.offer(1).is_none(), "no offer was committed");
    assert_eq!(
        producer.slots[0].generation,
        u64::MAX - 1,
        "generation untouched"
    );
}

#[test]
fn offer_id_exhaustion_closes_on_the_next_commit() {
    let mut producer = ready();
    producer.next_offer = u64::MAX;
    let (offer, _) = producer.upload(upload(), &[1u8; 8], 3).unwrap();
    assert_eq!(offer, u64::MAX);
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 4)
        .unwrap();
    producer.ring_event(released(offer, 2), 5).unwrap();
    assert_eq!(
        producer.upload(upload(), &[1u8; 8], 6),
        Err(ProducerError::Exhausted)
    );
    assert!(producer.closed());
}

#[test]
fn release_before_status_fails_ring_and_makes_live_offers_unknown() {
    let mut producer = ready_slots(2);
    let (offer, _) = producer.upload(upload(), &[9u8; 8], 3).unwrap();
    let mut second = upload();
    second.resource = 12;
    let (offer2, _) = producer.upload(second, &[9u8; 8], 4).unwrap();
    // An exact release with no final status yet is a Session ordering
    // violation: the ring fails and every live offer goes Unknown.
    assert_eq!(
        producer.ring_event(released(offer, 2), 5),
        Err(ProducerError::Closed)
    );
    assert!(producer.closed());
    assert_eq!(producer.cause(), CloseCause::Fault);
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Unknown
    );
    assert_eq!(
        producer.offer(offer2).unwrap().state,
        RingOfferState::Unknown
    );
    // A late status cannot settle an Unknown offer.
    producer
        .resource_status(11, 1, ResourceStatusKind::Accepted, 6)
        .unwrap();
    assert_eq!(
        producer.offer(offer).unwrap().state,
        RingOfferState::Unknown
    );
}

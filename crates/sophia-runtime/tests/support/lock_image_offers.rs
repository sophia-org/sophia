//! t338 lock slice 4a: sealed offers in custody, from receipt to their
//! terminal status and source release, and the retired sealed image lease.
//! The test acts as owner: it reserves worker jobs, reports hand-overs and
//! supplies results. No worker or socket runs here.
use std::sync::Arc;
use std::time::Duration;

use super::*;
use sophia_runtime::{SealedValidationFailure, SharedPixels};

const RESOURCE_CANCEL: LockFileKind = LockFileKind::ResourceCancel;

fn ready() -> Provider {
    let mut provider = Provider::new(true);
    provider.negotiate(1, 2, PRESENT | SEALED);
    provider.custody.image_channel_ready(GENERATION, 1).unwrap();
    provider.events();
    while provider.custody.take_inbound().is_some() {}
    provider
}

fn kinds(events: &[(LockFileKind, Vec<u8>)]) -> Vec<LockFileKind> {
    events.iter().map(|(kind, _)| *kind).collect()
}

fn status(events: &[(LockFileKind, Vec<u8>)], index: usize) -> (LockResourceState, u16) {
    assert_eq!(events[index].0, LockFileKind::ResourceStatus);
    let status = LockResourceStatus::decode(&events[index].1).unwrap();
    (status.status, status.reason)
}

fn released(events: &[(LockFileKind, Vec<u8>)], index: usize) -> LockSourceDisposition {
    assert_eq!(events[index].0, LockFileKind::SourceReleased);
    LockSourceReleased::decode(&events[index].1)
        .unwrap()
        .disposition
}

fn resource(offer: u64) -> LockResourceId {
    LockResourceId {
        id: 20 + offer,
        generation: 1,
    }
}

fn source(metadata: LockImageMetadata) -> LockImagePacket {
    let unused = LockImagePlane {
        offset: 0,
        stride: 0,
        fd_index: 0,
    };
    let mut planes = [unused; LOCK_IMAGE_MAX_PLANES];
    planes[0].stride = metadata.stride;
    LockImagePacket::Source {
        connection_epoch: EPOCH,
        channel_generation: GENERATION,
        metadata,
        planes,
    }
}

fn pixels() -> SharedPixels {
    SharedPixels::from(Arc::<[u8]>::from(vec![7; 32]))
}

impl Provider {
    /// Admits the oldest pending offer under worker job `job`.
    fn admit(&mut self, job: u64) -> u64 {
        let admission = self.custody.image_admission(self.now).unwrap().unwrap();
        assert_eq!(admission.bytes, 32);
        self.custody
            .admit_image(admission.offer_id, job, admission.deadline)
            .unwrap();
        admission.offer_id
    }

    /// Offer `offer` through to the worker under job `job`.
    fn validating(&mut self, offer: u64, job: u64) {
        self.image_begin(offer).unwrap();
        assert_eq!(self.admit(job), offer);
        assert_eq!(
            self.custody.image_source(&source(metadata(offer)), 1),
            Ok(LockImageSource::Submit { job })
        );
        self.custody.image_source_submitted(job).unwrap();
        self.events();
    }

    fn cancel(&mut self, offer: u64) -> Result<(), Errno> {
        let step = LockResourceStep {
            transaction: offer,
            resource: resource(offer),
            total_bytes: None,
        };
        self.submit(RESOURCE_CANCEL, &step.encode().unwrap())
    }

    fn actions(&mut self) -> Vec<LockImageWorkerAction> {
        std::iter::from_fn(|| self.custody.take_image_worker_action()).collect()
    }
}

#[test]
fn a_validated_offer_becomes_a_resource_with_both_terminal_events() {
    let mut provider = ready();
    let before = provider.custody.position();
    provider.image_begin(1).unwrap();
    assert_eq!(kinds(&provider.events()), [LockFileKind::Submitted]);
    assert_eq!(provider.admit(9), 1);
    let events = provider.events();
    assert_eq!(kinds(&events), [LockFileKind::ImageAdmitted]);
    assert_eq!(
        LockImageMetadata::decode(&events[0].1).unwrap(),
        metadata(1)
    );
    // Nothing else pending.
    assert_eq!(provider.custody.image_admission(provider.now), Ok(None));
    assert_eq!(
        provider.custody.image_source(&source(metadata(1)), 1),
        Ok(LockImageSource::Submit { job: 9 })
    );
    // A second Source for the same offer faults the channel.
    assert_eq!(
        provider.custody.image_source(&source(metadata(1)), 1),
        Err(Errno::EPROTO)
    );
    provider.custody.image_source_submitted(9).unwrap();
    // A validating offer has no custody deadline: the worker's runs.
    assert_eq!(provider.custody.image_deadline(), None);
    provider.custody.image_completed(9, Ok(pixels())).unwrap();
    let events = provider.events();
    assert_eq!(status(&events, 0), (LockResourceState::Accepted, 0));
    assert_eq!(released(&events, 1), LockSourceDisposition::Retained);
    let Some(LockInbound::ResourceReady {
        resource: ready,
        width_px: 4,
        height_px: 2,
        pixels,
    }) = provider.custody.take_inbound()
    else {
        panic!("no resource ready");
    };
    assert_eq!(ready, resource(1));
    assert_eq!(&pixels[..], &[7; 32][..]);
    assert_eq!(
        provider.custody.image_completed(9, Ok(pixels)),
        Err(Errno::ESTALE)
    );
    assert!(provider.custody.position().next_sequence > before.next_sequence);
    assert!(provider.actions().is_empty());
}

#[test]
fn source_packets_fault_on_foreign_or_unadmitted_identity_and_stale_after_settling() {
    let mut provider = ready();
    provider.image_begin(1).unwrap();
    provider.image_begin(2).unwrap();
    provider.admit(5);
    // Above the admitted mark.
    assert_eq!(
        provider.custody.image_source(&source(metadata(2)), 1),
        Err(Errno::EPROTO)
    );
    let LockImagePacket::Source {
        metadata: admitted,
        planes,
        ..
    } = source(metadata(1))
    else {
        unreachable!()
    };
    let foreign = |connection_epoch, channel_generation| LockImagePacket::Source {
        connection_epoch,
        channel_generation,
        metadata: admitted,
        planes,
    };
    assert_eq!(
        provider
            .custody
            .image_source(&foreign(EPOCH + 1, GENERATION), 1),
        Err(Errno::EPROTO)
    );
    assert_eq!(
        provider
            .custody
            .image_source(&foreign(EPOCH, GENERATION + 1), 1),
        Err(Errno::EPROTO)
    );
    assert_eq!(
        provider.custody.image_source(&source(metadata(1)), 2),
        Err(Errno::EPROTO)
    );
    let mut differing = metadata(1);
    differing.declared_bytes = 64;
    differing.height = 4;
    assert_eq!(
        provider.custody.image_source(&source(differing), 1),
        Err(Errno::EPROTO)
    );
    // A Hello is not a Source.
    let hello = LockImagePacket::Hello {
        connection_epoch: EPOCH,
        channel_generation: GENERATION,
    };
    assert_eq!(provider.custody.image_source(&hello, 0), Err(Errno::EPROTO));
    // Cancelled before its Source: a late packet at or below the mark is
    // stale, not a fault.
    provider.events();
    provider.cancel(1).unwrap();
    assert_eq!(
        provider.custody.image_source(&source(metadata(1)), 1),
        Ok(LockImageSource::Stale)
    );
}

#[test]
fn worker_failures_reject_with_invalid_source_or_validation_timeout() {
    for (failure, reason) in [
        (
            SealedValidationFailure::Malformed,
            LOCK_RESOURCE_REASON_INVALID_SOURCE,
        ),
        (
            SealedValidationFailure::Timeout,
            LOCK_RESOURCE_REASON_VALIDATION_TIMEOUT,
        ),
        (
            SealedValidationFailure::Cancelled,
            LOCK_RESOURCE_REASON_VALIDATION_TIMEOUT,
        ),
    ] {
        let mut provider = ready();
        provider.validating(1, 3);
        provider.custody.image_completed(3, Err(failure)).unwrap();
        let events = provider.events();
        assert_eq!(status(&events, 0), (LockResourceState::Rejected, reason));
        assert_eq!(
            released(&events, 1),
            LockSourceDisposition::RefusedBeforeUse
        );
        assert!(provider.custody.take_inbound().is_none());
    }
}

#[test]
fn cancel_settles_unsent_offers_now_and_validating_ones_on_completion() {
    let mut provider = ready();
    // Pending: Cancelled, with no source to release.
    provider.image_begin(1).unwrap();
    provider.events();
    provider.cancel(1).unwrap();
    let events = provider.events();
    assert_eq!(kinds(&events)[0], LockFileKind::Submitted);
    assert_eq!(status(&events, 1), (LockResourceState::Cancelled, 0));
    assert_eq!(events.len(), 2);
    // Admitted, source unsent: also released before import, and the
    // reservation goes back.
    provider.image_begin(2).unwrap();
    provider.admit(4);
    provider.events();
    provider.cancel(2).unwrap();
    let events = provider.events();
    assert_eq!(status(&events, 1), (LockResourceState::Cancelled, 0));
    assert_eq!(
        released(&events, 2),
        LockSourceDisposition::CancelledBeforeImport
    );
    assert_eq!(provider.actions(), [LockImageWorkerAction::Release(4)]);
    // A cancel naming the wrong resource is not this offer's.
    provider.validating(3, 6);
    let step = LockResourceStep {
        transaction: 3,
        resource: resource(4),
        total_bytes: None,
    };
    assert_eq!(
        provider.submit(RESOURCE_CANCEL, &step.encode().unwrap()),
        Err(Errno::EINVAL)
    );
    // Validating: only the receipt now; the job is cancelled and its
    // completion settles the offer.
    provider.cancel(3).unwrap();
    assert_eq!(kinds(&provider.events()), [LockFileKind::Submitted]);
    assert_eq!(provider.actions(), [LockImageWorkerAction::Cancel(6)]);
    assert_eq!(provider.cancel(3), Err(Errno::EINVAL));
    // The cancel came first, so channel loss does not replace it, and a
    // result that arrives anyway is not imported.
    provider
        .custody
        .image_channel_closed(GENERATION, LockChannelClosedReason::PeerDisconnect)
        .unwrap();
    assert_eq!(kinds(&provider.events()), [LockFileKind::ChannelClosed]);
    assert!(provider.actions().is_empty());
    provider.custody.image_completed(6, Ok(pixels())).unwrap();
    let events = provider.events();
    assert_eq!(status(&events, 0), (LockResourceState::Cancelled, 0));
    assert_eq!(
        released(&events, 1),
        LockSourceDisposition::CancelledBeforeImport
    );
    assert!(provider.custody.take_inbound().is_none());
}

#[test]
fn channel_loss_rejects_unsent_offers_and_keeps_validating_ones_until_completion() {
    let mut provider = ready();
    provider.validating(1, 2);
    provider.image_begin(2).unwrap();
    provider.admit(3);
    provider.events();
    provider
        .custody
        .image_channel_closed(GENERATION, LockChannelClosedReason::ProtocolFault)
        .unwrap();
    let events = provider.events();
    assert_eq!(
        kinds(&events),
        [
            LockFileKind::ChannelClosed,
            LockFileKind::ResourceStatus,
            LockFileKind::SourceReleased,
        ]
    );
    let unavailable = LOCK_RESOURCE_REASON_IMAGE_CHANNEL_UNAVAILABLE;
    assert_eq!(
        status(&events, 1),
        (LockResourceState::Rejected, unavailable)
    );
    assert_eq!(
        released(&events, 2),
        LockSourceDisposition::CancelledBeforeImport
    );
    assert_eq!(
        provider.actions(),
        [
            LockImageWorkerAction::Release(3),
            LockImageWorkerAction::Cancel(2),
        ]
    );
    // The validating offer settles on its completion, as channel loss.
    provider.custody.image_completed(2, Ok(pixels())).unwrap();
    let events = provider.events();
    assert_eq!(
        status(&events, 0),
        (LockResourceState::Rejected, unavailable)
    );
    assert_eq!(
        released(&events, 1),
        LockSourceDisposition::CancelledBeforeImport
    );
    assert!(provider.custody.take_inbound().is_none());
    // Every offer credit came back: the journal takes a full batch again.
    provider.image_begin(4).unwrap();
    assert_eq!(
        kinds(&provider.events()),
        [LockFileKind::Submitted, LockFileKind::ResourceStatus]
    );
}

#[test]
fn deadlines_expire_unsent_offers_as_validation_timeouts() {
    let mut provider = ready();
    let timeout = Duration::from_millis(100);
    provider.image_begin(1).unwrap();
    provider.events();
    assert_eq!(
        provider.custody.image_deadline(),
        Some(provider.now + timeout)
    );
    // Pending past its deadline: status only, reason 11.
    let later = provider.now + timeout;
    assert_eq!(
        provider
            .custody
            .expire_images(later - Duration::from_millis(1)),
        Ok(false)
    );
    assert_eq!(provider.custody.expire_images(later), Ok(true));
    let events = provider.events();
    assert_eq!(events.len(), 1);
    let expired = LOCK_RESOURCE_REASON_VALIDATION_TIMEOUT;
    assert_eq!(status(&events, 0), (LockResourceState::Rejected, expired));
    // Admitted and its Source matched, but the worker refused the hand-over:
    // the offer keeps its reservation until the deadline, then is refused
    // before use.
    provider.now = later;
    provider.image_begin(2).unwrap();
    provider.admit(8);
    assert_eq!(
        provider.custody.image_source(&source(metadata(2)), 1),
        Ok(LockImageSource::Submit { job: 8 })
    );
    provider.events();
    assert!(provider.actions().is_empty());
    let deadline = provider.custody.image_deadline().unwrap();
    assert_eq!(provider.custody.expire_images(deadline), Ok(true));
    let events = provider.events();
    assert_eq!(status(&events, 0), (LockResourceState::Rejected, expired));
    assert_eq!(
        released(&events, 1),
        LockSourceDisposition::RefusedBeforeUse
    );
    assert_eq!(provider.actions(), [LockImageWorkerAction::Release(8)]);
    assert_eq!(provider.custody.image_deadline(), None);
}

#[test]
fn admission_refuses_offers_that_no_longer_fit_the_lock() {
    let mut cases: Vec<(LockImageMetadata, u16)> = Vec::new();
    let mut stale = metadata(1);
    stale.admission_epoch = 3;
    cases.push((stale, 1));
    let mut topology = metadata(1);
    topology.topology_generation = 2;
    cases.push((topology, 7));
    let mut allocation = metadata(1);
    allocation.allocation_id = 12;
    cases.push((allocation, 2));
    let mut wide = metadata(1);
    wide.width = 65;
    wide.stride = 65 * 4;
    wide.declared_bytes = 65 * 2 * 4;
    cases.push((wide, 6));
    for (offer, reason) in cases {
        let mut provider = ready();
        provider
            .submit(LockFileKind::ImageBegin, &offer.encode().unwrap())
            .unwrap();
        provider.events();
        assert_eq!(provider.custody.image_admission(provider.now), Ok(None));
        let events = provider.events();
        assert_eq!(events.len(), 1, "reason {reason}");
        assert_eq!(status(&events, 0), (LockResourceState::Rejected, reason));
    }
    // Past the deadline when the owner gets to it.
    let mut provider = ready();
    provider.image_begin(1).unwrap();
    provider.events();
    let late = provider.now + Duration::from_millis(100);
    assert_eq!(provider.custody.image_admission(late), Ok(None));
    let expired = LOCK_RESOURCE_REASON_VALIDATION_TIMEOUT;
    assert_eq!(
        status(&provider.events(), 0),
        (LockResourceState::Rejected, expired)
    );
    // The owner's own refusal, such as for want of worker credit.
    provider.image_begin(2).unwrap();
    provider.events();
    assert_eq!(provider.custody.refuse_image(2, 0), Err(Errno::EINVAL));
    provider.custody.refuse_image(2, 6).unwrap();
    assert_eq!(
        status(&provider.events(), 0),
        (LockResourceState::Rejected, 6)
    );
    assert_eq!(provider.custody.refuse_image(2, 6), Err(Errno::ESTALE));
}

#[test]
fn offers_share_identities_and_the_live_limit_with_uploads_but_no_slot() {
    let mut provider = ready();
    provider.image_begin(1).unwrap();
    provider.image_begin(2).unwrap();
    provider.events();
    // A third offer exceeds max_offers: receipt then budget, at once.
    provider.image_begin(3).unwrap();
    let events = provider.events();
    assert_eq!(kinds(&events)[0], LockFileKind::Submitted);
    assert_eq!(status(&events, 1), (LockResourceState::Rejected, 6));
    let begin = |transaction, resource, slot| {
        LockResourceBegin {
            transaction,
            resource,
            width_px: 4,
            height_px: 2,
            slot,
        }
        .encode()
        .unwrap()
    };
    // An upload may not take an offered identity.
    assert_eq!(
        provider.submit(LockFileKind::ResourceBegin, &begin(40, resource(1), 0)),
        Err(Errno::EINVAL)
    );
    // Two offers and one upload fill the three live places.
    let upload = LockResourceId {
        id: 60,
        generation: 1,
    };
    provider
        .submit(LockFileKind::ResourceBegin, &begin(40, upload, 0))
        .unwrap();
    assert_eq!(
        status(&provider.events(), 1),
        (LockResourceState::Admitted, 0)
    );
    let other = LockResourceId {
        id: 61,
        generation: 1,
    };
    provider
        .submit(LockFileKind::ResourceBegin, &begin(41, other, 1))
        .unwrap();
    assert_eq!(
        status(&provider.events(), 1),
        (LockResourceState::Rejected, 6)
    );
    // An offer may not take the upload's identity either.
    let mut taken = metadata(4);
    taken.resource = upload.id;
    assert_eq!(
        provider.submit(LockFileKind::ImageBegin, &taken.encode().unwrap()),
        Err(Errno::EINVAL)
    );
}

#[test]
fn a_retired_sealed_image_stays_charged_until_its_last_reader_lets_go() {
    let mut provider = ready();
    provider.validating(1, 2);
    provider.custody.image_completed(2, Ok(pixels())).unwrap();
    provider.events();
    let Some(LockInbound::ResourceReady { pixels: held, .. }) = provider.custody.take_inbound()
    else {
        panic!("no resource ready");
    };
    let retire = LockResourceStep {
        transaction: 30,
        resource: resource(1),
        total_bytes: None,
    };
    provider
        .submit(LockFileKind::ResourceRetire, &retire.encode().unwrap())
        .unwrap();
    // The receipt only: its release waits for every clone.
    assert_eq!(kinds(&provider.events()), [LockFileKind::Submitted]);
    assert!(matches!(
        provider.custody.take_inbound(),
        Some(LockInbound::ResourceRetired(retired)) if retired == resource(1)
    ));
    assert_eq!(provider.custody.sealed_leases_pending(), 1);
    assert_eq!(provider.custody.collect_sealed_leases(), Ok(false));
    // Its identity and live place stay taken: with two offers live the
    // limit of three is full.
    let mut again = metadata(5);
    again.resource = 21;
    assert_eq!(
        provider.submit(LockFileKind::ImageBegin, &again.encode().unwrap()),
        Err(Errno::EINVAL)
    );
    provider.image_begin(6).unwrap();
    provider.image_begin(7).unwrap();
    provider.events();
    let upload = LockResourceBegin {
        transaction: 40,
        resource: LockResourceId {
            id: 60,
            generation: 1,
        },
        width_px: 4,
        height_px: 2,
        slot: 0,
    }
    .encode()
    .unwrap();
    provider
        .submit(LockFileKind::ResourceBegin, &upload)
        .unwrap();
    assert_eq!(
        status(&provider.events(), 1),
        (LockResourceState::Rejected, 6)
    );
    drop(held);
    assert_eq!(provider.custody.collect_sealed_leases(), Ok(true));
    let events = provider.events();
    assert_eq!(kinds(&events), [LockFileKind::ResourceReleased]);
    assert_eq!(
        LockResourceReleased::decode(&events[0].1).unwrap(),
        LockResourceReleased {
            transaction: 30,
            resource: resource(1),
            reason: 0,
        }
    );
    assert_eq!(provider.custody.sealed_leases_pending(), 0);
    // The place is free again.
    provider
        .submit(LockFileKind::ResourceBegin, &upload)
        .unwrap();
    assert_eq!(
        status(&provider.events(), 1),
        (LockResourceState::Admitted, 0)
    );
}

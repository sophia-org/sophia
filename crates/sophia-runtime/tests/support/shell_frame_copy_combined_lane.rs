//! G3-5b service 1 at the runtime boundary: the lane gate before reserve,
//! the Fail outcome before admission, and the two proposals one image-owner
//! service call makes (service_with runs service_owned twice).
use super::*;
use crate::ShellTransportError;
use sophia_protocol::shell_files::{
    SHELL_FRAME_LEAN_CAPABILITY as LEAN, SHELL_FRAME_TERMINAL_ONLY_CAPABILITY as TERMINAL,
};

/// The same frame through the hooked service and through today's service,
/// each on its own fixture. Reserve must never run, and both must leave the
/// same result, jobs, charges and readiness.
fn matches_todays_path(setup: impl Fn(&mut Joined)) {
    let mut hooked = Joined::new();
    setup(&mut hooked);
    let hooked_result = hooked.try_service();
    assert_eq!(hooked.take_events(), ["collect"], "reserve is never called");
    assert!(hooked.probe.reserved.is_none());
    let mut plain = Joined::new();
    setup(&mut plain);
    let plain_result = plain.try_plain_service();
    assert_eq!(hooked_result, plain_result);
    assert_eq!(hooked.owner.pending(), plain.owner.pending());
    assert_eq!(hooked.ready(), plain.ready());
    let grant = identity().grant;
    assert_eq!(
        hooked.f.epochs.resources(grant).unwrap().usage(),
        plain.f.epochs.resources(grant).unwrap().usage()
    );
}

#[test]
fn a_frame_without_the_negotiated_lane_bits_takes_todays_path() {
    for cleared in [TERMINAL, LEAN] {
        matches_todays_path(|j| {
            j.f.transport.capabilities &= !cleared;
            j.f.submit(1);
        });
    }
    // Not vacuous: today's path admits this copy.
    let mut plain = Joined::new();
    plain.f.transport.capabilities &= !LEAN;
    plain.f.submit(1);
    plain.try_plain_service().unwrap();
    assert_eq!(plain.owner.pending(), 1, "today's path admitted the copy");
}

#[test]
fn a_copy_off_the_active_ring_takes_todays_path() {
    matches_todays_path(|j| {
        j.f.submit_with(1, |frame| {
            frame.image.as_mut().unwrap().identity.generation = 2;
        });
    });
}

/// The description the queued copy's actual request derives, before service.
fn queued_description(j: &mut Joined) -> Option<sophia_protocol::ContentResourceBegin> {
    let files = j.f.transport.files_mut().unwrap().export();
    let Some(ShellFrameRecord::Commit(frame)) = files.peek_frame_request() else {
        panic!("the copy is queued");
    };
    let offer = crate::owned_ring_protocol::Offer::from(frame.image.unwrap());
    j.f.epochs
        .owned_copy_description(identity().grant, &offer.request)
}

#[test]
fn a_copy_without_a_derivable_description_takes_todays_path() {
    // A wire-valid stale pair: the candidate facts and the image topology
    // agree on 11 while the store's facts are 12. Ring, grants and lane bits
    // are the fixture's own.
    let stale = |j: &mut Joined| {
        j.f.submit_with(1, |frame| {
            frame.candidate.facts_generation = 11;
            frame.image.as_mut().unwrap().topology_generation = 11;
        });
    };
    let mut valid = Joined::new();
    valid.f.submit(1);
    assert!(
        queued_description(&mut valid).is_some(),
        "not vacuous: the fixture's own copy derives a description"
    );
    let mut probe = Joined::new();
    stale(&mut probe);
    assert_eq!(
        queued_description(&mut probe),
        None,
        "the queued stale request derives no description"
    );
    matches_todays_path(stale);
}

#[test]
fn a_composition_failure_fails_the_turn_before_any_admission() {
    let mut j = Joined::new();
    let grant = identity().grant;
    let usage = j.f.epochs.resources(grant).unwrap().usage();
    j.probe.fail = Some("first composition error".into());
    j.f.submit(1);
    assert_eq!(
        j.try_service(),
        Err(ShellTransportError::CombinedComposition(
            "first composition error".into()
        ))
    );
    assert_eq!(
        j.take_events(),
        ["collect", "reserve"],
        "one proposal: the error ends the call, nothing composes again"
    );
    let (description, allocations) = j.probe.reserved.clone().unwrap();
    assert_eq!(description.resource, offer(1, 0, 2).request.resource);
    assert_eq!(allocations, 1, "the store's one allocation");
    assert!(j.probe.inbox.is_none() && j.probe.row.is_none());
    assert_eq!(j.owner.pending(), 0, "no job was admitted");
    assert!(!j.owes(), "the ring offer was not reserved");
    assert_eq!(j.f.epochs.resources(grant).unwrap().usage(), usage);
    let files = j.f.transport.files_mut().unwrap();
    assert!(
        matches!(
            files.export().peek_frame_request(),
            Some(ShellFrameRecord::Commit(_))
        ),
        "the frame stays queued for the owner's failure path"
    );
    // The owner's failure path disconnects; the existing withdrawal settles.
    j.f.transport.disconnect(&mut j.f.epochs).unwrap();
    j.try_plain_service().unwrap();
    assert_eq!(j.owner.pending(), 0);
}

/// Journal pressure: reserve_owned_offer answers EAGAIN, which defers.
fn fill_journal(j: &mut Joined) {
    let filler = OutboundRecord::Content(
        TransactionId::from_raw(90),
        ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant: identity().grant,
            resource: ContentResourceId {
                id: 7,
                generation: 1,
            },
            status: 3,
            reason: ContentReason::Malformed as u16,
            next_ordinal: 0,
            admitted_bytes: 0,
        }),
    );
    assert!(Peer::fill_journal(&mut j.f.transport, &filler) > 0);
}

fn ack_journal(j: &mut Joined) {
    let files = j.f.transport.files_mut().unwrap().export_mut();
    let sequence = files.journal.records() as u64;
    let floor = files.journal.size();
    files
        .journal
        .ack(
            ShellFileAck {
                connection_epoch: 1,
                sequence,
            },
            Instant::now(),
        )
        .unwrap();
    j.floor = floor;
}

#[test]
fn a_deferred_proposal_cancels_its_ticket_and_the_second_proposal_composes_again() {
    let mut j = Joined::new();
    // Submit first: a full journal would refuse the transaction itself.
    j.f.submit(1);
    fill_journal(&mut j);
    assert!(
        matches!(
            j.f.transport
                .files_mut()
                .unwrap()
                .export()
                .peek_frame_request(),
            Some(ShellFrameRecord::Commit(_))
        ),
        "the copy is queued under journal pressure"
    );
    j.service();
    assert_eq!(
        j.take_events(),
        ["collect", "reserve", "cancel", "reserve", "cancel"],
        "two proposals in one call, one cancel per reservation"
    );
    assert!(j.probe.row.is_none());
    assert_eq!(j.owner.pending(), 0);
    assert!(!j.owes());
    // Still deferred on the next call: the same order again.
    j.service();
    assert_eq!(
        j.take_events(),
        ["collect", "reserve", "cancel", "reserve", "cancel"]
    );
    // The pressure clears: the next call admits and submits it once.
    ack_journal(&mut j);
    j.service();
    assert_eq!(j.take_events(), ["collect", "reserve", "plan", "submitted"]);
    assert!(j.probe.row.is_some());
    assert_eq!(j.owner.pending(), 1);
}

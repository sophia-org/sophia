//! G3-5d d-2b3d-2 source-04, beside `combined_journal.rs`: the client's
//! ring offer records and the admission-boundary diagnostics (the
//! validation-01 diagnosis, sections 2 and 3). The vendored producer keeps
//! four offer records (sophia-image-producer ring.rs `offers: [Offer; 4]`,
//! `MAX_OFFERS`); the exact `SlotReleased` frees the slot and leaves the
//! record Settled or Refused, and only `image_forget` frees the record, so
//! a fill of more than four frames must forget each offer after its exact
//! release was read and applied. Decoded (the peer's read), routed,
//! applied (the record's state), slot reusable and record reusable are
//! distinct facts; the owner's journal window shows commitment only. The
//! diagnostics print raw facts before the frozen assertions and add no
//! turn, poll, ACK, wait or assertion change.
use super::*;
use sophia_shell_client::{RingOfferInfo, RingOfferState};

/// One ring offer the fill submitted and has not forgotten.
#[derive(Clone, Copy, Debug)]
pub(super) struct LiveOffer {
    pub(super) frame: u64,
    /// The offer id `submit_at` returned.
    pub(super) offer: u64,
    /// The end state its exact release produces: Settled for a filler,
    /// Refused for the INVALID prefix.
    pub(super) expected: RingOfferState,
}

/// The producer's offer-record capacity.
pub(super) const OFFER_RECORDS: usize = 4;

/// Before a submission: the peer's journal progress and every live
/// offer's record; a record must be free (a miss is a gap, not a retry).
pub(super) fn client_facts(a: &Attempt, fill: &Fill, label: &str) {
    let live: Vec<(u64, u64, Option<RingOfferInfo>)> = fill
        .offers
        .iter()
        .map(|live| (live.frame, live.offer, a.peer.image_ring_offer(live.offer)))
        .collect();
    eprintln!(
        "{label}: peer {:?}; live offers {live:?}",
        a.peer.journal_progress()
    );
    assert!(
        fill.offers.len() < OFFER_RECORDS,
        "{label}: no free offer record: {:?}",
        fill.offers
    );
}

/// Record the offer `submit_at` returned for frame `frame`: Pending and
/// naming the frame's resource.
pub(super) fn offered(
    fill: &mut Fill,
    a: &Attempt,
    frame: u64,
    offer: u64,
    expected: RingOfferState,
    label: &str,
) {
    let info = a
        .peer
        .image_ring_offer(offer)
        .unwrap_or_else(|| panic!("{label}: offer {offer} is unknown to the producer"));
    assert_eq!(
        (info.state, info.resource, info.resource_generation),
        (RingOfferState::Pending, RESOURCE.id, frame),
        "{label}: {info:?}"
    );
    fill.offers.push(LiveOffer {
        frame,
        offer,
        expected,
    });
    eprintln!("{label}: offer {offer} for frame {frame}: {info:?}");
}

/// The exact offer's record, naming its frame's resource.
fn info_of(a: &Attempt, live: LiveOffer, label: &str) -> RingOfferInfo {
    let info = a.peer.image_ring_offer(live.offer).unwrap_or_else(|| {
        panic!(
            "{label}: offer {} of frame {} is gone",
            live.offer, live.frame
        )
    });
    assert_eq!(
        (info.resource, info.resource_generation),
        (RESOURCE.id, live.frame),
        "{label}: {info:?}"
    );
    info
}

/// Forget the live offer at `index`: its record is in its expected end
/// state (Pending and Unknown are refused), the forget succeeds and the
/// record is absent afterwards.
fn forget(fill: &mut Fill, a: &mut Attempt, index: usize, label: &str) {
    let live = fill.offers[index];
    let info = info_of(a, live, label);
    assert_eq!(
        info.state, live.expected,
        "{label}: offer {} of frame {} before forget: {info:?}",
        live.offer, live.frame
    );
    eprintln!(
        "{label}: forget offer {} of frame {}: {info:?}",
        live.offer, live.frame
    );
    a.peer
        .image_forget(live.offer)
        .unwrap_or_else(|error| panic!("{label}: forget {}: {error:?}", live.offer));
    assert_eq!(
        a.peer.image_ring_offer(live.offer),
        None,
        "{label}: offer {} forgotten",
        live.offer
    );
    fill.offers.remove(index);
}

/// Frame `frame`'s offer, whose exact release the peer has just read and
/// applied: forgotten now.
pub(super) fn forget_frame(fill: &mut Fill, a: &mut Attempt, frame: u64, label: &str) {
    let index = fill
        .offers
        .iter()
        .position(|live| live.frame == frame)
        .unwrap_or_else(|| {
            panic!(
                "{label}: frame {frame} has no live offer: {:?}",
                fill.offers
            )
        });
    forget(fill, a, index, label);
}

/// After a peer read: every live offer whose exact release has been
/// applied (its expected end state) is forgotten; one still Pending stays
/// live and is printed; any other state fails.
pub(super) fn forget_observed(fill: &mut Fill, a: &mut Attempt, label: &str) {
    let mut index = 0;
    while index < fill.offers.len() {
        let live = fill.offers[index];
        let info = info_of(a, live, label);
        if info.state == RingOfferState::Pending {
            eprintln!(
                "{label}: offer {} of frame {} still pending: {info:?}",
                live.offer, live.frame
            );
            index += 1;
        } else {
            forget(fill, a, index, label);
        }
    }
}

/// The owner's facts across an admission turn, printed raw before the
/// frozen assertions: every retained entry at or after `pre`'s tail, both
/// journals, the outbox, the owned credits, the releasing list, the
/// stores, the row, the worker and the peer's progress.
pub(super) fn admission_facts(
    lane: &Lane,
    a: &Attempt,
    pre: &Snapshot,
    post: &Snapshot,
    label: &str,
) {
    let since = pre.journal().tail;
    let appended: Vec<&RetainedEntry> = post
        .facts
        .table
        .iter()
        .filter(|entry| entry_start(entry) >= since)
        .collect();
    eprintln!(
        "{label}: pre {:?} -> post {:?}; appended since {since}: {appended:#?}; outbox {:?} blocked {}; lean {:?}; credits {:?}; releasing {:?}; stores {:?}; row {:?} pending {} cancels {}; image pending {}; {:?}; {:?}; peer {:?}",
        pre.journal(),
        post.journal(),
        post.facts.outbox,
        post.facts.output_blocked,
        post.facts.lean_frames_in_flight,
        post.facts.owned_live_credits,
        post.releasing,
        post.stores,
        lane.states(),
        lane.row().pending(),
        lane.row().unconfirmed_cancels(),
        lane.h.owner.processes.image_pending(0),
        counters(lane),
        usage(lane),
        a.peer.journal_progress()
    );
}

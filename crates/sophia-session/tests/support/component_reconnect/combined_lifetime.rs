//! G3-5c Session controls for REV3's lifetime table, through the controlled
//! walk (`turn_combined`) at a chosen turn Instant and the bar's real
//! content service. Release facts are counted apart: `release_attempts`,
//! `release_transfers` and `renderer_releases` (see the fixture).
use super::consume::LATER;
use super::*;
use crate::shell_component_connections::combined_row::Verdict;

const FAILED: (u16, u16) = (3, ContentReason::RendererFailed as u16);

/// Control 7a (C4): a Mismatch is delivered as Malformed, so no bundle can
/// come. Same grant, no later generation: Unknown. The entry stays before
/// its deadline and is removed at it, with no second outcome.
#[test]
fn a_malformed_mismatch_entry_retires_without_a_second_outcome() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    lane.native
        .serve_combined(output().id, Instant::now(), StubReply::Mismatch);
    assert_eq!(lane.until(&mut a, "failed"), job);
    lane.drive_until(&mut a, "the candidate's rejection", |_, a| {
        !a.terminals(1).is_empty()
    });
    assert!(lane.turn().is_none());
    assert_eq!(
        lane.states(),
        [(job, "failed")],
        "c7a: Failed stays while Unknown before its deadline"
    );
    assert!(lane.turn_at(Instant::now() + LATER).is_none());
    assert!(lane.states().is_empty(), "c7a: retired at its deadline");
    assert_eq!(lane.h.owner.processes.combined_row(0).pending(), 0);
    lane.content(&mut a, false);
    assert_eq!(
        a.terminals(1),
        [(3, ContentReason::Malformed as u16)],
        "c7a: the Malformed outcome, once, and no RendererFailed"
    );
    assert_eq!((a.accepted, a.refused), (0, 1), "c7a: one ResourceStatus");
    assert_eq!(lane.native.release_attempts(), 0, "c7a: no lease");
}

/// Control 16 (R2-1): the walk expires a live candidate's Held result with
/// the Release cell Full; a later flush transfers it while the candidate is
/// still Pending; the bundle then takes the failure terminal.
#[test]
fn an_expiry_verdict_survives_a_later_debt_transfer_and_fails_its_bundle() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    lane.native.fill_renderer_queue();
    assert!(lane.turn_at(Instant::now() + LATER).is_none());
    assert_eq!(lane.states(), [(job, "debt")]);
    assert_eq!(
        lane.verdict(job),
        Some(Verdict::Expired),
        "c16: the walk expired the live candidate"
    );
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    assert!(lane.turn().is_none());
    assert_eq!(
        (lane.states(), lane.verdict(job)),
        (vec![(job, "failed")], Some(Verdict::Expired)),
        "c16: Failed(Expired) after the transfer"
    );
    assert_eq!(lane.h.owner.processes.combined_row(0).pending(), 1);
    lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
    assert_eq!(
        a.terminals(1),
        [FAILED],
        "c16: the expired candidate takes the failure terminal"
    );
    assert_eq!(lane.native.ordinary_queues(), 0, "c16: no install");
    assert_eq!(lane.native.accepts(), 0);
    assert_eq!(lane.native.classifies(), 0);
    assert!(lane.states().is_empty(), "c16: consumed");
    assert_eq!(lane.native.release_attempts(), 2, "release, then flush");
    assert_eq!(lane.native.release_transfers(), 1);
    assert_eq!(
        lane.native.renderer_releases(),
        1,
        "c16: one Release reached the renderer"
    );
}

/// Control 16's sibling: the walk's Release transfers at once.
#[test]
fn an_expiry_verdict_survives_an_immediate_transfer_and_fails_its_bundle() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    assert!(lane.turn_at(Instant::now() + LATER).is_none());
    assert_eq!(
        (lane.states(), lane.verdict(job)),
        (vec![(job, "failed")], Some(Verdict::Expired)),
        "c16 immediate: Failed(Expired) after the transfer"
    );
    lane.consume(&mut a, 1, Instant::now(), None, true).unwrap();
    assert_eq!(
        a.terminals(1),
        [FAILED],
        "c16 immediate: the failure terminal"
    );
    assert_eq!(lane.native.ordinary_queues(), 0);
    assert!(lane.states().is_empty());
    assert_eq!(lane.native.release_attempts(), 1);
    assert_eq!(lane.native.renderer_releases(), 1);
}

/// Control 17 (R2-2): the grant and stream stay open and every turn is
/// before the admission deadline. With no native the decision is recorded
/// and the terminal issued; the next walk releases the Held lease.
#[test]
fn a_no_native_consumption_records_its_decision_and_the_next_walk_releases_it() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let job = lane.held(&mut a, 1);
    let served = lane.consume(&mut a, 1, Instant::now(), None, false);
    assert_eq!(
        served,
        Ok(()),
        "c17: the no-native match never goes ordinary"
    );
    assert_eq!(a.terminals(1), [FAILED], "c17: one terminal, no install");
    assert_eq!(lane.native.ordinary_queues(), 0);
    assert!(lane.pending(&a).is_empty());
    assert_eq!(
        (lane.states(), lane.verdict(job)),
        (vec![(job, "held")], Some(Verdict::Consumed)),
        "c17: the decision is recorded and the lease still Held"
    );
    assert_eq!(lane.native.release_attempts(), 0);
    lane.native.fill_renderer_queue();
    assert!(lane.turn().is_none());
    assert_eq!(
        lane.native.release_attempts(),
        1,
        "c17: released at the next walk, before its deadline"
    );
    assert_eq!(
        (lane.states(), lane.verdict(job)),
        (vec![(job, "debt")], Some(Verdict::Consumed)),
        "c17: Consumed Debt"
    );
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    assert!(lane.turn().is_none());
    assert!(lane.states().is_empty(), "c17: the flush transferred");
    assert_eq!(lane.native.release_transfers(), 1);
    assert_eq!(lane.native.renderer_releases(), 1);
    assert!(lane.turn().is_none());
    assert_eq!(lane.native.release_attempts(), 2, "c17: nothing more");
    lane.content(&mut a, false);
    assert_eq!(a.terminals(1), [FAILED], "c17: no second terminal");
}

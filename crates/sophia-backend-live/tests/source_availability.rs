#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
//! Which retained surfaces have no drawable source, where, and why (t306).

use sophia_backend_live::{
    LiveRendererImageRetryGate, LiveSourceAvailability, LiveSourceUnavailableReason,
};
use sophia_protocol::{OutputId, SurfaceId};
use sophia_renderer_live::LiveRendererImageId;
use std::collections::BTreeSet;

fn surface(index: u32) -> SurfaceId {
    SurfaceId::new(index, 1)
}

fn output(raw: u64) -> OutputId {
    OutputId::from_raw(raw)
}

fn outputs(raws: &[u64]) -> Option<BTreeSet<OutputId>> {
    Some(raws.iter().map(|&raw| output(raw)).collect())
}

fn image(raw: u64) -> LiveRendererImageId {
    LiveRendererImageId::from_raw(raw)
}

#[test]
fn an_unavailable_surface_is_left_out_only_where_its_scope_says() {
    let mut availability = LiveSourceAvailability::default();
    availability.mark(
        surface(1),
        LiveSourceUnavailableReason::Pending(image(7)),
        outputs(&[2]),
    );
    availability.mark(surface(2), LiveSourceUnavailableReason::Lost, None);
    assert!(availability.available_on(surface(1), output(1), None));
    assert!(!availability.available_on(surface(1), output(2), None));
    assert!(!availability.available_on(surface(2), output(1), None));
    assert!(availability.available_on(surface(3), output(2), None));
}

#[test]
fn the_presenting_surface_is_never_left_out() {
    // Its new Present draws its new source; filtering it would block the very
    // frame that recovers it.
    let mut availability = LiveSourceAvailability::default();
    availability.mark(surface(4), LiveSourceUnavailableReason::Lost, None);
    assert!(availability.available_on(surface(4), output(1), Some(surface(4))));
    assert!(!availability.available_on(surface(4), output(1), Some(surface(5))));
}

#[test]
fn only_a_committed_present_recovers_a_lost_source() {
    let mut availability = LiveSourceAvailability::default();
    availability.mark(surface(1), LiveSourceUnavailableReason::Lost, None);
    // An image arriving is not the client drawing again.
    assert!(availability.image_local(image(1), output(1)).is_empty());
    assert!(!availability.available_on(surface(1), output(1), None));
    availability.committed(surface(1));
    assert!(availability.available_on(surface(1), output(1), None));
}

#[test]
fn a_pending_image_releases_its_surfaces_where_it_arrives() {
    let mut availability = LiveSourceAvailability::default();
    availability.mark(
        surface(1),
        LiveSourceUnavailableReason::Pending(image(9)),
        outputs(&[1, 2]),
    );
    availability.mark(
        surface(2),
        LiveSourceUnavailableReason::Pending(image(8)),
        outputs(&[1]),
    );
    assert_eq!(
        availability.image_local(image(9), output(1)),
        vec![surface(1)]
    );
    assert!(availability.available_on(surface(1), output(1), None));
    assert!(!availability.available_on(surface(1), output(2), None));
    assert!(!availability.available_on(surface(2), output(1), None));
    assert_eq!(
        availability.image_local(image(9), output(2)),
        vec![surface(1)]
    );
    assert!(availability.available_on(surface(1), output(2), None));
}

#[test]
fn lost_overrides_pending_and_scopes_of_one_reason_widen() {
    let mut availability = LiveSourceAvailability::default();
    availability.mark(
        surface(1),
        LiveSourceUnavailableReason::Pending(image(3)),
        outputs(&[1]),
    );
    availability.mark(
        surface(1),
        LiveSourceUnavailableReason::Pending(image(3)),
        outputs(&[2]),
    );
    assert!(!availability.available_on(surface(1), output(1), None));
    assert!(!availability.available_on(surface(1), output(2), None));
    availability.mark(surface(1), LiveSourceUnavailableReason::Lost, outputs(&[3]));
    // A later pending mark does not hide that nothing remains to wait for.
    availability.mark(
        surface(1),
        LiveSourceUnavailableReason::Pending(image(3)),
        None,
    );
    assert!(availability.image_local(image(3), output(3)).is_empty());
    assert!(!availability.available_on(surface(1), output(3), None));
}

#[test]
fn replaced_outputs_keep_only_what_is_lost_everywhere() {
    // Output ids are reused across topologies; a scope must not carry over
    // to whatever head the next topology gives the same raw id.
    let mut availability = LiveSourceAvailability::default();
    availability.mark(surface(1), LiveSourceUnavailableReason::Lost, outputs(&[2]));
    availability.mark(
        surface(2),
        LiveSourceUnavailableReason::Pending(image(5)),
        None,
    );
    availability.mark(surface(3), LiveSourceUnavailableReason::Lost, None);
    availability.outputs_replaced();
    assert!(availability.available_on(surface(1), output(2), None));
    assert!(availability.available_on(surface(2), output(1), None));
    assert!(!availability.available_on(surface(3), output(1), None));
}

#[test]
fn destroyed_surfaces_leave_no_entry() {
    let mut availability = LiveSourceAvailability::default();
    availability.mark(surface(1), LiveSourceUnavailableReason::Lost, None);
    availability.mark(surface(2), LiveSourceUnavailableReason::Lost, None);
    availability.prune(&[surface(1)]);
    assert_eq!(
        availability.surfaces().collect::<Vec<_>>(),
        vec![surface(2)]
    );
}

#[test]
fn pending_destinations_name_each_image_and_output_still_waited_for() {
    let mut availability = LiveSourceAvailability::default();
    availability.mark(
        surface(1),
        LiveSourceUnavailableReason::Pending(image(1)),
        outputs(&[2, 3]),
    );
    availability.mark(
        surface(2),
        LiveSourceUnavailableReason::Pending(image(4)),
        None,
    );
    availability.mark(surface(3), LiveSourceUnavailableReason::Lost, None);
    assert_eq!(
        availability.pending_destinations(),
        [
            (image(1), Some(output(2))),
            (image(1), Some(output(3))),
            (image(4), None)
        ]
        .into_iter()
        .collect()
    );
}

#[test]
fn publication_releases_only_output_scoped_pending_entries() {
    let mut availability = LiveSourceAvailability::default();
    availability.mark(
        surface(1),
        LiveSourceUnavailableReason::Pending(image(1)),
        outputs(&[2]),
    );
    availability.mark(
        surface(2),
        LiveSourceUnavailableReason::Pending(image(2)),
        None,
    );
    availability.mark(surface(3), LiveSourceUnavailableReason::Lost, outputs(&[2]));
    availability.release_output_scoped_pending();
    assert!(availability.available_on(surface(1), output(2), None));
    assert!(!availability.available_on(surface(2), output(1), None));
    assert!(!availability.available_on(surface(3), output(2), None));
}

#[test]
fn a_rebound_topology_drops_output_scopes_but_keeps_every_all_scope_entry() {
    let mut availability = LiveSourceAvailability::default();
    availability.mark(surface(1), LiveSourceUnavailableReason::Lost, outputs(&[2]));
    availability.mark(
        surface(2),
        LiveSourceUnavailableReason::Pending(image(5)),
        None,
    );
    availability.mark(surface(3), LiveSourceUnavailableReason::Lost, None);
    availability.outputs_rebound();
    assert!(availability.available_on(surface(1), output(2), None));
    assert!(!availability.available_on(surface(2), output(1), None));
    assert!(!availability.available_on(surface(3), output(1), None));
}

#[test]
fn a_full_store_sleeps_until_storage_changes_and_busy_work_is_retried() {
    let mut gate = LiveRendererImageRetryGate::default();
    // Never tried: due.
    assert!(gate.due(7));
    // Deferred because the store had no room, at storage progress 7: not due
    // again until progress moves, however many passes the owner makes.
    gate.observe(7, false);
    for _ in 0..3 {
        assert!(!gate.due(7));
    }
    // An eviction or a released allocation changes progress at the same
    // native retirement count: due.
    assert!(gate.due(8));
    // Deferred behind GPU work in flight: due on every pass until it settles.
    gate.observe(8, true);
    assert!(gate.due(8));
    // The attempt recorded the progress it left behind, so it does not wake
    // itself; only a new change or a busy deferral does.
    gate.observe(9, false);
    assert!(!gate.due(9));
    gate.clear();
    assert!(gate.due(9));
}

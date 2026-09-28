//! The legacy projection chunks' half of `tab_group_selection.rs`: the same
//! strict optional-surface contract through `encode/decode_wm_tab_groups`.
//! IPC-only: it retires with the socket wire (t269).
use sophia_protocol::*;

// Only the projection fixture is used here.
#[allow(dead_code)]
#[path = "support/policy_record_fixture.rs"]
mod fixture;

fn group(selected: Option<SurfaceId>, members: Vec<SurfaceId>) -> PolicyTabGroup {
    let mut group = fixture::proposal().tab_groups[0].clone();
    group.selected = selected;
    group.members = members;
    group
}

fn legacy(groups: Vec<PolicyTabGroup>) -> Result<Vec<PolicyTabGroup>, IpcCodecError> {
    decode_wm_tab_groups(&encode_wm_tab_groups(&groups, 2, 0)?)
}

#[test]
fn a_zero_index_member_round_trips_on_the_legacy_wire() {
    let groups = vec![group(
        Some(SurfaceId::new(3, 1)),
        vec![SurfaceId::new(0, 1), SurfaceId::new(3, 1)],
    )];
    assert_eq!(legacy(groups.clone()).unwrap(), groups);
}

#[test]
fn a_zero_generation_selection_is_refused_on_the_legacy_wire() {
    let groups = vec![group(
        Some(SurfaceId::new(5, 0)),
        vec![SurfaceId::new(5, 1)],
    )];
    assert!(legacy(groups).is_err());
}

#[test]
fn a_zero_index_selection_and_no_selection_round_trip_on_the_legacy_wire() {
    for groups in [
        vec![group(
            Some(SurfaceId::new(0, 1)),
            vec![SurfaceId::new(0, 1)],
        )],
        vec![group(None, Vec::new())],
    ] {
        assert_eq!(legacy(groups.clone()).unwrap(), groups);
    }
}

#[test]
fn an_all_ones_or_zero_generation_selection_is_refused_on_the_legacy_wire() {
    for (index, generation) in [(u32::MAX, 1), (u32::MAX, 0), (5, 0)] {
        let groups = vec![group(
            Some(SurfaceId::new(index, generation)),
            vec![SurfaceId::new(3, 1)],
        )];
        assert!(legacy(groups).is_err(), "{index} {generation}");
    }
}

//! The tab group selected surface on both wires (legacy projection chunks and
//! the WM file projection), which share `decode_policy_tab_groups_records`.
//!
//! It follows the strict optional-surface contract: no selection is exactly
//! (0, 0); a selection is any index but u32::MAX with a nonzero generation,
//! index zero included, in both directions. Before this contract (at
//! 6871d0d8, pinned by 71fde5fb) a selected index zero was refused, no
//! selection was written as (u32::MAX, 0) that the decoder refused, and a
//! selected all-ones index passed the codec to be refused only by Engine.
//! Membership and visible placement remain Engine's to judge.
use sophia_protocol::wm_files::*;
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

fn file(groups: Vec<PolicyTabGroup>) -> Result<Vec<PolicyTabGroup>, WmFilePayloadError> {
    let mut proposal = fixture::proposal();
    proposal.tab_groups = groups;
    let header = WmFileHeader {
        kind: WmFileKind::Projection,
        connection_epoch: proposal.connection_epoch,
        submission_id: 81,
        sequence: 0,
    };
    let bytes = encode_wm_file_projection(header, &proposal, u64::MAX)?;
    Ok(decode_wm_file_projection(&bytes, u64::MAX)?.tab_groups)
}

/// The group record's selected (index, generation) as the encoder writes it.
fn encoded_selected(group: PolicyTabGroup) -> (u32, u32) {
    let sections = encode_policy_tab_groups_records(&[group]).unwrap();
    let header = &sections[0].bytes;
    (
        u32::from_le_bytes(header[32..36].try_into().unwrap()),
        u32::from_le_bytes(header[36..40].try_into().unwrap()),
    )
}

/// One hand-written group record (48 bytes) with the given selected pair and
/// no members.
fn raw_group(index: u32, generation: u32) -> Vec<u8> {
    let template = group(None, Vec::new());
    let mut bytes = Vec::new();
    bytes.extend(template.output.raw().to_le_bytes());
    bytes.extend(template.group.to_le_bytes());
    for value in [
        template.geometry.x,
        template.geometry.y,
        template.geometry.width,
        template.geometry.height,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(index.to_le_bytes());
    bytes.extend(generation.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes
}

fn decode_raw(index: u32, generation: u32) -> Result<Vec<PolicyTabGroup>, IpcCodecError> {
    let bytes = raw_group(index, generation);
    decode_policy_tab_groups_records(&[PolicyRecordSectionRef {
        kind: PROJECTION_TAB_GROUP_RECORD_KIND,
        count: 1,
        bytes: &bytes,
    }])
}

#[test]
fn a_zero_index_member_round_trips_on_both_wires() {
    let groups = vec![group(
        Some(SurfaceId::new(3, 1)),
        vec![SurfaceId::new(0, 1), SurfaceId::new(3, 1)],
    )];
    assert_eq!(legacy(groups.clone()).unwrap(), groups);
    assert_eq!(file(groups.clone()).unwrap(), groups);
}

#[test]
fn a_zero_generation_selection_is_refused() {
    assert!(decode_raw(5, 0).is_err());
    let groups = vec![group(
        Some(SurfaceId::new(5, 0)),
        vec![SurfaceId::new(5, 1)],
    )];
    assert!(legacy(groups).is_err());
}

#[test]
fn a_zero_index_selection_and_no_selection_round_trip_on_both_wires() {
    assert_eq!(
        encoded_selected(group(
            Some(SurfaceId::new(0, 1)),
            vec![SurfaceId::new(0, 1)]
        )),
        (0, 1)
    );
    assert_eq!(encoded_selected(group(None, Vec::new())), (0, 0));
    assert_eq!(decode_raw(0, 0).unwrap()[0].selected, None);
    for groups in [
        vec![group(
            Some(SurfaceId::new(0, 1)),
            vec![SurfaceId::new(0, 1)],
        )],
        vec![group(None, Vec::new())],
    ] {
        assert_eq!(legacy(groups.clone()).unwrap(), groups);
        assert_eq!(file(groups.clone()).unwrap(), groups);
    }
}

#[test]
fn an_all_ones_or_zero_generation_selection_is_refused_in_both_directions() {
    for (index, generation) in [(u32::MAX, 1), (u32::MAX, 0), (5, 0)] {
        assert!(
            decode_raw(index, generation).is_err(),
            "{index} {generation}"
        );
        let groups = vec![group(
            Some(SurfaceId::new(index, generation)),
            vec![SurfaceId::new(3, 1)],
        )];
        assert!(encode_policy_tab_groups_records(&groups).is_err());
        assert!(legacy(groups.clone()).is_err());
        assert!(file(groups).is_err());
    }
}

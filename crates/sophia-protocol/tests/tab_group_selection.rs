//! Characterization of the tab group selected-surface codec at 6871d0d8, on
//! both wires (legacy projection chunks and the WM file projection), which
//! share `decode_policy_tab_groups_records`. These pin current behaviour; they
//! are not the intended contract.
//!
//! The strict optional-surface contract used by the file bodies is: absent is
//! exactly (0, 0); present is any index but u32::MAX with a nonzero
//! generation, index zero included. Member records already follow the present
//! half. The selected field does not:
//!
//! - a selected surface at index zero is refused, although the same surface is
//!   an accepted member and Engine accepts the selection;
//! - no selection is encoded as `SurfaceId::INVALID`, (u32::MAX, 0), which the
//!   decoder refuses, so a group without a selection cannot round trip; only a
//!   hand-written (0, 0) decodes as none;
//! - a selected all-ones index with a nonzero generation is accepted by the
//!   codec and refused only later by Engine, which finds it among no members.
//!
//! The tests marked `ignore` state the strict contract; a fix should flip them.
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
fn a_zero_index_selection_is_refused_on_both_wires() {
    let groups = vec![group(
        Some(SurfaceId::new(0, 1)),
        vec![SurfaceId::new(0, 1)],
    )];
    assert_eq!(encoded_selected(groups[0].clone()), (0, 1));
    assert!(legacy(groups.clone()).is_err());
    assert!(file(groups).is_err());
}

#[test]
fn no_selection_encodes_a_sentinel_its_own_decoder_refuses() {
    let groups = vec![group(None, Vec::new())];
    assert_eq!(encoded_selected(groups[0].clone()), (u32::MAX, 0));
    assert!(legacy(groups.clone()).is_err());
    assert!(file(groups).is_err());
    // Only a hand-written (0, 0) decodes as no selection.
    assert_eq!(decode_raw(0, 0).unwrap()[0].selected, None);
    assert!(decode_raw(u32::MAX, 0).is_err());
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
fn an_all_ones_selected_index_passes_the_codec_and_is_left_to_engine() {
    let selected = SurfaceId::new(u32::MAX, 1);
    assert_eq!(decode_raw(u32::MAX, 1).unwrap()[0].selected, Some(selected));
    let groups = vec![group(Some(selected), vec![SurfaceId::new(3, 1)])];
    assert_eq!(legacy(groups.clone()).unwrap(), groups);
    assert_eq!(file(groups.clone()).unwrap(), groups);
}

#[test]
fn contract_zero_index_selection_and_no_selection_round_trip() {
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
fn contract_all_ones_selected_index_is_refused_by_the_codec() {
    assert!(decode_raw(u32::MAX, 1).is_err());
}

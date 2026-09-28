#[test]
fn output_candidate_supports_mirrored_and_extended_groups_with_independent_modes() {
    let snapshot = output_authority_snapshot();
    assert_eq!(snapshot.validate(), Ok(()));
    assert_eq!(mixed_output_candidate().validate_against(&snapshot), Ok(()));
}

#[test]
fn output_candidate_rejects_stale_head_and_duplicate_membership() {
    let snapshot = output_authority_snapshot();
    let mut candidate = mixed_output_candidate();
    candidate.heads[1].head_generation -= 1;
    assert_eq!(
        candidate.validate_against(&snapshot),
        Err(OutputTopologyCandidateError::StaleHead(
            DisplayHeadId::from_raw(2)
        ))
    );

    let mut candidate = mixed_output_candidate();
    candidate.groups[1].members[0].head = DisplayHeadId::from_raw(2);
    assert_eq!(
        candidate.validate_against(&snapshot),
        Err(OutputTopologyCandidateError::DuplicateMembership(
            DisplayHeadId::from_raw(2)
        ))
    );
}

#[test]
fn output_candidate_can_split_one_mirror_member_into_a_new_extended_output() {
    let snapshot = output_authority_snapshot();
    let mut candidate = mixed_output_candidate();
    candidate.groups[0].members.pop();
    candidate.groups.insert(
        1,
        OutputLogicalGroupProposal {
            output: OutputId::INVALID,
            logical: Rect {
                x: 2560,
                y: 0,
                width: 1920,
                height: 1080,
            },
            members: vec![OutputGroupMember {
                head: DisplayHeadId::from_raw(2),
                mapping: OutputHeadMapping::Fit,
            }],
        },
    );
    candidate.groups[2].logical.x = 4480;
    assert_eq!(candidate.validate_against(&snapshot), Ok(()));
}

#[test]
fn output_candidate_rejects_overlapping_extended_groups() {
    let snapshot = output_authority_snapshot();
    let mut candidate = mixed_output_candidate();
    candidate.groups[1].logical.x = 1280;
    assert_eq!(
        candidate.validate_against(&snapshot),
        Err(OutputTopologyCandidateError::LogicalOverlap {
            first: 0,
            second: 1,
        })
    );
}

//! Neutral topology assertions retained from the retired output socket peer.
use sophia_protocol::{
    DisplayHeadId, DisplayModeId, OutputAuthoritySnapshot, OutputGroupMember, OutputHeadDescriptor,
    OutputHeadMapping, OutputLogicalGroupState, OutputModeDescriptor, OutputTransformSet,
    OutputVrrPolicy, Rect, Size,
};
#[path = "support/output_candidate.rs"]
mod peer;
use peer::{
    MirrorSizingPolicy, mixed_mirror_extended_candidate, mixed_mirror_extended_topology_is_applied,
};

#[test]
fn mixed_candidate_keeps_modes_and_forms_one_mirror_plus_one_extended_group() {
    let snapshot = three_head_snapshot();
    let candidate = mixed_mirror_extended_candidate(
        &snapshot,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::OptimizeForPrimary,
    )
    .unwrap();

    assert_eq!(candidate.heads.len(), 3);
    assert_eq!(candidate.groups.len(), 2);
    assert_eq!(candidate.groups[0].output.raw(), 1);
    assert_eq!(candidate.groups[0].members.len(), 2);
    assert_eq!(
        candidate.groups[0].members[0].mapping,
        OutputHeadMapping::Exact
    );
    assert_eq!(
        candidate.groups[0].members[1].mapping,
        OutputHeadMapping::Fit
    );
    assert_eq!(candidate.groups[1].output.raw(), 3);
    assert_eq!(
        candidate.groups[1].members[0].mapping,
        OutputHeadMapping::Exact
    );
    assert_eq!(candidate.groups[1].logical.x, 2560);
    assert_eq!(candidate.groups[1].logical.width, 1920);
    assert_eq!(candidate.heads[0].mode.raw(), 11);
    assert_eq!(candidate.heads[1].mode.raw(), 12);
    assert_eq!(candidate.heads[2].mode.raw(), 13);
    assert!(
        candidate
            .heads
            .iter()
            .all(|head| head.vrr == OutputVrrPolicy::Disabled)
    );
    candidate.validate_against(&snapshot).unwrap();
}

/// Optimizing for the smaller member sizes the group to it and swaps who
/// resamples.
///
/// A mirror group has one logical size, so exactly one member can be exact.
/// Which one is a choice about the desk, not about the compositor: every head
/// keeps its own mode either way, and the extended output simply starts where
/// the narrower group now ends.
#[test]
fn mixed_candidate_optimized_for_the_member_sizes_the_group_to_it() {
    let snapshot = three_head_snapshot();
    let candidate = mixed_mirror_extended_candidate(
        &snapshot,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::OptimizeForMember,
    )
    .unwrap();

    assert_eq!(candidate.groups[0].logical.width, 1920);
    assert_eq!(candidate.groups[0].logical.height, 1080);
    assert_eq!(
        candidate.groups[0].members[0].mapping,
        OutputHeadMapping::Fit
    );
    assert_eq!(
        candidate.groups[0].members[1].mapping,
        OutputHeadMapping::Exact
    );
    assert_eq!(candidate.groups[1].logical.x, 1920);
    // Every head keeps the mode it was already running.
    assert_eq!(candidate.heads[0].mode.raw(), 11);
    assert_eq!(candidate.heads[1].mode.raw(), 12);
    assert_eq!(candidate.heads[2].mode.raw(), 13);
    candidate.validate_against(&snapshot).unwrap();
}

/// Centring unscaled sizes the group to fit inside both members, so neither
/// resamples.
///
/// The other two policies each make one panel pixel-exact by making the other
/// stretch. This one gives that up: the group takes a size both heads contain,
/// every member maps exactly, and a head with room left over shows a border
/// instead of a resampled image. It is the only policy under which both panels
/// are pixel-exact at once, and the border is what it costs.
#[test]
fn mixed_candidate_centered_unscaled_leaves_both_members_exact() {
    let snapshot = three_head_snapshot();
    let candidate = mixed_mirror_extended_candidate(
        &snapshot,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::CenterUnscaled,
    )
    .unwrap();

    // The smaller member's mode, which is the largest rectangle both contain.
    assert_eq!(candidate.groups[0].logical.width, 1920);
    assert_eq!(candidate.groups[0].logical.height, 1080);
    assert_eq!(
        candidate.groups[0].members[0].mapping,
        OutputHeadMapping::Exact
    );
    assert_eq!(
        candidate.groups[0].members[1].mapping,
        OutputHeadMapping::Exact
    );
    // Nothing resampled, and no head changed mode to achieve it.
    assert_eq!(candidate.heads[0].mode.raw(), 11);
    assert_eq!(candidate.heads[1].mode.raw(), 12);
    assert_eq!(candidate.heads[2].mode.raw(), 13);
    assert_eq!(candidate.groups[1].logical.x, 1920);
    candidate.validate_against(&snapshot).unwrap();
}

/// The size is the minimum on each axis, not whichever head is smaller.
///
/// Two heads need not be ordered: one can be wider while the other is taller,
/// and then neither mode fits inside the other. Taking either one whole would
/// leave the other head's image running past its edge, where `clip_to_target`
/// crops it rather than bordering it -- a policy that promises nothing resamples
/// would instead silently lose pixels. The per-axis minimum is the largest
/// rectangle both heads contain, and under it both still map exactly.
#[test]
fn centered_unscaled_fits_heads_that_are_larger_on_different_axes() {
    let mut snapshot = three_head_snapshot();
    snapshot.heads[0] = head(1, 11, "Display 1", 2560, 1080);
    snapshot.heads[1] = head(2, 12, "Display 2", 1920, 1440);
    snapshot.groups[0].logical.width = 2560;
    snapshot.groups[0].logical.height = 1080;

    let candidate = mixed_mirror_extended_candidate(
        &snapshot,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::CenterUnscaled,
    )
    .unwrap();

    assert_eq!(candidate.groups[0].logical.width, 1920);
    assert_eq!(candidate.groups[0].logical.height, 1080);
    for member in &candidate.groups[0].members {
        assert_eq!(member.mapping, OutputHeadMapping::Exact);
    }
    candidate.validate_against(&snapshot).unwrap();
}

/// The applied-topology predicate reads the size, not only the mappings.
///
/// Two exact members sized to the larger head crop the smaller one instead of
/// bordering it, and that configuration wears exactly the pair of mappings
/// centre-unscaled produces. A predicate that compared mappings alone would call
/// it settled, and the proof would report a desk it had not built.
#[test]
fn applied_topology_rejects_exact_members_at_the_wrong_logical_size() {
    let snapshot = three_head_snapshot();
    let candidate = mixed_mirror_extended_candidate(
        &snapshot,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::CenterUnscaled,
    )
    .unwrap();

    let mut applied = snapshot.clone();
    applied.groups[0].logical = candidate.groups[0].logical;
    applied.groups[0].members = candidate.groups[0].members.clone();
    applied.groups[1].logical = candidate.groups[1].logical;
    applied.groups[1].members = candidate.groups[1].members.clone();
    assert!(mixed_mirror_extended_topology_is_applied(
        &applied,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::CenterUnscaled,
    ));

    // Same mappings, the larger head's size: the smaller member is cropped.
    let mut cropped = applied.clone();
    cropped.groups[0].logical.width = 2560;
    cropped.groups[0].logical.height = 1440;
    assert!(!mixed_mirror_extended_topology_is_applied(
        &cropped,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::CenterUnscaled,
    ));
}

/// A restarted proof recognizes the topology it already applied.
///
/// The supervisor restarts this policy, and a restart lands after its topology
/// is live. The candidate it would rebuild names a base epoch the compositor
/// has moved past, so the compositor refuses it as stale -- and a proof that
/// reads any non-commit as failure exits, is restarted, and exhausts the
/// supervisor. A live session died exactly that way over one transport
/// timeout.
#[test]
fn an_applied_mixed_topology_is_recognized_without_resubmitting_it() {
    let snapshot = three_head_snapshot();
    assert!(!mixed_mirror_extended_topology_is_applied(
        &snapshot,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::OptimizeForPrimary,
    ));

    let applied = OutputAuthoritySnapshot {
        topology_epoch: 5,
        primary_output: sophia_protocol::OutputId::from_raw(1),
        heads: snapshot.heads.clone(),
        groups: vec![
            OutputLogicalGroupState {
                output: sophia_protocol::OutputId::from_raw(1),
                generation: 3,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
                members: vec![
                    OutputGroupMember {
                        head: DisplayHeadId::from_raw(1),
                        mapping: OutputHeadMapping::Exact,
                    },
                    OutputGroupMember {
                        head: DisplayHeadId::from_raw(2),
                        mapping: OutputHeadMapping::Fit,
                    },
                ],
            },
            OutputLogicalGroupState {
                output: sophia_protocol::OutputId::from_raw(3),
                generation: 3,
                logical: Rect {
                    x: 2560,
                    y: 0,
                    width: 1920,
                    height: 1200,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Exact,
                }],
            },
        ],
    };

    assert!(mixed_mirror_extended_topology_is_applied(
        &applied,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::OptimizeForPrimary,
    ));
    // The other optimization is a different desk, and is not this one.
    assert!(!mixed_mirror_extended_topology_is_applied(
        &applied,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::OptimizeForMember,
    ));
}

#[test]
fn mixed_candidate_refuses_to_disable_an_unmentioned_connected_head() {
    let mut snapshot = three_head_snapshot();
    snapshot.heads.push(head(4, 14, "Display 4", 1280, 1024));
    snapshot.groups.push(group(4, 4, 4480, 1280, 1024));

    let error = mixed_mirror_extended_candidate(
        &snapshot,
        "Display 1",
        "Display 2",
        "Display 3",
        MirrorSizingPolicy::OptimizeForPrimary,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("proof requires exactly three connected heads")
    );
}

fn three_head_snapshot() -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: 4,
        primary_output: sophia_protocol::OutputId::from_raw(1),
        heads: vec![
            head(1, 11, "Display 1", 2560, 1440),
            head(2, 12, "Display 2", 1920, 1080),
            head(3, 13, "Display 3", 1920, 1200),
        ],
        groups: vec![
            group(1, 1, 0, 2560, 1440),
            group(2, 2, 2560, 1920, 1080),
            group(3, 3, 4480, 1920, 1200),
        ],
    }
}

fn head(raw: u64, mode_raw: u64, label: &str, width: i32, height: i32) -> OutputHeadDescriptor {
    OutputHeadDescriptor {
        head: DisplayHeadId::from_raw(raw),
        generation: 2,
        label: label.to_owned(),
        connected: true,
        enabled: true,
        current_mode: Some(DisplayModeId::from_raw(mode_raw)),
        transforms: OutputTransformSet::NORMAL,
        vrr_capable: false,
        modes: vec![OutputModeDescriptor {
            mode: DisplayModeId::from_raw(mode_raw),
            pixel_size: Size { width, height },
            refresh_millihz: 60_000,
            preferred: true,
        }],
    }
}

fn group(
    output_raw: u64,
    head_raw: u64,
    x: i32,
    width: i32,
    height: i32,
) -> OutputLogicalGroupState {
    OutputLogicalGroupState {
        output: sophia_protocol::OutputId::from_raw(output_raw),
        generation: 2,
        logical: Rect {
            x,
            y: 0,
            width,
            height,
        },
        members: vec![OutputGroupMember {
            head: DisplayHeadId::from_raw(head_raw),
            mapping: OutputHeadMapping::Exact,
        }],
    }
}

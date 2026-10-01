//! Pure candidate construction retained from the retired output reference peer.
use sophia_protocol::{
    OutputAuthoritySnapshot, OutputGroupMember, OutputHeadDescriptor, OutputHeadMapping,
    OutputHeadTargetProposal, OutputLogicalGroupProposal, OutputLogicalGroupState,
    OutputTopologyCandidate, OutputTopologyCandidateError, OutputTopologyIntent, OutputTransform,
    OutputVrrPolicy, Rect,
};

#[derive(Debug)]
pub enum CandidateError {
    Candidate(OutputTopologyCandidateError),
    InvalidProofTopology(&'static str),
}
impl core::fmt::Display for CandidateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Candidate(error) => write!(f, "output candidate: {error}"),
            Self::InvalidProofTopology(reason) => write!(f, "invalid proof topology: {reason}"),
        }
    }
}
impl std::error::Error for CandidateError {}
impl From<OutputTopologyCandidateError> for CandidateError {
    fn from(error: OutputTopologyCandidateError) -> Self {
        Self::Candidate(error)
    }
}

/// How a mirror group chooses its logical size, and what that costs.
///
/// A mirror group has one logical size and its members are placed into it, so a
/// member whose mode differs from that size either resamples to reach it or
/// keeps its own pixels and leaves the remainder unused. There is no third
/// outcome, and which one a group takes is a property of the group rather than a
/// mode change: every head keeps its own mode under all three policies.
///
/// Named for the policy rather than for a head. Two of these do name a head, and
/// the third deliberately optimizes for neither, so a type called "which head is
/// optimized" could not hold it without one of its values meaning something else.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MirrorSizingPolicy {
    /// Size to the primary. It stays pixel-exact and the member resamples down.
    #[default]
    OptimizeForPrimary,
    /// Size to the member. It stays pixel-exact and the primary resamples up.
    OptimizeForMember,
    /// Size so that every member contains the logical image at its own scale.
    ///
    /// Nothing resamples. The size is the smallest mode on each axis across the
    /// group, so it fits inside every member, and `OutputHeadMapping::Exact`
    /// centres it there — a head larger than the image shows a border rather than
    /// a stretch. This is what macOS's "optimize for" cannot express, and it is
    /// the only policy under which both panels are pixel-exact at once.
    ///
    /// The per-axis minimum can name a size no member's mode matches, when two
    /// heads are larger on different axes. That is still correct: every member
    /// contains it exactly, and every member shows a border.
    CenterUnscaled,
}

/// Builds a complete three-head candidate: two heads mirror one logical output,
/// and the third extends it to the right.
///
/// The proof shape is deliberately exact. Extra connected heads are rejected
/// instead of being disabled as a side effect, and all three heads keep their
/// current modes under every policy. The extended head is always exact; within
/// the mirror group, `policy` decides both the logical size and which members
/// reach it by resampling. Head identity comes entirely from the labels this
/// call is given: nothing here knows a connector name or a panel size.
pub fn mixed_mirror_extended_candidate(
    snapshot: &OutputAuthoritySnapshot,
    mirror_primary_label: &str,
    mirror_member_label: &str,
    extended_label: &str,
    policy: MirrorSizingPolicy,
) -> Result<OutputTopologyCandidate, CandidateError> {
    snapshot.validate()?;
    if mirror_primary_label == mirror_member_label
        || mirror_primary_label == extended_label
        || mirror_member_label == extended_label
    {
        return Err(CandidateError::InvalidProofTopology(
            "proof labels are not distinct",
        ));
    }
    let connected = snapshot
        .heads
        .iter()
        .filter(|head| head.connected)
        .collect::<Vec<_>>();
    if connected.len() != 3 {
        return Err(CandidateError::InvalidProofTopology(
            "proof requires exactly three connected heads",
        ));
    }
    let primary = head_by_label(&connected, mirror_primary_label)?;
    let member = head_by_label(&connected, mirror_member_label)?;
    let extended = head_by_label(&connected, extended_label)?;
    for head in [primary, member, extended] {
        if !head.enabled || head.current_mode.is_none() {
            return Err(CandidateError::InvalidProofTopology(
                "proof head is not enabled with a current mode",
            ));
        }
        if !head.transforms.contains(OutputTransform::Normal) {
            return Err(CandidateError::InvalidProofTopology(
                "proof head does not support the normal transform",
            ));
        }
    }

    let primary_group = group_for_head(snapshot, primary)?;
    let member_group = group_for_head(snapshot, member)?;
    let extended_group = group_for_head(snapshot, extended)?;
    if primary_group.output == extended_group.output {
        return Err(CandidateError::InvalidProofTopology(
            "mirror primary and extended head already share one logical output",
        ));
    }
    // A pre-existing mirror is acceptable, but an unrelated shared identity is
    // not: consuming it would silently remove another logical placement.
    if member_group.output != primary_group.output && member_group.output == extended_group.output {
        return Err(CandidateError::InvalidProofTopology(
            "mirror member currently belongs to the extended output",
        ));
    }

    let mirror_size = mirror_logical_size(primary, member, policy)?;
    let mirror_logical = Rect {
        x: primary_group.logical.x,
        y: primary_group.logical.y,
        width: mirror_size.width,
        height: mirror_size.height,
    };
    let extended_x = mirror_logical.x.checked_add(mirror_logical.width).ok_or(
        CandidateError::InvalidProofTopology("extended placement overflows root coordinates"),
    )?;
    let (primary_mapping, member_mapping) = mirror_member_mappings(policy);
    let targets = [primary, member, extended]
        .into_iter()
        .map(|head| OutputHeadTargetProposal {
            head: head.head,
            head_generation: head.generation,
            mode: head
                .current_mode
                .expect("enabled proof head has a current mode"),
            transform: OutputTransform::Normal,
            vrr: OutputVrrPolicy::Disabled,
        })
        .collect::<Vec<_>>();
    let candidate = OutputTopologyCandidate {
        base_topology_epoch: snapshot.topology_epoch,
        intent: OutputTopologyIntent::Apply,
        primary_group_index: 0,
        heads: targets,
        groups: vec![
            OutputLogicalGroupProposal {
                output: primary_group.output,
                logical: mirror_logical,
                members: vec![
                    OutputGroupMember {
                        head: primary.head,
                        mapping: primary_mapping,
                    },
                    OutputGroupMember {
                        head: member.head,
                        mapping: member_mapping,
                    },
                ],
            },
            OutputLogicalGroupProposal {
                output: extended_group.output,
                logical: Rect {
                    x: extended_x,
                    y: mirror_logical.y,
                    width: extended_group.logical.width,
                    height: extended_group.logical.height,
                },
                members: vec![OutputGroupMember {
                    head: extended.head,
                    mapping: OutputHeadMapping::Exact,
                }],
            },
        ],
    };
    candidate.validate_against(snapshot)?;
    Ok(candidate)
}

/// Whether the snapshot already shows the topology this proof asks for.
///
/// A supervised policy is restarted, and a restart lands after the topology it
/// applied is already live. Re-submitting the candidate it built the first time
/// names a base epoch the compositor has moved past, which is refused as stale
/// -- correctly, and fatally for a proof that reads any non-commit as failure.
/// So the proof asks first whether the desk already looks the way it wants,
/// which is the only question that survives being asked twice.
pub fn mixed_mirror_extended_topology_is_applied(
    snapshot: &OutputAuthoritySnapshot,
    mirror_primary_label: &str,
    mirror_member_label: &str,
    extended_label: &str,
    policy: MirrorSizingPolicy,
) -> bool {
    let connected = snapshot
        .heads
        .iter()
        .filter(|head| head.connected)
        .collect::<Vec<_>>();
    let (Ok(primary), Ok(member), Ok(extended)) = (
        head_by_label(&connected, mirror_primary_label),
        head_by_label(&connected, mirror_member_label),
        head_by_label(&connected, extended_label),
    ) else {
        return false;
    };
    let (Ok(primary_group), Ok(member_group), Ok(extended_group)) = (
        group_for_head(snapshot, primary),
        group_for_head(snapshot, member),
        group_for_head(snapshot, extended),
    ) else {
        return false;
    };
    if primary_group.output != member_group.output
        || primary_group.output == extended_group.output
        || extended_group.members.len() != 1
    {
        return false;
    }
    let (primary_mapping, member_mapping) = mirror_member_mappings(policy);
    // The size, not only the mappings. Two exact members sized to the larger
    // head crop the smaller one instead of bordering it, and that configuration
    // wears the same pair of mappings as the one that does not.
    let Ok(expected_size) = mirror_logical_size(primary, member, policy) else {
        return false;
    };
    if primary_group.logical.width != expected_size.width
        || primary_group.logical.height != expected_size.height
    {
        return false;
    }
    let mapping_of = |head: &OutputHeadDescriptor| {
        primary_group
            .members
            .iter()
            .find(|candidate| candidate.head == head.head)
            .map(|candidate| candidate.mapping)
    };
    mapping_of(primary) == Some(primary_mapping) && mapping_of(member) == Some(member_mapping)
}

/// The logical size a mirror group takes under a policy.
///
/// Shared by the builder and the applied-topology predicate on purpose. The two
/// used to agree only because the predicate did not look at the size at all, and
/// a policy whose whole point is which size gets chosen cannot be confirmed by
/// reading the mappings alone.
fn mirror_logical_size(
    primary: &OutputHeadDescriptor,
    member: &OutputHeadDescriptor,
    policy: MirrorSizingPolicy,
) -> Result<sophia_protocol::Size, CandidateError> {
    match policy {
        MirrorSizingPolicy::OptimizeForPrimary => current_mode_pixel_size(primary),
        MirrorSizingPolicy::OptimizeForMember => current_mode_pixel_size(member),
        MirrorSizingPolicy::CenterUnscaled => {
            let primary_size = current_mode_pixel_size(primary)?;
            let member_size = current_mode_pixel_size(member)?;
            // Per axis, not whichever head is smaller overall: a group of a
            // 2560x1080 and a 1920x1440 head has no smaller member, and taking
            // either one whole would leave the other cropped by `clip_to_target`
            // rather than bordered. The minimum on each axis is the largest
            // rectangle both heads contain.
            Ok(sophia_protocol::Size {
                width: primary_size.width.min(member_size.width),
                height: primary_size.height.min(member_size.height),
            })
        }
    }
}

/// The mappings a mirror group's two members take under a policy.
///
/// `Exact` means the same placement in all three: take the logical size verbatim
/// and centre it. What differs is whether there is any remainder to leave, which
/// is decided by `mirror_logical_size` rather than here.
const fn mirror_member_mappings(
    policy: MirrorSizingPolicy,
) -> (OutputHeadMapping, OutputHeadMapping) {
    match policy {
        MirrorSizingPolicy::OptimizeForPrimary => {
            (OutputHeadMapping::Exact, OutputHeadMapping::Fit)
        }
        MirrorSizingPolicy::OptimizeForMember => (OutputHeadMapping::Fit, OutputHeadMapping::Exact),
        MirrorSizingPolicy::CenterUnscaled => (OutputHeadMapping::Exact, OutputHeadMapping::Exact),
    }
}

/// The pixel size of the mode this head is currently running.
fn current_mode_pixel_size(
    head: &OutputHeadDescriptor,
) -> Result<sophia_protocol::Size, CandidateError> {
    let mode = head
        .current_mode
        .ok_or(CandidateError::InvalidProofTopology(
            "proof head is not enabled with a current mode",
        ))?;
    head.modes
        .iter()
        .find(|descriptor| descriptor.mode == mode)
        .map(|descriptor| descriptor.pixel_size)
        .ok_or(CandidateError::InvalidProofTopology(
            "proof head reports a current mode it does not advertise",
        ))
}

fn head_by_label<'a>(
    heads: &[&'a OutputHeadDescriptor],
    label: &str,
) -> Result<&'a OutputHeadDescriptor, CandidateError> {
    let mut matches = heads.iter().copied().filter(|head| head.label == label);
    let head = matches.next().ok_or(CandidateError::InvalidProofTopology(
        "proof label is absent",
    ))?;
    if matches.next().is_some() {
        return Err(CandidateError::InvalidProofTopology(
            "proof label is ambiguous",
        ));
    }
    Ok(head)
}

fn group_for_head<'a>(
    snapshot: &'a OutputAuthoritySnapshot,
    head: &OutputHeadDescriptor,
) -> Result<&'a OutputLogicalGroupState, CandidateError> {
    snapshot
        .groups
        .iter()
        .find(|group| group.members.iter().any(|member| member.head == head.head))
        .ok_or(CandidateError::InvalidProofTopology(
            "proof head has no logical output",
        ))
}

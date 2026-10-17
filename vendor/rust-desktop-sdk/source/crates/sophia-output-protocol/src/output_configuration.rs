use std::collections::{BTreeMap, BTreeSet};

use crate::{DisplayHeadId, DisplayModeId, OutputId, Rect, Size};

pub const MAX_OUTPUT_AUTHORITY_HEADS: usize = 16;
pub const MAX_OUTPUT_AUTHORITY_GROUPS: usize = 16;
pub const MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD: usize = 128;
pub const MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP: usize = 4;
pub const MAX_OUTPUT_AUTHORITY_LABEL_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputTransform {
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
}

impl OutputTransform {
    pub const fn bit(self) -> u16 {
        1 << self as u16
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputTransformSet(u16);

impl OutputTransformSet {
    pub const NORMAL: Self = Self(OutputTransform::Normal.bit());
    pub const ALL: Self = Self((1 << 8) - 1);

    pub const fn from_bits(bits: u16) -> Option<Self> {
        if bits != 0 && bits & !Self::ALL.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn contains(self, transform: OutputTransform) -> bool {
        self.0 & transform.bit() != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputVrrPolicy {
    Disabled,
    Automatic,
    Always,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum OutputHeadMapping {
    #[default]
    Fit,
    Cover,
    Exact,
}

impl OutputHeadMapping {
    pub const fn reduced_name(self) -> &'static str {
        match self {
            Self::Fit => "fit",
            Self::Cover => "cover",
            Self::Exact => "exact",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputTopologyIntent {
    ValidateOnly,
    Apply,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputModeDescriptor {
    pub mode: DisplayModeId,
    pub pixel_size: Size,
    pub refresh_millihz: u32,
    pub preferred: bool,
}

impl OutputModeDescriptor {
    pub const fn is_valid(self) -> bool {
        self.mode.is_valid()
            && self.pixel_size.width > 0
            && self.pixel_size.height > 0
            && self.refresh_millihz > 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputHeadDescriptor {
    pub head: DisplayHeadId,
    pub generation: u64,
    pub label: String,
    pub connected: bool,
    pub enabled: bool,
    pub current_mode: Option<DisplayModeId>,
    pub transforms: OutputTransformSet,
    pub vrr_capable: bool,
    pub modes: Vec<OutputModeDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputGroupMember {
    pub head: DisplayHeadId,
    pub mapping: OutputHeadMapping,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputLogicalGroupState {
    pub output: OutputId,
    pub generation: u64,
    pub logical: Rect,
    pub members: Vec<OutputGroupMember>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputAuthoritySnapshot {
    pub topology_epoch: u64,
    pub primary_output: OutputId,
    pub heads: Vec<OutputHeadDescriptor>,
    pub groups: Vec<OutputLogicalGroupState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputHeadTargetProposal {
    pub head: DisplayHeadId,
    pub head_generation: u64,
    pub mode: DisplayModeId,
    pub transform: OutputTransform,
    pub vrr: OutputVrrPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputLogicalGroupProposal {
    /// A current output preserves that logical identity. `INVALID` asks Engine
    /// to mint a fresh identity when the candidate commits.
    pub output: OutputId,
    pub logical: Rect,
    pub members: Vec<OutputGroupMember>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputTopologyCandidate {
    pub base_topology_epoch: u64,
    pub intent: OutputTopologyIntent,
    pub primary_group_index: u16,
    /// Complete set of enabled physical targets. A connected head omitted here
    /// is disabled by the candidate.
    pub heads: Vec<OutputHeadTargetProposal>,
    /// Complete set of logical outputs. One member means extended output;
    /// several members mean one mirrored logical output.
    pub groups: Vec<OutputLogicalGroupProposal>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputTopologyCandidateError {
    InvalidSnapshot(&'static str),
    StaleTopology,
    Empty,
    HeadCapacityExceeded,
    GroupCapacityExceeded,
    InvalidPrimaryGroup,
    InvalidHead(DisplayHeadId),
    DuplicateHead(DisplayHeadId),
    UnknownHead(DisplayHeadId),
    DisconnectedHead(DisplayHeadId),
    StaleHead(DisplayHeadId),
    UnknownMode(DisplayHeadId),
    UnsupportedTransform(DisplayHeadId),
    UnsupportedVrr(DisplayHeadId),
    InvalidGroup(usize),
    GroupHeadCapacityExceeded(usize),
    DuplicateOutput(OutputId),
    UnknownOutput(OutputId),
    DuplicateMembership(DisplayHeadId),
    MissingTarget(DisplayHeadId),
    UngroupedTarget(DisplayHeadId),
    LogicalOverlap { first: usize, second: usize },
}

impl core::fmt::Display for OutputTopologyCandidateError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for OutputTopologyCandidateError {}

impl OutputAuthoritySnapshot {
    pub fn validate(&self) -> Result<(), OutputTopologyCandidateError> {
        if self.topology_epoch == 0 {
            return Err(OutputTopologyCandidateError::InvalidSnapshot(
                "topology epoch is invalid",
            ));
        }
        if self.heads.is_empty() || self.groups.is_empty() {
            return Err(OutputTopologyCandidateError::InvalidSnapshot(
                "snapshot is empty",
            ));
        }
        if self.heads.len() > MAX_OUTPUT_AUTHORITY_HEADS
            || self.groups.len() > MAX_OUTPUT_AUTHORITY_GROUPS
        {
            return Err(OutputTopologyCandidateError::InvalidSnapshot(
                "snapshot exceeds its capacity",
            ));
        }

        let mut heads = BTreeSet::new();
        for descriptor in &self.heads {
            if !descriptor.head.is_valid()
                || descriptor.generation == 0
                || descriptor.label.is_empty()
                || descriptor.label.len() > MAX_OUTPUT_AUTHORITY_LABEL_BYTES
                || descriptor.modes.is_empty()
                || descriptor.modes.len() > MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD
            {
                return Err(OutputTopologyCandidateError::InvalidSnapshot(
                    "head descriptor is invalid",
                ));
            }
            if !heads.insert(descriptor.head) {
                return Err(OutputTopologyCandidateError::InvalidSnapshot(
                    "head identity is duplicated",
                ));
            }
            let mut modes = BTreeSet::new();
            if descriptor
                .modes
                .iter()
                .any(|mode| !mode.is_valid() || !modes.insert(mode.mode))
            {
                return Err(OutputTopologyCandidateError::InvalidSnapshot(
                    "mode descriptor is invalid or duplicated",
                ));
            }
            if descriptor.enabled
                && (!descriptor.connected
                    || descriptor
                        .current_mode
                        .is_none_or(|mode| !modes.contains(&mode)))
            {
                return Err(OutputTopologyCandidateError::InvalidSnapshot(
                    "enabled head has no admitted current mode",
                ));
            }
        }

        let mut outputs = BTreeSet::new();
        let mut memberships = BTreeSet::new();
        for group in &self.groups {
            if !group.output.is_valid()
                || group.generation == 0
                || !valid_logical(group.logical)
                || group.members.is_empty()
                || group.members.len() > MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP
                || !outputs.insert(group.output)
            {
                return Err(OutputTopologyCandidateError::InvalidSnapshot(
                    "logical group is invalid",
                ));
            }
            for member in &group.members {
                if !heads.contains(&member.head) || !memberships.insert(member.head) {
                    return Err(OutputTopologyCandidateError::InvalidSnapshot(
                        "group membership is invalid",
                    ));
                }
            }
        }
        if !outputs.contains(&self.primary_output) {
            return Err(OutputTopologyCandidateError::InvalidSnapshot(
                "primary output is absent",
            ));
        }
        for head in self.heads.iter().filter(|head| head.enabled) {
            if !memberships.contains(&head.head) {
                return Err(OutputTopologyCandidateError::InvalidSnapshot(
                    "enabled head is not grouped",
                ));
            }
        }
        Ok(())
    }
}

impl OutputTopologyCandidate {
    pub fn validate_against(
        &self,
        snapshot: &OutputAuthoritySnapshot,
    ) -> Result<(), OutputTopologyCandidateError> {
        snapshot.validate()?;
        if self.base_topology_epoch != snapshot.topology_epoch {
            return Err(OutputTopologyCandidateError::StaleTopology);
        }
        if self.heads.is_empty() || self.groups.is_empty() {
            return Err(OutputTopologyCandidateError::Empty);
        }
        if self.heads.len() > MAX_OUTPUT_AUTHORITY_HEADS {
            return Err(OutputTopologyCandidateError::HeadCapacityExceeded);
        }
        if self.groups.len() > MAX_OUTPUT_AUTHORITY_GROUPS {
            return Err(OutputTopologyCandidateError::GroupCapacityExceeded);
        }
        if usize::from(self.primary_group_index) >= self.groups.len() {
            return Err(OutputTopologyCandidateError::InvalidPrimaryGroup);
        }

        let known_heads = snapshot
            .heads
            .iter()
            .map(|head| (head.head, head))
            .collect::<BTreeMap<_, _>>();
        let mut targets = BTreeMap::new();
        for target in &self.heads {
            if !target.head.is_valid() {
                return Err(OutputTopologyCandidateError::InvalidHead(target.head));
            }
            if targets.insert(target.head, target).is_some() {
                return Err(OutputTopologyCandidateError::DuplicateHead(target.head));
            }
            let descriptor = known_heads
                .get(&target.head)
                .ok_or(OutputTopologyCandidateError::UnknownHead(target.head))?;
            if !descriptor.connected {
                return Err(OutputTopologyCandidateError::DisconnectedHead(target.head));
            }
            if descriptor.generation != target.head_generation {
                return Err(OutputTopologyCandidateError::StaleHead(target.head));
            }
            if !descriptor.modes.iter().any(|mode| mode.mode == target.mode) {
                return Err(OutputTopologyCandidateError::UnknownMode(target.head));
            }
            if !descriptor.transforms.contains(target.transform) {
                return Err(OutputTopologyCandidateError::UnsupportedTransform(
                    target.head,
                ));
            }
            if target.vrr != OutputVrrPolicy::Disabled && !descriptor.vrr_capable {
                return Err(OutputTopologyCandidateError::UnsupportedVrr(target.head));
            }
        }

        let known_outputs = snapshot
            .groups
            .iter()
            .map(|group| group.output)
            .collect::<BTreeSet<_>>();
        let mut outputs = BTreeSet::new();
        let mut memberships = BTreeSet::new();
        for (index, group) in self.groups.iter().enumerate() {
            if !valid_logical(group.logical) || group.members.is_empty() {
                return Err(OutputTopologyCandidateError::InvalidGroup(index));
            }
            if group.members.len() > MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP {
                return Err(OutputTopologyCandidateError::GroupHeadCapacityExceeded(
                    index,
                ));
            }
            if group.output.is_valid() {
                if !known_outputs.contains(&group.output) {
                    return Err(OutputTopologyCandidateError::UnknownOutput(group.output));
                }
                if !outputs.insert(group.output) {
                    return Err(OutputTopologyCandidateError::DuplicateOutput(group.output));
                }
            }
            for member in &group.members {
                if !memberships.insert(member.head) {
                    return Err(OutputTopologyCandidateError::DuplicateMembership(
                        member.head,
                    ));
                }
                if !targets.contains_key(&member.head) {
                    return Err(OutputTopologyCandidateError::MissingTarget(member.head));
                }
            }
        }
        if let Some(head) = targets
            .keys()
            .find(|head| !memberships.contains(head))
            .copied()
        {
            return Err(OutputTopologyCandidateError::UngroupedTarget(head));
        }
        for first in 0..self.groups.len() {
            for second in first + 1..self.groups.len() {
                if overlaps(self.groups[first].logical, self.groups[second].logical) {
                    return Err(OutputTopologyCandidateError::LogicalOverlap { first, second });
                }
            }
        }
        Ok(())
    }
}

const fn valid_logical(rect: Rect) -> bool {
    rect.x >= 0 && rect.y >= 0 && rect.width > 0 && rect.height > 0
}

fn overlaps(first: Rect, second: Rect) -> bool {
    let Some(first_right) = first.x.checked_add(first.width) else {
        return true;
    };
    let Some(first_bottom) = first.y.checked_add(first.height) else {
        return true;
    };
    let Some(second_right) = second.x.checked_add(second.width) else {
        return true;
    };
    let Some(second_bottom) = second.y.checked_add(second.height) else {
        return true;
    };
    first.x < second_right
        && second.x < first_right
        && first.y < second_bottom
        && second.y < first_bottom
}

use super::*;

/// Candidate resources split into the ones to enable and the ones to disable.
type CandidateResourceSplit<Enabled, Disabled> = (
    Vec<LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>>,
    Vec<LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>>,
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionSemanticStartupBarrier {
    Waiting,
    Ready,
    Invalid,
}

/// Pure admission boundary for the first semantic modeset.
///
/// A prepared framebuffer is meaningful only for a required head whose worker
/// was established first. KMS may mutate only when both sets exactly cover the
/// unique required-head set.
pub fn reduce_live_production_semantic_startup_barrier(
    required: &[sophia_engine::RenderHeadId],
    workers: &BTreeSet<sophia_engine::RenderHeadId>,
    prepared: &BTreeSet<sophia_engine::RenderHeadId>,
) -> LiveProductionSemanticStartupBarrier {
    let required_set = required.iter().copied().collect::<BTreeSet<_>>();
    if required.is_empty()
        || required_set.len() != required.len()
        || required_set.iter().any(|head| !head.is_valid())
        || !workers.is_subset(&required_set)
        || !prepared.is_subset(&required_set)
        || !prepared.is_subset(workers)
    {
        return LiveProductionSemanticStartupBarrier::Invalid;
    }
    if workers == &required_set && prepared == &required_set {
        LiveProductionSemanticStartupBarrier::Ready
    } else {
        LiveProductionSemanticStartupBarrier::Waiting
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyCurrentHead {
    pub head: sophia_engine::RenderHeadId,
    pub enabled: bool,
    pub card_index: usize,
    pub output: OutputId,
    pub selection: crate::LibdrmNativePrimaryPlaneSelection,
    pub target_generation: u64,
    pub scale: u32,
    pub refresh_millihz: u32,
    pub transform: sophia_protocol::OutputTransform,
    pub mapping: sophia_protocol::OutputHeadMapping,
    pub vrr: sophia_protocol::OutputVrrPolicy,
}

impl LiveProductionNativeTopologyCurrentHead {
    pub const fn new(
        head: sophia_engine::RenderHeadId,
        card_index: usize,
        output: OutputId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        target_generation: u64,
    ) -> Self {
        Self::new_with_target(
            head,
            true,
            card_index,
            output,
            selection,
            target_generation,
            1,
            60_000,
            sophia_protocol::OutputTransform::Normal,
            sophia_protocol::OutputHeadMapping::Fit,
            sophia_protocol::OutputVrrPolicy::Disabled,
        )
    }

    pub const fn new_with_enabled(
        head: sophia_engine::RenderHeadId,
        enabled: bool,
        card_index: usize,
        output: OutputId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        target_generation: u64,
    ) -> Self {
        Self::new_with_target(
            head,
            enabled,
            card_index,
            output,
            selection,
            target_generation,
            1,
            60_000,
            sophia_protocol::OutputTransform::Normal,
            sophia_protocol::OutputHeadMapping::Fit,
            sophia_protocol::OutputVrrPolicy::Disabled,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub const fn new_with_target(
        head: sophia_engine::RenderHeadId,
        enabled: bool,
        card_index: usize,
        output: OutputId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        target_generation: u64,
        scale: u32,
        refresh_millihz: u32,
        transform: sophia_protocol::OutputTransform,
        mapping: sophia_protocol::OutputHeadMapping,
        vrr: sophia_protocol::OutputVrrPolicy,
    ) -> Self {
        Self {
            head,
            enabled,
            card_index,
            output,
            selection,
            target_generation,
            scale,
            refresh_millihz,
            transform,
            mapping,
            vrr,
        }
    }
}

/// Reduces one backend-private current head into the exact passive target that
/// Engine may plan against. Disabled heads are not render targets.
///
/// Keeping this reduction pure prevents composition from recovering a stale
/// session-global mapping or hard-coded target generation after an IPC topology
/// commit.
pub fn reduce_live_production_head_render_target(
    head: LiveProductionNativeTopologyCurrentHead,
) -> Option<sophia_engine::HeadRenderTarget> {
    head.enabled.then_some(sophia_engine::HeadRenderTarget {
        head: head.head,
        output: head.output,
        target_generation: head.target_generation,
        native_size: head.selection.size(),
        scale: head.scale,
        refresh_millihz: head.refresh_millihz,
        transform: head.transform,
        mapping: head.mapping,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyDisposition {
    Enabled {
        output: OutputId,
        selection: crate::LibdrmNativePrimaryPlaneSelection,
        scale: u32,
        refresh_millihz: u32,
        transform: sophia_protocol::OutputTransform,
        mapping: sophia_protocol::OutputHeadMapping,
        vrr: sophia_protocol::OutputVrrPolicy,
    },
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyHeadPlan {
    pub head: sophia_engine::RenderHeadId,
    pub card_index: usize,
    pub previous_output: OutputId,
    pub previous_enabled: bool,
    pub previous_selection: crate::LibdrmNativePrimaryPlaneSelection,
    pub previous_target_generation: u64,
    pub previous_scale: u32,
    pub previous_refresh_millihz: u32,
    pub previous_transform: sophia_protocol::OutputTransform,
    pub previous_mapping: sophia_protocol::OutputHeadMapping,
    pub previous_vrr: sophia_protocol::OutputVrrPolicy,
    pub candidate_target_generation: u64,
    pub disposition: LiveProductionNativeTopologyDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyPlan {
    pub primary_output: OutputId,
    pub primary_heads: BTreeMap<OutputId, sophia_engine::RenderHeadId>,
    pub outputs: Vec<sophia_engine::HeadlessOutput>,
    pub logical_viewports: Vec<crate::LiveOutputAuthorityLogicalViewport>,
    pub heads: Vec<LiveProductionNativeTopologyHeadPlan>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyApplyPhase {
    Prepared,
    Applying,
    RollingBack,
    Applied,
    RolledBack,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LiveProductionNativeTopologyCard {
    card_index: usize,
    heads: Vec<sophia_engine::RenderHeadId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyApplyTransition {
    Accepted,
    Retry,
    CardApplied {
        card_index: usize,
        heads: Vec<sophia_engine::RenderHeadId>,
    },
    Applied {
        card_index: usize,
        heads: Vec<sophia_engine::RenderHeadId>,
    },
    RollbackRequired {
        failed_card_index: usize,
    },
    CardRolledBack {
        card_index: usize,
        heads: Vec<sophia_engine::RenderHeadId>,
    },
    RolledBack {
        card_index: usize,
        heads: Vec<sophia_engine::RenderHeadId>,
    },
    FailedWithoutMutation {
        card_index: usize,
    },
    RollbackFailed {
        card_index: usize,
    },
    OutOfOrder,
    Terminal,
}

/// Orders blocking card commits and reverses the accepted prefix on failure.
///
/// KMS is atomic only within one DRM card. This reducer supplies the missing
/// userspace transaction across cards: cards apply in stable index order, and a
/// later rejection rolls the accepted prefix back in reverse order. It owns no
/// DRM handles; the live executor keeps candidate and rollback resource owners
/// beside it and consumes the card named by `current_card_index()`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyApplyCoordinator {
    cards: Vec<LiveProductionNativeTopologyCard>,
    phase: LiveProductionNativeTopologyApplyPhase,
    next_apply: usize,
    applied: usize,
    rollback_remaining: usize,
}

#[derive(Debug)]
pub enum LiveProductionNativeTopologyCandidateResource<Enabled, Disabled> {
    Enabled(Enabled),
    Disabled(Disabled),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyResourceTransition {
    Accepted,
    Ready,
    Duplicate,
    UnknownHead,
    WrongDisposition,
}

#[derive(Debug)]
pub struct LiveProductionNativeTopologyResourceRejection<Owner> {
    pub transition: LiveProductionNativeTopologyResourceTransition,
    pub owner: Owner,
}

/// Affine prepare-all owner for a topology transaction.
///
/// Every affected head needs a candidate resource (an enabled framebuffer or
/// explicit disabled-head property set) and an enabled rollback framebuffer.
/// `ready()` becomes true only after both complete sets exist. This is the
/// safety boundary that prevents the cross-card coordinator from beginning an
/// irreversible prefix with no resource capable of restoring it.
#[derive(Debug)]
pub struct LiveProductionNativeTopologyResourceCohort<Enabled, Disabled> {
    expected: BTreeMap<
        sophia_engine::RenderHeadId,
        (usize, LiveProductionNativeTopologyDisposition, bool),
    >,
    candidate: BTreeMap<
        sophia_engine::RenderHeadId,
        LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>,
    >,
    rollback: BTreeMap<
        sophia_engine::RenderHeadId,
        LiveProductionNativeTopologyCandidateResource<Enabled, Disabled>,
    >,
}

type LiveProductionPreparedTopologyHead =
    crate::LivePreparedRenderedTopologyHead<crate::NativeGbmRenderedScanoutOwner>;

type LiveProductionNativeTopologyResources = LiveProductionNativeTopologyResourceCohort<
    LiveProductionPreparedTopologyHead,
    crate::LibdrmNativePreparedDisabledTopologyHead,
>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyPreparationPhase {
    PreparingCandidate,
    PreparingRollback,
    Prepared,
    Applying,
    RollingBack,
    Applied,
    CandidateInstalled,
    FirstFramesQueued,
    RolledBack,
    Aborting,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LiveProductionNativeTopologyPreparationReport {
    pub phase: LiveProductionNativeTopologyPreparationPhase,
    pub candidate_prepared: usize,
    pub rollback_prepared: usize,
    pub affected_heads: usize,
}

#[derive(Debug)]
pub(super) struct LiveProductionNativeTopologyPreparation {
    plan: LiveProductionNativeTopologyPlan,
    rollback: crate::LiveResolvedOutputTopology,
    resources: LiveProductionNativeTopologyResources,
    rollback_frames:
        BTreeMap<sophia_engine::RenderHeadId, crate::LiveProductionHeadCompositionFrame>,
    apply: LiveProductionNativeTopologyApplyCoordinator,
    phase: LiveProductionNativeTopologyPreparationPhase,
    failure: Option<String>,
}

struct LiveProductionNativeInstalledHead {
    index: usize,
    enabled: bool,
    output: OutputId,
    selection: crate::LibdrmNativePrimaryPlaneSelection,
    target_generation: u64,
    scale: u32,
    refresh_millihz: u32,
    transform: sophia_protocol::OutputTransform,
    mapping: sophia_protocol::OutputHeadMapping,
    vrr: sophia_protocol::OutputVrrPolicy,
    output_frames: OutputFramePresentationState,
}

include!("topology/resource_cohort.rs");

include!("topology/apply_coordinator.rs");

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveProductionNativeTopologyPlanError {
    Empty,
    DuplicateCurrentHead(sophia_engine::RenderHeadId),
    DuplicateCandidateHead(sophia_engine::RenderHeadId),
    MissingCurrentHead(sophia_engine::RenderHeadId),
    MissingCandidateHead(sophia_engine::RenderHeadId),
    InvalidOutput(OutputId),
    InvalidPrimaryHead(OutputId),
    InvalidGeneration(sophia_engine::RenderHeadId),
    ModeUnavailable(sophia_engine::RenderHeadId),
    PublishedSnapshotMismatch,
    Native(String),
}

impl core::fmt::Display for LiveProductionNativeTopologyPlanError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for LiveProductionNativeTopologyPlanError {}

include!("topology/planning.rs");

include!("topology/semantic_startup.rs");
include!("topology/publication.rs");
include!("topology/preparation_status.rs");
include!("topology/preparation.rs");
include!("topology/apply.rs");
include!("topology/installation.rs");
include!("topology/resource_preparation.rs");

fn topology_vrr_enabled(policy: sophia_protocol::OutputVrrPolicy) -> Option<bool> {
    match policy {
        sophia_protocol::OutputVrrPolicy::Disabled => Some(false),
        sophia_protocol::OutputVrrPolicy::Automatic => None,
        sophia_protocol::OutputVrrPolicy::Always => Some(true),
    }
}

fn topology_card_changes(
    state: &LiveProductionNativeTopologyPreparation,
    card_index: usize,
    rollback: bool,
) -> Result<Vec<crate::LibdrmNativeAtomicTopologyChange>, Box<dyn std::error::Error>> {
    let heads = state.resources.card_heads(card_index);
    if heads.is_empty() {
        return Err("topology card effect has no heads".into());
    }
    heads
        .into_iter()
        .map(|head| {
            let resource = if rollback {
                state.resources.rollback(head)
            } else {
                state.resources.candidate(head)
            }
            .ok_or("topology card effect is missing a prepared head")?;
            Ok(match resource {
                LiveProductionNativeTopologyCandidateResource::Enabled(owner) => {
                    crate::LibdrmNativeAtomicTopologyChange::Enabled(owner.atomic_head())
                }
                LiveProductionNativeTopologyCandidateResource::Disabled(owner) => {
                    crate::LibdrmNativeAtomicTopologyChange::Disabled(owner.atomic_head())
                }
            })
        })
        .collect()
}

pub fn validate_live_production_rollback_topology(
    plan: &LiveProductionNativeTopologyPlan,
    rollback: &crate::LiveResolvedOutputTopology,
) -> Result<(), Box<dyn std::error::Error>> {
    let outputs = rollback
        .outputs
        .iter()
        .map(|output| (output.id, output))
        .collect::<BTreeMap<_, _>>();
    if outputs.len() != rollback.outputs.len()
        || outputs.is_empty()
        || !outputs.contains_key(&rollback.primary_output)
    {
        return Err("rollback topology has invalid logical-output coverage".into());
    }
    let targets = rollback
        .targets
        .iter()
        .map(|target| (target.head, target))
        .collect::<BTreeMap<_, _>>();
    let disabled = rollback
        .disabled_heads
        .iter()
        .map(|head| (head.head, head))
        .collect::<BTreeMap<_, _>>();
    if targets.len() != rollback.targets.len()
        || disabled.len() != rollback.disabled_heads.len()
        || targets.keys().any(|head| disabled.contains_key(head))
        || targets.len().saturating_add(disabled.len()) != plan.heads.len()
    {
        return Err("rollback topology has invalid physical-head coverage".into());
    }
    for head in &plan.heads {
        if head.previous_enabled {
            let target = targets
                .get(&head.head)
                .ok_or("rollback topology omitted a previously enabled head")?;
            let output = outputs
                .get(&head.previous_output)
                .ok_or("rollback topology omitted a previous logical output")?;
            if target.output != head.previous_output
                || target.target_generation != head.previous_target_generation
                || target.native_size != head.previous_selection.size()
                || target.timing.width
                    != u32::try_from(target.native_size.width).unwrap_or_default()
                || target.timing.height
                    != u32::try_from(target.native_size.height).unwrap_or_default()
                || target.timing.refresh_millihz != head.previous_refresh_millihz
                || output.scale != head.previous_scale
                || target.transform != head.previous_transform
                || target.mapping != head.previous_mapping
                || target.vrr != head.previous_vrr
            {
                return Err("rollback topology changed previous enabled-head state".into());
            }
        } else {
            let previous = disabled
                .get(&head.head)
                .ok_or("rollback topology omitted a previously disabled head")?;
            if previous.target_generation != head.previous_target_generation {
                return Err("rollback topology changed previous disabled-head generation".into());
            }
        }
    }
    Ok(())
}

pub fn validate_live_production_topology_frames(
    plan: &LiveProductionNativeTopologyPlan,
    frames: Vec<crate::LiveProductionHeadCompositionFrame>,
    candidate: bool,
) -> Result<
    BTreeMap<sophia_engine::RenderHeadId, crate::LiveProductionHeadCompositionFrame>,
    Box<dyn std::error::Error>,
> {
    let mut by_head = BTreeMap::new();
    for frame in frames {
        let head = frame.head;
        if by_head.insert(head, frame).is_some() {
            return Err("topology composition repeats a physical head".into());
        }
    }
    let expected = plan
        .heads
        .iter()
        .filter(|head| {
            if candidate {
                matches!(
                    head.disposition,
                    LiveProductionNativeTopologyDisposition::Enabled { .. }
                )
            } else {
                head.previous_enabled
            }
        })
        .collect::<Vec<_>>();
    if by_head.len() != expected.len() {
        return Err("topology composition has incomplete physical-head coverage".into());
    }
    let scene_generation = by_head
        .values()
        .next()
        .map(|frame| frame.scene_generation)
        .filter(|generation| *generation != 0)
        .ok_or("topology composition has an invalid scene generation")?;
    if by_head
        .values()
        .any(|frame| frame.scene_generation != scene_generation)
    {
        return Err("topology composition frames disagree on scene generation".into());
    }
    for head in expected {
        let frame = by_head
            .get(&head.head)
            .ok_or("topology composition omitted a physical head")?;
        let (output, size, scale, target_generation, mapping) = if candidate {
            let LiveProductionNativeTopologyDisposition::Enabled {
                output,
                selection,
                scale,
                mapping,
                ..
            } = head.disposition
            else {
                unreachable!("candidate expected set excludes disabled heads");
            };
            (
                output,
                selection.size(),
                scale,
                head.candidate_target_generation,
                mapping,
            )
        } else {
            (
                head.previous_output,
                head.previous_selection.size(),
                head.previous_scale,
                head.previous_target_generation,
                head.previous_mapping,
            )
        };
        let damage = frame
            .frame
            .output_damage_snapshot
            .as_ref()
            .ok_or("topology composition frame has no damage snapshot")?;
        if damage.output
            != (sophia_engine::HeadlessOutput {
                id: output,
                size,
                scale,
            })
        {
            return Err("topology composition frame targets the wrong output extent".into());
        }
        if frame.target_generation != target_generation {
            return Err("topology composition frame targets a stale generation".into());
        }
        if frame.mapping != mapping {
            return Err("topology composition frame targets the wrong mapping".into());
        }
    }
    Ok(by_head)
}

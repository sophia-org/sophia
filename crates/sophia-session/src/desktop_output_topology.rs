use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use sophia_backend_live::LibdrmNativeOutputCapability;
use sophia_config::{
    ConfigDigest, ConfigGeneration, DesktopOutputReconcileError, DesktopOutputReconciliation,
    DesktopOutputScaleCapabilities, DesktopOutputState, DesktopOutputTiming,
    DesktopOutputTopologyConnector, DesktopOutputTopologySnapshot, DesktopOutputTransform,
    DesktopOutputTransformSet, DesktopOutputVrrMode, validate_desktop_output_reconciliation,
    validate_desktop_output_topology_snapshot,
};
use sophia_engine::HeadlessOutput;
use sophia_protocol::{
    DisplayHeadId, OutputAuthoritySnapshot, OutputGroupMember, OutputHeadMapping,
    OutputHeadTargetProposal, OutputId, OutputLogicalGroupProposal, OutputTopologyCandidate,
    OutputTopologyCandidateError, OutputTopologyIntent, OutputTransform, OutputVrrPolicy, Rect,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOutputActivationTarget {
    output: OutputId,
    rollback: DesktopOutputState,
    requested: DesktopOutputState,
}

impl NativeOutputActivationTarget {
    pub const fn output(&self) -> OutputId {
        self.output
    }

    pub const fn rollback(&self) -> &DesktopOutputState {
        &self.rollback
    }

    pub const fn requested(&self) -> &DesktopOutputState {
        &self.requested
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeOutputActivationPlan {
    generation: ConfigGeneration,
    digest: ConfigDigest,
    targets: Vec<NativeOutputActivationTarget>,
    focused_output: Option<OutputId>,
}

impl NativeOutputActivationPlan {
    pub const fn generation(&self) -> ConfigGeneration {
        self.generation
    }

    pub const fn digest(&self) -> ConfigDigest {
        self.digest
    }

    pub fn targets(&self) -> &[NativeOutputActivationTarget] {
        &self.targets
    }

    pub const fn focused_output(&self) -> Option<OutputId> {
        self.focused_output
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeOutputTopologyProjectionError {
    Empty,
    DuplicateOutput(u64),
    MissingCapability(u64),
    UnexpectedCapability(u64),
    PixelSizeMismatch(u64),
    ScaleUnsupported(u64),
    PositionExhausted,
    InvalidTopology(DesktopOutputReconcileError),
}

impl fmt::Display for NativeOutputTopologyProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("native output topology is empty"),
            Self::DuplicateOutput(output) => {
                write!(formatter, "native output {output} is duplicated")
            }
            Self::MissingCapability(output) => {
                write!(formatter, "native output {output} has no DRM capability")
            }
            Self::UnexpectedCapability(output) => {
                write!(formatter, "DRM capability {output} has no Engine output")
            }
            Self::PixelSizeMismatch(output) => {
                write!(
                    formatter,
                    "native output {output} disagrees with its selected mode"
                )
            }
            Self::ScaleUnsupported(output) => {
                write!(
                    formatter,
                    "native output {output} scale is outside supported bounds"
                )
            }
            Self::PositionExhausted => {
                formatter.write_str("native output logical position exhausted")
            }
            Self::InvalidTopology(error) => error.fmt(formatter),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeOutputActivationPlanError {
    InvalidOutput(u64),
    DuplicateConnector(String),
    MissingCapability(String),
    UnexpectedCapability(String),
    CapabilityDrift(String),
    InvalidReconciliation(DesktopOutputReconcileError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeOutputAuthorityCandidateError {
    InvalidSnapshot(OutputTopologyCandidateError),
    MissingCapability(String),
    MissingOpaqueHead(String),
    MissingSnapshotHead(u64),
    MissingMode(String),
    MissingMirrorPrimary(String),
    InvalidLogicalGeometry(String),
    InvalidCandidate(OutputTopologyCandidateError),
}

impl fmt::Display for NativeOutputAuthorityCandidateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSnapshot(error) => write!(formatter, "invalid output snapshot: {error}"),
            Self::MissingCapability(connector) => {
                write!(
                    formatter,
                    "native connector {connector:?} has no DRM capability"
                )
            }
            Self::MissingOpaqueHead(connector) => {
                write!(
                    formatter,
                    "native connector {connector:?} has no opaque head"
                )
            }
            Self::MissingSnapshotHead(head) => {
                write!(
                    formatter,
                    "native head {head} is absent from the output snapshot"
                )
            }
            Self::MissingMode(connector) => {
                write!(
                    formatter,
                    "native connector {connector:?} has no requested output mode"
                )
            }
            Self::MissingMirrorPrimary(connector) => {
                write!(
                    formatter,
                    "mirror member {connector:?} has no enabled primary"
                )
            }
            Self::InvalidLogicalGeometry(connector) => {
                write!(
                    formatter,
                    "native connector {connector:?} has invalid logical geometry"
                )
            }
            Self::InvalidCandidate(error) => write!(formatter, "invalid output candidate: {error}"),
        }
    }
}

impl fmt::Display for NativeOutputActivationPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOutput(output) => write!(formatter, "native output {output} is invalid"),
            Self::DuplicateConnector(connector) => {
                write!(formatter, "native connector {connector:?} is duplicated")
            }
            Self::MissingCapability(connector) => {
                write!(
                    formatter,
                    "native connector {connector:?} has no DRM capability"
                )
            }
            Self::UnexpectedCapability(connector) => {
                write!(
                    formatter,
                    "DRM connector {connector:?} has no admitted output"
                )
            }
            Self::CapabilityDrift(connector) => {
                write!(
                    formatter,
                    "native connector {connector:?} changed after topology projection"
                )
            }
            Self::InvalidReconciliation(error) => error.fmt(formatter),
        }
    }
}

pub fn prepare_native_output_activation_plan(
    capabilities: &[LibdrmNativeOutputCapability],
    topology: &DesktopOutputTopologySnapshot,
    reconciliation: &DesktopOutputReconciliation,
) -> Result<NativeOutputActivationPlan, NativeOutputActivationPlanError> {
    validate_desktop_output_reconciliation(reconciliation, topology)
        .map_err(NativeOutputActivationPlanError::InvalidReconciliation)?;

    let mut capabilities_by_connector = BTreeMap::new();
    for capability in capabilities {
        let output = capability.output().raw();
        if output == 0 {
            return Err(NativeOutputActivationPlanError::InvalidOutput(output));
        }
        // Capabilities sharing an output are a mirror group, which is admitted:
        // one logical output backed by several connectors. What must stay unique
        // is the connector below -- a cable drives one head, and two capabilities
        // naming one would make that head's state ambiguous.
        let connector = capability.connector_key();
        if capabilities_by_connector
            .insert(connector, capability)
            .is_some()
        {
            return Err(NativeOutputActivationPlanError::DuplicateConnector(
                connector.to_owned(),
            ));
        }
    }
    let mut requested_by_connector = reconciliation
        .outputs
        .iter()
        .map(|output| (output.connector.as_str(), output))
        .collect::<BTreeMap<_, _>>();
    let mut targets = Vec::with_capacity(topology.connectors.len());
    for connector in &topology.connectors {
        let capability = capabilities_by_connector
            .remove(connector.connector.as_str())
            .ok_or_else(|| {
                NativeOutputActivationPlanError::MissingCapability(connector.connector.clone())
            })?;
        if !capability_matches_topology(capability, connector) {
            return Err(NativeOutputActivationPlanError::CapabilityDrift(
                connector.connector.clone(),
            ));
        }
        let requested = requested_by_connector
            .remove(connector.connector.as_str())
            .ok_or_else(|| invalid_reconciliation("output is absent from reconciliation"))?;
        targets.push(NativeOutputActivationTarget {
            output: capability.output(),
            rollback: connector.current.clone(),
            requested: requested.clone(),
        });
    }
    if let Some(connector) = capabilities_by_connector.keys().next() {
        return Err(NativeOutputActivationPlanError::UnexpectedCapability(
            (*connector).to_owned(),
        ));
    }
    let focused_output = reconciliation
        .focused_connector
        .as_deref()
        .map(|focused| {
            targets
                .iter()
                .find(|target| target.requested().connector == focused)
                .map(NativeOutputActivationTarget::output)
                .ok_or_else(|| invalid_reconciliation("focused output is absent from plan"))
        })
        .transpose()?;
    Ok(NativeOutputActivationPlan {
        generation: reconciliation.generation,
        digest: reconciliation.digest,
        targets,
        focused_output,
    })
}

/// Projects a validated desktop-profile plan into the same complete candidate
/// consumed by the live output authority.
///
/// Startup uses this only after topology-only kernel validation succeeds. The
/// result contains no framebuffer or KMS handle: the live session owner later
/// composes candidate-sized frames from committed scene state and resolves the
/// physical resources through its ordinary prepare/apply/rollback transaction.
pub fn prepare_native_output_authority_candidate(
    plan: &NativeOutputActivationPlan,
    capabilities: &[LibdrmNativeOutputCapability],
    snapshot: &OutputAuthoritySnapshot,
    mapping: OutputHeadMapping,
) -> Result<OutputTopologyCandidate, NativeOutputAuthorityCandidateError> {
    snapshot
        .validate()
        .map_err(NativeOutputAuthorityCandidateError::InvalidSnapshot)?;

    let capabilities = capabilities
        .iter()
        .map(|capability| (capability.connector_key(), capability))
        .collect::<BTreeMap<_, _>>();
    let snapshot_heads = snapshot
        .heads
        .iter()
        .map(|head| (head.head, head))
        .collect::<BTreeMap<_, _>>();

    struct EnabledTarget<'a> {
        state: &'a DesktopOutputState,
        output: OutputId,
        head: DisplayHeadId,
        proposal: OutputHeadTargetProposal,
    }

    let mut enabled = Vec::new();
    for target in plan
        .targets()
        .iter()
        .filter(|target| target.requested().enabled)
    {
        let state = target.requested();
        let capability = capabilities
            .get(state.connector.as_str())
            .copied()
            .ok_or_else(|| {
                NativeOutputAuthorityCandidateError::MissingCapability(state.connector.clone())
            })?;
        let head = capability.head().ok_or_else(|| {
            NativeOutputAuthorityCandidateError::MissingOpaqueHead(state.connector.clone())
        })?;
        let head = DisplayHeadId::from_raw(head.raw());
        let descriptor = snapshot_heads.get(&head).copied().ok_or(
            NativeOutputAuthorityCandidateError::MissingSnapshotHead(head.raw()),
        )?;
        let mode = descriptor
            .modes
            .iter()
            .find(|mode| {
                u32::try_from(mode.pixel_size.width).ok() == Some(state.mode.width)
                    && u32::try_from(mode.pixel_size.height).ok() == Some(state.mode.height)
                    && mode.refresh_millihz == state.mode.refresh_millihz
            })
            .map(|mode| mode.mode)
            .ok_or_else(|| {
                NativeOutputAuthorityCandidateError::MissingMode(state.connector.clone())
            })?;
        enabled.push(EnabledTarget {
            state,
            output: capability.output(),
            head,
            proposal: OutputHeadTargetProposal {
                head,
                head_generation: descriptor.generation,
                mode,
                transform: protocol_transform(state.transform),
                vrr: protocol_vrr(state.vrr),
            },
        });
    }

    let mut groups = Vec::new();
    let mut group_outputs = Vec::new();
    for primary in enabled
        .iter()
        .filter(|target| target.state.mirror_of.is_none())
    {
        let mut members = enabled
            .iter()
            .filter(|target| {
                target
                    .state
                    .mirror_of
                    .as_deref()
                    .unwrap_or(target.state.connector.as_str())
                    == primary.state.connector
            })
            .collect::<Vec<_>>();
        members.sort_by_key(|target| target.head);
        if let Some(index) = members
            .iter()
            .position(|target| target.head == primary.head)
        {
            members.swap(0, index);
        }
        let logical = logical_rect(primary.state)?;
        groups.push(OutputLogicalGroupProposal {
            output: primary.output,
            logical,
            members: members
                .into_iter()
                .map(|target| OutputGroupMember {
                    head: target.head,
                    mapping,
                })
                .collect(),
        });
        group_outputs.push(primary.output);
    }
    if let Some(orphan) = enabled.iter().find(|target| {
        target.state.mirror_of.as_ref().is_some_and(|primary| {
            !enabled.iter().any(|candidate| {
                candidate.state.mirror_of.is_none() && candidate.state.connector == *primary
            })
        })
    }) {
        return Err(NativeOutputAuthorityCandidateError::MissingMirrorPrimary(
            orphan.state.connector.clone(),
        ));
    }

    normalize_logical_origin(&mut groups)?;
    let primary_output = plan
        .focused_output()
        .filter(|output| group_outputs.contains(output))
        .or_else(|| {
            group_outputs
                .contains(&snapshot.primary_output)
                .then_some(snapshot.primary_output)
        })
        .or_else(|| group_outputs.first().copied())
        .ok_or_else(|| {
            NativeOutputAuthorityCandidateError::InvalidLogicalGeometry("none".into())
        })?;
    let primary_group_index = group_outputs
        .iter()
        .position(|output| *output == primary_output)
        .and_then(|index| u16::try_from(index).ok())
        .ok_or_else(|| {
            NativeOutputAuthorityCandidateError::InvalidLogicalGeometry("primary".into())
        })?;
    let candidate = OutputTopologyCandidate {
        base_topology_epoch: snapshot.topology_epoch,
        intent: OutputTopologyIntent::Apply,
        primary_group_index,
        heads: enabled.into_iter().map(|target| target.proposal).collect(),
        groups,
    };
    candidate
        .validate_against(snapshot)
        .map_err(NativeOutputAuthorityCandidateError::InvalidCandidate)?;
    Ok(candidate)
}

fn logical_rect(state: &DesktopOutputState) -> Result<Rect, NativeOutputAuthorityCandidateError> {
    let (width, height) = match state.transform {
        DesktopOutputTransform::Rotate90
        | DesktopOutputTransform::Rotate270
        | DesktopOutputTransform::Flipped90
        | DesktopOutputTransform::Flipped270 => (state.mode.height, state.mode.width),
        DesktopOutputTransform::Normal
        | DesktopOutputTransform::Rotate180
        | DesktopOutputTransform::Flipped
        | DesktopOutputTransform::Flipped180 => (state.mode.width, state.mode.height),
    };
    let logical = |pixels: u32| {
        pixels
            .checked_mul(1_000)?
            .checked_add(state.scale_milli.checked_sub(1)?)?
            .checked_div(state.scale_milli)
            .and_then(|extent| i32::try_from(extent).ok())
            .filter(|extent| *extent > 0)
    };
    Ok(Rect {
        x: state.position.0,
        y: state.position.1,
        width: logical(width).ok_or_else(|| {
            NativeOutputAuthorityCandidateError::InvalidLogicalGeometry(state.connector.clone())
        })?,
        height: logical(height).ok_or_else(|| {
            NativeOutputAuthorityCandidateError::InvalidLogicalGeometry(state.connector.clone())
        })?,
    })
}

fn normalize_logical_origin(
    groups: &mut [OutputLogicalGroupProposal],
) -> Result<(), NativeOutputAuthorityCandidateError> {
    let minimum_x = groups
        .iter()
        .map(|group| group.logical.x)
        .min()
        .unwrap_or(0)
        .min(0);
    let minimum_y = groups
        .iter()
        .map(|group| group.logical.y)
        .min()
        .unwrap_or(0)
        .min(0);
    for group in groups {
        group.logical.x = group.logical.x.checked_sub(minimum_x).ok_or_else(|| {
            NativeOutputAuthorityCandidateError::InvalidLogicalGeometry("origin".into())
        })?;
        group.logical.y = group.logical.y.checked_sub(minimum_y).ok_or_else(|| {
            NativeOutputAuthorityCandidateError::InvalidLogicalGeometry("origin".into())
        })?;
    }
    Ok(())
}

const fn protocol_transform(transform: DesktopOutputTransform) -> OutputTransform {
    match transform {
        DesktopOutputTransform::Normal => OutputTransform::Normal,
        DesktopOutputTransform::Rotate90 => OutputTransform::Rotate90,
        DesktopOutputTransform::Rotate180 => OutputTransform::Rotate180,
        DesktopOutputTransform::Rotate270 => OutputTransform::Rotate270,
        DesktopOutputTransform::Flipped => OutputTransform::Flipped,
        DesktopOutputTransform::Flipped90 => OutputTransform::Flipped90,
        DesktopOutputTransform::Flipped180 => OutputTransform::Flipped180,
        DesktopOutputTransform::Flipped270 => OutputTransform::Flipped270,
    }
}

const fn protocol_vrr(vrr: DesktopOutputVrrMode) -> OutputVrrPolicy {
    match vrr {
        DesktopOutputVrrMode::Disabled => OutputVrrPolicy::Disabled,
        DesktopOutputVrrMode::Automatic => OutputVrrPolicy::Automatic,
        DesktopOutputVrrMode::Always => OutputVrrPolicy::Always,
    }
}

impl std::error::Error for NativeOutputTopologyProjectionError {}

impl std::error::Error for NativeOutputActivationPlanError {}

impl std::error::Error for NativeOutputAuthorityCandidateError {}

pub fn project_native_output_topology(
    capabilities: &[LibdrmNativeOutputCapability],
    outputs: &[HeadlessOutput],
) -> Result<DesktopOutputTopologySnapshot, NativeOutputTopologyProjectionError> {
    if capabilities.is_empty() || outputs.is_empty() {
        return Err(NativeOutputTopologyProjectionError::Empty);
    }
    // Capabilities sharing one logical output are a mirror group: N connectors
    // driving one `SnapshotOutput`. Grouping rather than rejecting is what admits
    // mirroring at all; the members are validated against each other below.
    let mut capabilities_by_output: BTreeMap<u64, Vec<&LibdrmNativeOutputCapability>> =
        BTreeMap::new();
    for capability in capabilities {
        capabilities_by_output
            .entry(capability.output().raw())
            .or_default()
            .push(capability);
    }

    let mut seen_outputs = BTreeSet::new();
    let mut logical_x = 0i32;
    let mut connectors = Vec::with_capacity(outputs.len());
    for output in outputs {
        let output_id = output.id.raw();
        if !seen_outputs.insert(output_id) {
            return Err(NativeOutputTopologyProjectionError::DuplicateOutput(
                output_id,
            ));
        }
        let group = capabilities_by_output.remove(&output_id).ok_or(
            NativeOutputTopologyProjectionError::MissingCapability(output_id),
        )?;
        let capability = group[0];
        let selected = timing(capability.selected_mode());
        // Heads of a group need not share a mode. The logical output is sized by
        // its primary, because that is what the scene is composed at, and each
        // other head runs its own mode with the scene placed onto it.
        if u32::try_from(output.size.width).ok() != Some(selected.width)
            || u32::try_from(output.size.height).ok() != Some(selected.height)
        {
            return Err(NativeOutputTopologyProjectionError::PixelSizeMismatch(
                output_id,
            ));
        }
        let scale_milli = output
            .scale
            .checked_mul(1_000)
            .filter(|scale| (1_000..=8_000).contains(scale))
            .ok_or(NativeOutputTopologyProjectionError::ScaleUnsupported(
                output_id,
            ))?;
        let position = (logical_x, 0);
        logical_x = logical_x
            .checked_add(logical_extent(selected.width, scale_milli)?)
            .ok_or(NativeOutputTopologyProjectionError::PositionExhausted)?;
        // Each member keeps its own connector row, because the topology describes
        // hardware and the hardware is N connectors. They share one position, which
        // is what makes them one logical output rather than N side by side.
        for member in group {
            connectors.push(DesktopOutputTopologyConnector {
                connector: member.connector_key().to_owned(),
                connected: true,
                // Profiles name nominal timings. Distinct DRM modelines can
                // reduce to the same resolution and integer refresh rate.
                modes: member
                    .modes()
                    .iter()
                    .copied()
                    .map(timing)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                preferred_mode: member.preferred_mode().map(timing),
                scales: DesktopOutputScaleCapabilities {
                    minimum_milli: 1_000,
                    maximum_milli: 8_000,
                    step_milli: 1_000,
                    automatic_milli: scale_milli,
                },
                transforms: DesktopOutputTransformSet::NORMAL,
                vrr_capable: member.vrr_configurable(),
                current: DesktopOutputState {
                    connector: member.connector_key().to_owned(),
                    // This head's own mode, which is the one it is actually
                    // scanning out. Stamping the group's mode here described a
                    // state the connector could not present, and the snapshot
                    // validator said so.
                    // The projection describes hardware, not configuration. Group
                    // membership arrives from the candidate, so the topology's own
                    // view of a connector never claims one.
                    mirror_of: None,
                    enabled: true,
                    mode: timing(member.selected_mode()),
                    scale_milli,
                    position,
                    transform: DesktopOutputTransform::Normal,
                    vrr: DesktopOutputVrrMode::Disabled,
                },
            });
        }
    }
    if let Some(output) = capabilities_by_output.keys().next().copied() {
        return Err(NativeOutputTopologyProjectionError::UnexpectedCapability(
            output,
        ));
    }
    let topology = DesktopOutputTopologySnapshot { connectors };
    validate_desktop_output_topology_snapshot(&topology)
        .map_err(NativeOutputTopologyProjectionError::InvalidTopology)?;
    Ok(topology)
}

fn timing(timing: sophia_backend_live::LibdrmNativeOutputTiming) -> DesktopOutputTiming {
    DesktopOutputTiming::new(timing.width, timing.height, timing.refresh_millihz)
}

fn capability_matches_topology(
    capability: &LibdrmNativeOutputCapability,
    connector: &DesktopOutputTopologyConnector,
) -> bool {
    let capability_modes = capability
        .modes()
        .iter()
        .copied()
        .map(timing)
        .collect::<BTreeSet<_>>();
    let topology_modes = connector.modes.iter().copied().collect::<BTreeSet<_>>();
    capability.connector_key() == connector.connector
        && capability_modes == topology_modes
        && capability.preferred_mode().map(timing) == connector.preferred_mode
        && timing(capability.selected_mode()) == connector.current.mode
        && capability.vrr_configurable() == connector.vrr_capable
}

fn invalid_reconciliation(message: &str) -> NativeOutputActivationPlanError {
    NativeOutputActivationPlanError::InvalidReconciliation(
        DesktopOutputReconcileError::InvalidReconciliation(message.to_owned()),
    )
}

fn logical_extent(
    pixels: u32,
    scale_milli: u32,
) -> Result<i32, NativeOutputTopologyProjectionError> {
    let extent = u64::from(pixels)
        .checked_mul(1_000)
        .and_then(|value| value.checked_add(u64::from(scale_milli) - 1))
        .map(|value| value / u64::from(scale_milli))
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or(NativeOutputTopologyProjectionError::PositionExhausted)?;
    Ok(extent)
}

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::{
    ConfigDigest, ConfigGeneration, DESKTOP_OUTPUT_MAX_NAMED, DesktopOutputAvailability,
    DesktopOutputCandidate, DesktopOutputMode, DesktopOutputScale, DesktopOutputTransform,
    DesktopOutputVrrMode, valid_desktop_output_connector,
};

mod adaptive;
mod identity;
pub use adaptive::{
    DesktopOutputAdjustment, DesktopOutputAdjustmentReason, DesktopOutputResolution,
    resolve_desktop_output_candidate,
};

const OUTPUT_MODE_REFRESH_TOLERANCE_MILLIHZ: u32 = 500;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DesktopOutputTiming {
    pub width: u32,
    pub height: u32,
    pub refresh_millihz: u32,
}

impl DesktopOutputTiming {
    pub const fn new(width: u32, height: u32, refresh_millihz: u32) -> Self {
        Self {
            width,
            height,
            refresh_millihz,
        }
    }

    const fn valid(self) -> bool {
        self.width > 0
            && self.width <= 16_384
            && self.height > 0
            && self.height <= 16_384
            && self.refresh_millihz >= 1_000
            && self.refresh_millihz <= 1_000_000
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopOutputTransformSet(u8);

impl DesktopOutputTransformSet {
    pub const NORMAL: Self = Self(1 << 0);
    pub const ROTATE_90: Self = Self(1 << 1);
    pub const ROTATE_180: Self = Self(1 << 2);
    pub const ROTATE_270: Self = Self(1 << 3);
    pub const FLIPPED: Self = Self(1 << 4);
    pub const FLIPPED_90: Self = Self(1 << 5);
    pub const FLIPPED_180: Self = Self(1 << 6);
    pub const FLIPPED_270: Self = Self(1 << 7);
    pub const ALL: Self = Self(u8::MAX);

    pub const fn contains(self, transform: DesktopOutputTransform) -> bool {
        let required = match transform {
            DesktopOutputTransform::Normal => Self::NORMAL.0,
            DesktopOutputTransform::Rotate90 => Self::ROTATE_90.0,
            DesktopOutputTransform::Rotate180 => Self::ROTATE_180.0,
            DesktopOutputTransform::Rotate270 => Self::ROTATE_270.0,
            DesktopOutputTransform::Flipped => Self::FLIPPED.0,
            DesktopOutputTransform::Flipped90 => Self::FLIPPED_90.0,
            DesktopOutputTransform::Flipped180 => Self::FLIPPED_180.0,
            DesktopOutputTransform::Flipped270 => Self::FLIPPED_270.0,
        };
        self.0 & required != 0
    }
}

impl std::ops::BitOr for DesktopOutputTransformSet {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopOutputScaleCapabilities {
    pub minimum_milli: u32,
    pub maximum_milli: u32,
    pub step_milli: u32,
    pub automatic_milli: u32,
}

impl DesktopOutputScaleCapabilities {
    const fn supports(self, scale_milli: u32) -> bool {
        self.minimum_milli <= self.maximum_milli
            && self.step_milli > 0
            && scale_milli >= self.minimum_milli
            && scale_milli <= self.maximum_milli
            && (scale_milli - self.minimum_milli).is_multiple_of(self.step_milli)
    }

    const fn valid(self) -> bool {
        self.minimum_milli >= 250
            && self.maximum_milli <= 8_000
            && self.supports(self.automatic_milli)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopOutputState {
    pub connector: String,
    pub enabled: bool,
    pub mode: DesktopOutputTiming,
    pub scale_milli: u32,
    pub position: (i32, i32),
    pub transform: DesktopOutputTransform,
    pub vrr: DesktopOutputVrrMode,
    /// The primary of the mirror group this connector belongs to, if any.
    ///
    /// Named for its primary because policy sees one `SnapshotOutput` and no
    /// connector identity, so the group needs a single owner and the configured
    /// output is it. `None` is the ordinary desktop: this connector is its own
    /// logical output.
    pub mirror_of: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopOutputTopologyConnector {
    pub connector: String,
    pub connected: bool,
    pub modes: Vec<DesktopOutputTiming>,
    pub preferred_mode: Option<DesktopOutputTiming>,
    pub scales: DesktopOutputScaleCapabilities,
    pub transforms: DesktopOutputTransformSet,
    pub vrr_capable: bool,
    pub current: DesktopOutputState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopOutputTopologySnapshot {
    pub connectors: Vec<DesktopOutputTopologyConnector>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopOutputReconciliation {
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub outputs: Vec<DesktopOutputState>,
    pub focused_connector: Option<String>,
    /// The connector an adaptive candidate lit because nothing it names could
    /// be. `None` whenever the profile's own outputs were used.
    pub fallback_connector: Option<String>,
    /// Realized, unique workspace affinities. Absent outputs retain their
    /// configured preference in the profile, not a second live claim here.
    pub policy_keys: BTreeMap<String, u64>,
    pub adjustments: Vec<DesktopOutputAdjustment>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesktopOutputReconcileError {
    InvalidCandidate(String),
    InvalidTopology(String),
    InvalidReconciliation(String),
    UnknownConnector(String),
    AmbiguousConnector(String),
    DisconnectedConnector(String),
    PreferredModeUnavailable(String),
    ModeUnavailable(String),
    ModeAmbiguous(String),
    ScaleUnsupported(String),
    TransformUnsupported(String),
    VrrUnsupported(String),
    FocusedOutputDisabled(String),
    OutputOverlap {
        first: String,
        second: String,
    },
    /// A mirror group named a connector that another output already claims, or that
    /// is itself configured as a named output. One connector belongs to one logical
    /// output.
    MirrorConnectorClaimed {
        primary: String,
        mirrored: String,
    },
    NoEnabledOutput,
}

impl fmt::Display for DesktopOutputReconcileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCandidate(message) => {
                write!(formatter, "invalid output candidate: {message}")
            }
            Self::InvalidTopology(message) => {
                write!(formatter, "invalid output topology: {message}")
            }
            Self::MirrorConnectorClaimed { primary, mirrored } => write!(
                formatter,
                "output {primary:?} mirrors {mirrored:?}, which another output already claims"
            ),
            Self::InvalidReconciliation(message) => {
                write!(formatter, "invalid output reconciliation: {message}")
            }
            Self::UnknownConnector(connector) => {
                write!(formatter, "unknown connector {connector:?}")
            }
            Self::AmbiguousConnector(connector) => {
                write!(
                    formatter,
                    "connector {connector:?} is present on multiple GPUs; qualify its GPU"
                )
            }
            Self::DisconnectedConnector(connector) => {
                write!(formatter, "connector {connector:?} is disconnected")
            }
            Self::PreferredModeUnavailable(connector) => {
                write!(formatter, "connector {connector:?} has no preferred mode")
            }
            Self::ModeUnavailable(connector) => {
                write!(formatter, "requested mode is unavailable on {connector:?}")
            }
            Self::ModeAmbiguous(connector) => {
                write!(formatter, "requested mode is ambiguous on {connector:?}")
            }
            Self::ScaleUnsupported(connector) => {
                write!(formatter, "requested scale is unsupported on {connector:?}")
            }
            Self::TransformUnsupported(connector) => {
                write!(
                    formatter,
                    "requested transform is unsupported on {connector:?}"
                )
            }
            Self::VrrUnsupported(connector) => {
                write!(
                    formatter,
                    "requested VRR policy is unsupported on {connector:?}"
                )
            }
            Self::FocusedOutputDisabled(connector) => {
                write!(
                    formatter,
                    "startup-focused connector {connector:?} is disabled"
                )
            }
            Self::OutputOverlap { first, second } => {
                write!(
                    formatter,
                    "enabled outputs {first:?} and {second:?} overlap"
                )
            }
            Self::NoEnabledOutput => formatter.write_str("output candidate disables every output"),
        }
    }
}

impl std::error::Error for DesktopOutputReconcileError {}

pub fn reconcile_desktop_output_candidate(
    candidate: &DesktopOutputCandidate,
    topology: &DesktopOutputTopologySnapshot,
) -> Result<DesktopOutputReconciliation, DesktopOutputReconcileError> {
    match resolve_desktop_output_candidate(candidate, topology, None)? {
        DesktopOutputResolution::Active(resolved) => Ok(resolved),
        DesktopOutputResolution::Waiting { .. } => {
            Err(DesktopOutputReconcileError::NoEnabledOutput)
        }
    }
}

fn reconcile_required_outputs(
    candidate: &DesktopOutputCandidate,
    topology: &DesktopOutputTopologySnapshot,
) -> Result<DesktopOutputReconciliation, DesktopOutputReconcileError> {
    validate_candidate(candidate)?;
    validate_topology(topology)?;
    validate_mirror_against_topology(candidate, topology)?;
    let mut outputs = topology
        .connectors
        .iter()
        .map(|connector| {
            if candidate.inherit_sophia {
                connector.current.clone()
            } else {
                DesktopOutputState {
                    connector: connector.connector.clone(),
                    enabled: false,
                    mode: connector.preferred_mode.unwrap_or(connector.current.mode),
                    scale_milli: connector.scales.automatic_milli,
                    position: (0, 0),
                    transform: DesktopOutputTransform::Normal,
                    vrr: DesktopOutputVrrMode::Disabled,
                    mirror_of: None,
                }
            }
        })
        .collect::<Vec<_>>();
    let mut focused_connector = None;
    for requested in &candidate.named {
        let index = topology
            .connectors
            .iter()
            .position(|connector| connector.connector == requested.connector);
        let enabled = requested.enabled.unwrap_or(match index {
            Some(index) if candidate.inherit_sophia => outputs[index].enabled,
            _ => true,
        });
        let Some(index) = index else {
            return Err(DesktopOutputReconcileError::UnknownConnector(
                requested.connector.clone(),
            ));
        };
        let connector = &topology.connectors[index];
        let output = &mut outputs[index];
        if enabled && !connector.connected {
            return Err(DesktopOutputReconcileError::DisconnectedConnector(
                connector.connector.clone(),
            ));
        }
        output.enabled = enabled;
        if let Some(mode) = requested.mode {
            output.mode = resolve_mode(connector, mode)?;
        }
        if let Some(scale) = requested.scale {
            output.scale_milli = match scale {
                DesktopOutputScale::Automatic => connector.scales.automatic_milli,
                DesktopOutputScale::FixedMilli(scale_milli) => scale_milli,
            };
        }
        if !connector.scales.supports(output.scale_milli) {
            return Err(DesktopOutputReconcileError::ScaleUnsupported(
                connector.connector.clone(),
            ));
        }
        if let Some(position) = requested.position {
            output.position = position;
        }
        if let Some(transform) = requested.transform {
            output.transform = transform;
        }
        if !connector.transforms.contains(output.transform) {
            return Err(DesktopOutputReconcileError::TransformUnsupported(
                connector.connector.clone(),
            ));
        }
        if let Some(vrr) = requested.vrr {
            output.vrr = vrr;
        }
        if output.vrr != DesktopOutputVrrMode::Disabled && !connector.vrr_capable {
            return Err(DesktopOutputReconcileError::VrrUnsupported(
                connector.connector.clone(),
            ));
        }
        if requested.focus_at_startup == Some(true) {
            if !output.enabled {
                return Err(DesktopOutputReconcileError::FocusedOutputDisabled(
                    connector.connector.clone(),
                ));
            }
            focused_connector = Some(connector.connector.clone());
        }
    }
    apply_mirror_groups(candidate, &mut outputs)?;
    if !outputs.iter().any(|output| output.enabled) {
        return Err(DesktopOutputReconcileError::NoEnabledOutput);
    }
    reject_overlaps(&outputs)?;
    let reconciliation = DesktopOutputReconciliation {
        generation: candidate.generation,
        digest: candidate.digest,
        outputs,
        focused_connector,
        fallback_connector: None,
        policy_keys: BTreeMap::new(),
        adjustments: Vec::new(),
    };
    validate_desktop_output_reconciliation(&reconciliation, topology)?;
    Ok(reconciliation)
}

pub fn validate_desktop_output_topology_snapshot(
    topology: &DesktopOutputTopologySnapshot,
) -> Result<(), DesktopOutputReconcileError> {
    validate_topology(topology)
}

pub fn validate_desktop_output_reconciliation(
    reconciliation: &DesktopOutputReconciliation,
    topology: &DesktopOutputTopologySnapshot,
) -> Result<(), DesktopOutputReconcileError> {
    validate_topology_for_availability(topology, true)?;
    if reconciliation.generation.raw() == 0
        || reconciliation.outputs.len() != topology.connectors.len()
    {
        return Err(DesktopOutputReconcileError::InvalidReconciliation(
            "generation and output count must match the admitted topology".to_owned(),
        ));
    }
    let mut connectors = topology
        .connectors
        .iter()
        .map(|connector| (connector.connector.as_str(), connector))
        .collect::<BTreeMap<_, _>>();
    for output in &reconciliation.outputs {
        let Some(connector) = connectors.remove(output.connector.as_str()) else {
            return Err(DesktopOutputReconcileError::InvalidReconciliation(
                "output connectors must be unique and exactly cover the topology".to_owned(),
            ));
        };
        if output.enabled && !connector.connected {
            return Err(DesktopOutputReconcileError::DisconnectedConnector(
                output.connector.clone(),
            ));
        }
        // An unavailable connector carries no timing obligation until enabled.
        if !output.enabled && connector.modes.is_empty() {
            continue;
        }
        if !connector.modes.contains(&output.mode) {
            return Err(DesktopOutputReconcileError::ModeUnavailable(
                output.connector.clone(),
            ));
        }
        if !connector.scales.supports(output.scale_milli) {
            return Err(DesktopOutputReconcileError::ScaleUnsupported(
                output.connector.clone(),
            ));
        }
        if !connector.transforms.contains(output.transform) {
            return Err(DesktopOutputReconcileError::TransformUnsupported(
                output.connector.clone(),
            ));
        }
        if output.vrr != DesktopOutputVrrMode::Disabled && !connector.vrr_capable {
            return Err(DesktopOutputReconcileError::VrrUnsupported(
                output.connector.clone(),
            ));
        }
        if !(-1_000_000..=1_000_000).contains(&output.position.0)
            || !(-1_000_000..=1_000_000).contains(&output.position.1)
        {
            return Err(DesktopOutputReconcileError::InvalidReconciliation(
                "output position is outside its supported range".to_owned(),
            ));
        }
    }
    if !connectors.is_empty() {
        return Err(DesktopOutputReconcileError::InvalidReconciliation(
            "output connectors must exactly cover the topology".to_owned(),
        ));
    }
    if !reconciliation.outputs.iter().any(|output| output.enabled) {
        return Err(DesktopOutputReconcileError::NoEnabledOutput);
    }
    if let Some(focused) = reconciliation.focused_connector.as_deref() {
        let Some(output) = reconciliation
            .outputs
            .iter()
            .find(|output| output.connector == focused)
        else {
            return Err(DesktopOutputReconcileError::InvalidReconciliation(
                "focused connector is absent from the output set".to_owned(),
            ));
        };
        if !output.enabled {
            return Err(DesktopOutputReconcileError::FocusedOutputDisabled(
                focused.to_owned(),
            ));
        }
    }
    if let Some(fallback) = reconciliation.fallback_connector.as_deref() {
        // A fallback carries the session's fallback affinity, so it has to be
        // an output that is actually lit and the one the session starts on.
        if reconciliation.focused_connector.as_deref() != Some(fallback)
            || !reconciliation
                .outputs
                .iter()
                .any(|output| output.connector == fallback && output.enabled)
        {
            return Err(DesktopOutputReconcileError::InvalidReconciliation(
                "fallback connector must be the enabled, focused output".to_owned(),
            ));
        }
    }
    let mut keys = BTreeSet::new();
    for (connector, key) in &reconciliation.policy_keys {
        if *key == 0
            || !keys.insert(*key)
            || !reconciliation.outputs.iter().any(|output| {
                output.enabled && output.mirror_of.is_none() && &output.connector == connector
            })
        {
            return Err(DesktopOutputReconcileError::InvalidReconciliation(
                "policy affinities must be unique and belong to enabled logical outputs".into(),
            ));
        }
    }
    reject_overlaps(&reconciliation.outputs)
}

fn validate_candidate(
    candidate: &DesktopOutputCandidate,
) -> Result<(), DesktopOutputReconcileError> {
    if candidate.named.len() > DESKTOP_OUTPUT_MAX_NAMED {
        return Err(DesktopOutputReconcileError::InvalidCandidate(
            "named output count exceeds its bound".to_owned(),
        ));
    }
    let mut connectors = BTreeSet::new();
    let mut policy_keys = BTreeSet::new();
    let mut focused = false;
    for output in &candidate.named {
        if output
            .policy_key
            .is_some_and(|key| key == 0 || !policy_keys.insert(key))
        {
            return Err(DesktopOutputReconcileError::InvalidCandidate(
                "named policy affinities must be nonzero and unique".into(),
            ));
        }
        if !valid_desktop_output_connector(&output.connector)
            || !connectors.insert(output.connector.clone())
        {
            return Err(DesktopOutputReconcileError::InvalidCandidate(
                "connector identities must be valid and unique".to_owned(),
            ));
        }
        if output.position.is_some_and(|(x, y)| {
            !(-1_000_000..=1_000_000).contains(&x) || !(-1_000_000..=1_000_000).contains(&y)
        }) {
            return Err(DesktopOutputReconcileError::InvalidCandidate(
                "output position is outside its supported range".to_owned(),
            ));
        }
        if output.focus_at_startup == Some(true) {
            if focused {
                return Err(DesktopOutputReconcileError::InvalidCandidate(
                    "more than one output requests startup focus".to_owned(),
                ));
            }
            focused = true;
        }
    }
    validate_mirror_groups(candidate, &connectors)?;
    if candidate.fallback_policy_key.is_some_and(|key| key == 0)
        || (candidate.fallback_policy_key.is_some()
            && candidate.availability != DesktopOutputAvailability::Adaptive)
    {
        return Err(DesktopOutputReconcileError::InvalidCandidate(
            "a fallback policy key must be nonzero and belongs to an adaptive candidate".to_owned(),
        ));
    }
    Ok(())
}

/// Checks every mirror group against the rest of the candidate.
///
/// Parsing already rejected a group that names itself, repeats a connector, or
/// names none. What only the whole candidate can answer is whether two logical
/// outputs claim the same head, which is the one arrangement that would make
/// "one logical output backed by N connectors" untrue.
fn validate_mirror_groups(
    candidate: &DesktopOutputCandidate,
    named: &BTreeSet<String>,
) -> Result<(), DesktopOutputReconcileError> {
    let mut claimed = BTreeSet::new();
    for output in &candidate.named {
        for mirrored in &output.mirror {
            // A mirrored connector is driven by its group's primary, so it cannot
            // also be configured as an output in its own right.
            if named.contains(mirrored) || !claimed.insert(mirrored.clone()) {
                return Err(DesktopOutputReconcileError::MirrorConnectorClaimed {
                    primary: output.connector.clone(),
                    mirrored: mirrored.clone(),
                });
            }
        }
    }
    Ok(())
}

/// Checks each mirror group against the attached hardware, then refuses it.
///
/// The order matters. A configuration that could never work should be reported as
/// wrong even while mirroring is unimplemented, because "Sophia cannot do this yet"
/// and "this asks for something impossible" send an operator to different places.
/// So unknown connectors, disconnected ones, and mode mismatches are named first,
/// and only a request that would work once the scanout half exists gets the
/// unsupported refusal.
///
/// Every member resolves its own mode. The mirror-fit policy then names how one
/// logical scene maps to unequal native targets; silently inventing a placement
/// policy remains forbidden.
fn validate_mirror_against_topology(
    candidate: &DesktopOutputCandidate,
    topology: &DesktopOutputTopologySnapshot,
) -> Result<(), DesktopOutputReconcileError> {
    for output in &candidate.named {
        if output.mirror.is_empty() {
            continue;
        }
        let Some(primary) = topology
            .connectors
            .iter()
            .find(|connector| connector.connector == output.connector)
        else {
            return Err(DesktopOutputReconcileError::UnknownConnector(
                output.connector.clone(),
            ));
        };
        // The primary's own mode still has to resolve, because the group's scene is
        // composed at it. What no longer has to hold is that every member can
        // present it: each head runs its own mode and the scene is placed onto it,
        // so a member that cannot match is the ordinary case rather than a refusal.
        let _ = resolve_mode(primary, output.mode.unwrap_or(DesktopOutputMode::Preferred))?;

        for mirrored in &output.mirror {
            let Some(member) = topology
                .connectors
                .iter()
                .find(|connector| &connector.connector == mirrored)
            else {
                return Err(DesktopOutputReconcileError::UnknownConnector(
                    mirrored.clone(),
                ));
            };
            if !member.connected {
                return Err(DesktopOutputReconcileError::DisconnectedConnector(
                    mirrored.clone(),
                ));
            }
        }
    }
    Ok(())
}

fn validate_topology(
    topology: &DesktopOutputTopologySnapshot,
) -> Result<(), DesktopOutputReconcileError> {
    validate_topology_for_availability(topology, false)
}

fn validate_topology_for_availability(
    topology: &DesktopOutputTopologySnapshot,
    adaptive: bool,
) -> Result<(), DesktopOutputReconcileError> {
    if (!adaptive && topology.connectors.is_empty())
        || topology.connectors.len() > DESKTOP_OUTPUT_MAX_NAMED
    {
        return Err(DesktopOutputReconcileError::InvalidTopology(
            "connector count is outside its supported range".to_owned(),
        ));
    }
    let mut names = BTreeSet::new();
    for connector in &topology.connectors {
        if !valid_desktop_output_connector(&connector.connector)
            || !names.insert(connector.connector.clone())
        {
            return Err(DesktopOutputReconcileError::InvalidTopology(
                "connector identities must be valid and unique".to_owned(),
            ));
        }
        if connector.current.connector != connector.connector
            || (!adaptive && connector.modes.is_empty())
            || connector.modes.len() > 256
            || !connector.scales.valid()
            || !connector
                .transforms
                .contains(DesktopOutputTransform::Normal)
        {
            return Err(DesktopOutputReconcileError::InvalidTopology(format!(
                "connector {:?} has inconsistent capabilities",
                connector.connector
            )));
        }
        let mut modes = BTreeSet::new();
        if connector
            .modes
            .iter()
            .any(|mode| !mode.valid() || !modes.insert(*mode))
            || connector
                .preferred_mode
                .is_some_and(|mode| !modes.contains(&mode))
            || (!connector.modes.is_empty() && !modes.contains(&connector.current.mode))
            || (connector.modes.is_empty() && connector.current.enabled)
            || !connector.scales.supports(connector.current.scale_milli)
            || !connector.transforms.contains(connector.current.transform)
            || !(-1_000_000..=1_000_000).contains(&connector.current.position.0)
            || !(-1_000_000..=1_000_000).contains(&connector.current.position.1)
            || (connector.current.vrr != DesktopOutputVrrMode::Disabled && !connector.vrr_capable)
            || (connector.current.enabled && !connector.connected)
        {
            return Err(DesktopOutputReconcileError::InvalidTopology(format!(
                "connector {:?} has an invalid current state",
                connector.connector
            )));
        }
    }
    Ok(())
}

fn resolve_mode(
    connector: &DesktopOutputTopologyConnector,
    requested: DesktopOutputMode,
) -> Result<DesktopOutputTiming, DesktopOutputReconcileError> {
    let DesktopOutputMode::Exact {
        width,
        height,
        refresh_millihz,
    } = requested
    else {
        return connector.preferred_mode.ok_or_else(|| {
            DesktopOutputReconcileError::PreferredModeUnavailable(connector.connector.clone())
        });
    };
    let mut matches = connector
        .modes
        .iter()
        .copied()
        .filter(|mode| {
            mode.width == width
                && mode.height == height
                && mode.refresh_millihz.abs_diff(refresh_millihz)
                    <= OUTPUT_MODE_REFRESH_TOLERANCE_MILLIHZ
        })
        .collect::<Vec<_>>();
    matches.sort_by_key(|mode| mode.refresh_millihz.abs_diff(refresh_millihz));
    let Some(selected) = matches.first().copied() else {
        return Err(DesktopOutputReconcileError::ModeUnavailable(
            connector.connector.clone(),
        ));
    };
    if matches.get(1).is_some_and(|next| {
        next.refresh_millihz.abs_diff(refresh_millihz)
            == selected.refresh_millihz.abs_diff(refresh_millihz)
    }) {
        return Err(DesktopOutputReconcileError::ModeAmbiguous(
            connector.connector.clone(),
        ));
    }
    Ok(selected)
}

/// Binds each group's members to its primary and makes them agree.
///
/// The members take the primary's logical state -- position, scale, transform,
/// enabled -- because a group is one logical output and those describe the output
/// rather than the cable. They keep their own *mode*, because that describes the
/// cable, and the group's scene is placed onto each head at whatever mode it runs.
/// Taking rather than checking means the group cannot be configured into
/// disagreement about the things that must agree.
fn apply_mirror_groups(
    candidate: &DesktopOutputCandidate,
    outputs: &mut [DesktopOutputState],
) -> Result<(), DesktopOutputReconcileError> {
    for requested in &candidate.named {
        if requested.mirror.is_empty() {
            continue;
        }
        let Some(primary) = outputs
            .iter()
            .find(|output| output.connector == requested.connector)
            .cloned()
        else {
            return Err(DesktopOutputReconcileError::UnknownConnector(
                requested.connector.clone(),
            ));
        };
        for member in &requested.mirror {
            let Some(state) = outputs
                .iter_mut()
                .find(|output| &output.connector == member)
            else {
                return Err(DesktopOutputReconcileError::UnknownConnector(
                    member.clone(),
                ));
            };
            state.enabled = primary.enabled;
            // Not the mode. Each head runs its own and the group's scene is placed
            // onto it -- taking the primary's was the shared-buffer assumption, and
            // it is what made a group of mismatched panels impossible.
            state.scale_milli = primary.scale_milli;
            state.position = primary.position;
            state.transform = primary.transform;
            state.vrr = primary.vrr;
            state.mirror_of = Some(primary.connector.clone());
        }
    }
    Ok(())
}

/// The logical output a connector belongs to: its group's primary, or itself.
fn mirror_group_of(output: &DesktopOutputState) -> &str {
    output.mirror_of.as_deref().unwrap_or(&output.connector)
}

fn reject_overlaps(outputs: &[DesktopOutputState]) -> Result<(), DesktopOutputReconcileError> {
    for (index, first) in outputs
        .iter()
        .enumerate()
        .filter(|(_, output)| output.enabled)
    {
        for second in outputs
            .iter()
            .skip(index + 1)
            .filter(|output| output.enabled)
        {
            // Members of one mirror group share a position by definition -- that
            // is what makes them one logical output rather than two side by side.
            // The overlap rule is about distinct screens landing on each other.
            if mirror_group_of(first) == mirror_group_of(second) {
                continue;
            }
            if output_rect(first).overlaps(output_rect(second)) {
                return Err(DesktopOutputReconcileError::OutputOverlap {
                    first: first.connector.clone(),
                    second: second.connector.clone(),
                });
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct OutputRect {
    x: i64,
    y: i64,
    width: i64,
    height: i64,
}

impl OutputRect {
    const fn overlaps(self, other: Self) -> bool {
        self.x < other.x + other.width
            && other.x < self.x + self.width
            && self.y < other.y + other.height
            && other.y < self.y + self.height
    }
}

fn output_rect(output: &DesktopOutputState) -> OutputRect {
    let (width, height) = match output.transform {
        DesktopOutputTransform::Rotate90
        | DesktopOutputTransform::Rotate270
        | DesktopOutputTransform::Flipped90
        | DesktopOutputTransform::Flipped270 => (output.mode.height, output.mode.width),
        DesktopOutputTransform::Normal
        | DesktopOutputTransform::Rotate180
        | DesktopOutputTransform::Flipped
        | DesktopOutputTransform::Flipped180 => (output.mode.width, output.mode.height),
    };
    let logical = |pixels: u32| {
        i64::from(pixels)
            .saturating_mul(1_000)
            .saturating_add(i64::from(output.scale_milli) - 1)
            / i64::from(output.scale_milli)
    };
    OutputRect {
        x: i64::from(output.position.0),
        y: i64::from(output.position.1),
        width: logical(width),
        height: logical(height),
    }
}

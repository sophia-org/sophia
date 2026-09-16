use core::mem::size_of;

use crate::{
    LayoutNodeCapabilities, OutputId, PolicyActionRegistration, PolicyConfiguration,
    PolicyDirtyRequest, PolicyInteractionAxis, PolicyInteractionKind, PolicyInteractionPhase,
    PolicyOutputProjection, PolicyOutputSnapshot, PolicyPresentationState,
    PolicyProjectionIndicator, PolicyProjectionOutcome, PolicyProjectionOutputStatus,
    PolicyProjectionProposal, PolicyRequestCause, PolicySceneSnapshot, PolicySessionOperation,
    PolicySessionOperationOutcome, PolicySessionOperationRequest, PolicySurfaceClassification,
    PolicySurfaceKind, PolicySurfacePlacement, PolicySurfaceSnapshot, PolicyTransform, Rect,
    SOPHIA_WM_CAPABILITY_ACTIONS, SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT,
    SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS, Size, SurfaceConstraints, SurfaceId, TransactionId,
    WmActionId, WmChromePolicy, WmFocusRingStyle, WmFrameStyle, WmRgb8,
    valid_policy_interaction_payload,
};

use super::{
    IpcCodecError, PROJECTION_INDICATOR_RECORD_KIND, PROJECTION_OUTPUT_RECORD_KIND,
    PROJECTION_OUTPUT_STATUS_RECORD_KIND, PROJECTION_PLACEMENT_RECORD_KIND,
    SNAPSHOT_ACTION_RECORD_KIND, SNAPSHOT_OUTPUT_RECORD_KIND,
    SNAPSHOT_SESSION_OPERATION_RECORD_KIND, SNAPSHOT_SURFACE_RECORD_KIND,
    SOPHIA_WM_OUTCOME_COMMITTED, SOPHIA_WM_OUTCOME_DISCONNECTED,
    SOPHIA_WM_OUTCOME_REJECTED_INVALID, SOPHIA_WM_OUTCOME_REJECTED_STALE,
    SOPHIA_WM_OUTCOME_TIMED_OUT, WmV1PolicyConfiguration, WmV1PolicyDirty, WmV1ProjectionBegin,
    WmV1ProjectionChunk, WmV1ProjectionEnd, WmV1ProjectionIndicatorRecord, WmV1ProjectionOutcome,
    WmV1ProjectionOutputRecord, WmV1ProjectionOutputStatusRecord, WmV1ProjectionPlacementRecord,
    WmV1ProjectionRequest, WmV1SessionOperationOutcome, WmV1SessionOperationRequest,
    WmV1SnapshotActionRecord, WmV1SnapshotBegin, WmV1SnapshotChunk, WmV1SnapshotEnd,
    WmV1SnapshotOutputRecord, WmV1SnapshotSessionOperationRecord, WmV1SnapshotSurfaceRecord,
    decode_wm_v1_projection_indicator_records, decode_wm_v1_projection_output_records,
    decode_wm_v1_projection_output_status_records, decode_wm_v1_projection_placement_records,
    decode_wm_v1_snapshot_action_records, decode_wm_v1_snapshot_output_records,
    decode_wm_v1_snapshot_session_operation_records, decode_wm_v1_snapshot_surface_records,
    encode_wm_v1_projection_indicator_records, encode_wm_v1_projection_output_records,
    encode_wm_v1_projection_output_status_records, encode_wm_v1_projection_placement_records,
    encode_wm_v1_snapshot_action_records, encode_wm_v1_snapshot_output_records,
    encode_wm_v1_snapshot_session_operation_records, encode_wm_v1_snapshot_surface_records,
};

const OUTPUT_ID_WIRE_SIZE: usize = size_of::<u64>();

/// First capability-gated snapshot extension. It deliberately lives outside
/// the generated ordinary-record range; see the forward-compatibility rule.
pub const SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND: u16 = 0xFF00;
const SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE: usize = 16;

pub const POLICY_SURFACE_CAPABILITY_MOVABLE: u16 = 1 << 0;
pub const POLICY_SURFACE_CAPABILITY_RESIZABLE: u16 = 1 << 1;
pub const POLICY_SURFACE_CAPABILITY_FOCUSABLE: u16 = 1 << 2;
pub const POLICY_SURFACE_CAPABILITY_CLOSABLE: u16 = 1 << 3;
pub const POLICY_SURFACE_CAPABILITY_FULLSCREENABLE: u16 = 1 << 4;
const POLICY_SURFACE_CAPABILITY_SUPPORTED: u16 = POLICY_SURFACE_CAPABILITY_MOVABLE
    | POLICY_SURFACE_CAPABILITY_RESIZABLE
    | POLICY_SURFACE_CAPABILITY_FOCUSABLE
    | POLICY_SURFACE_CAPABILITY_CLOSABLE
    | POLICY_SURFACE_CAPABILITY_FULLSCREENABLE;
const POLICY_PRESENTATION_FULLSCREEN: u16 = 1 << 0;
const POLICY_PRESENTATION_MAXIMIZED: u16 = 1 << 1;
const POLICY_PRESENTATION_MINIMIZED: u16 = 1 << 2;
const POLICY_PRESENTATION_SUPPORTED: u16 =
    POLICY_PRESENTATION_FULLSCREEN | POLICY_PRESENTATION_MAXIMIZED | POLICY_PRESENTATION_MINIMIZED;
const POLICY_SESSION_OPERATION_SURFACE_TARGET: u16 = 1 << 0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmV1SnapshotTransfer {
    pub transaction: TransactionId,
    pub begin: WmV1SnapshotBegin,
    pub chunks: Vec<WmV1SnapshotChunk>,
    pub end: WmV1SnapshotEnd,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmV1DecodedSnapshot {
    pub launch_origins: Vec<crate::PolicyLaunchContext>,
    pub scene: PolicySceneSnapshot,
    pub actions: Vec<PolicyActionRegistration>,
    pub classifications: Vec<PolicySurfaceClassification>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmV1SnapshotSurfaceClassificationRecord {
    pub surface_index: u32,
    pub surface_generation: u32,
    pub classification: u64,
}

pub fn encode_wm_v1_snapshot_surface_classification_records(
    records: &[WmV1SnapshotSurfaceClassificationRecord],
) -> Result<Vec<u8>, IpcCodecError> {
    if records.len() > crate::POLICY_MAX_SURFACES {
        return Err(IpcCodecError::CountTooLarge {
            count: records.len(),
            max: crate::POLICY_MAX_SURFACES,
        });
    }
    let mut data = Vec::with_capacity(records.len() * SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE);
    for record in records {
        data.extend_from_slice(&record.surface_index.to_le_bytes());
        data.extend_from_slice(&record.surface_generation.to_le_bytes());
        data.extend_from_slice(&record.classification.to_le_bytes());
    }
    Ok(data)
}

pub fn decode_wm_v1_snapshot_surface_classification_records(
    data: &[u8],
    item_count: u32,
) -> Result<Vec<WmV1SnapshotSurfaceClassificationRecord>, IpcCodecError> {
    let count = item_count as usize;
    if count > crate::POLICY_MAX_SURFACES {
        return Err(IpcCodecError::CountTooLarge {
            count,
            max: crate::POLICY_MAX_SURFACES,
        });
    }
    let expected = count
        .checked_mul(SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE)
        .ok_or(IpcCodecError::CountTooLarge {
            count,
            max: crate::POLICY_MAX_SURFACES,
        })?;
    if data.len() < expected {
        return Err(IpcCodecError::Truncated);
    }
    if data.len() > expected {
        return Err(IpcCodecError::TrailingBytes(data.len() - expected));
    }
    let mut records = Vec::with_capacity(count);
    for record in data.chunks_exact(SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_SIZE) {
        records.push(WmV1SnapshotSurfaceClassificationRecord {
            surface_index: u32::from_le_bytes(record[0..4].try_into().expect("fixed record")),
            surface_generation: u32::from_le_bytes(record[4..8].try_into().expect("fixed record")),
            classification: u64::from_le_bytes(record[8..16].try_into().expect("fixed record")),
        });
    }
    Ok(records)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmV1ProjectionTransfer {
    pub transaction: TransactionId,
    pub begin: WmV1ProjectionBegin,
    pub chunks: Vec<WmV1ProjectionChunk>,
    pub end: WmV1ProjectionEnd,
}

pub fn encode_wm_v1_policy_projection_request(
    request: &crate::PolicyProjectionRequest,
) -> Result<WmV1ProjectionRequest, IpcCodecError> {
    if request.connection_epoch == 0
        || request.request_id == 0
        || request.scene_generation == 0
        || request.policy_generation == 0
    {
        return Err(invalid("projection_request_identity", 0));
    }
    if request.affected_outputs.is_empty()
        || request.affected_outputs.len() > crate::POLICY_MAX_OUTPUTS
    {
        return Err(IpcCodecError::CountTooLarge {
            count: request.affected_outputs.len(),
            max: crate::POLICY_MAX_OUTPUTS,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut affected_outputs =
        Vec::with_capacity(request.affected_outputs.len() * OUTPUT_ID_WIRE_SIZE);
    for output in &request.affected_outputs {
        if !output.is_valid() || !seen.insert(*output) {
            return Err(invalid("affected_output", output.raw() as u32));
        }
        affected_outputs.extend_from_slice(&output.raw().to_le_bytes());
    }
    let (
        cause_kind,
        interaction_phase,
        interaction_kind,
        interaction_axis,
        activation_serial,
        action,
        target_index,
        target_generation,
        interaction,
    ) = match request.cause {
        crate::PolicyRequestCause::OutputAction { .. } => {
            return Err(invalid("output_action_requires_separate_message", 0));
        }
        PolicyRequestCause::SceneChanged => (0, 0, 0, 0, 0, 0, 0, 0, Rect::default()),
        PolicyRequestCause::Action {
            activation_serial,
            action,
        } => {
            if activation_serial == 0 || !action.is_valid() {
                return Err(invalid("action_cause", 0));
            }
            (
                1,
                0,
                0,
                0,
                activation_serial,
                action.raw(),
                0,
                0,
                Rect::default(),
            )
        }
        PolicyRequestCause::Focus { target } => {
            if !target.is_valid() {
                return Err(invalid("focus_cause", 0));
            }
            (
                2,
                0,
                0,
                0,
                0,
                0,
                target.index(),
                target.generation(),
                Rect::default(),
            )
        }
        PolicyRequestCause::PointerFocus { output, target } => {
            if output.raw() == 0
                || !request.affected_outputs.contains(&output)
                || target.is_some_and(|target| !target.is_valid())
            {
                return Err(invalid("pointer_focus_cause", 0));
            }
            (
                4,
                0,
                0,
                0,
                0,
                output.raw(),
                target.map_or(0, |t| t.index()),
                target.map_or(0, |t| t.generation()),
                Rect::default(),
            )
        }
        PolicyRequestCause::Interaction {
            phase,
            kind,
            axis,
            target,
            geometry,
        } => {
            if !target.is_valid() || !valid_policy_interaction_payload(phase, kind, axis, geometry)
            {
                return Err(invalid("interaction_cause", 0));
            }
            (
                3,
                phase as u16,
                kind as u16,
                axis as u16,
                0,
                0,
                target.index(),
                target.generation(),
                geometry,
            )
        }
    };
    Ok(WmV1ProjectionRequest {
        connection_epoch: request.connection_epoch,
        request_id: request.request_id,
        scene_generation: request.scene_generation,
        policy_generation: request.policy_generation,
        cause_kind,
        interaction_phase,
        interaction_kind,
        interaction_axis,
        activation_serial,
        action,
        target_index,
        target_generation,
        interaction_x: interaction.x,
        interaction_y: interaction.y,
        interaction_width: interaction.width,
        interaction_height: interaction.height,
        affected_output_count: request.affected_outputs.len() as u16,
        affected_outputs,
    })
}

pub fn decode_wm_v1_policy_projection_request(
    request: &WmV1ProjectionRequest,
) -> Result<crate::PolicyProjectionRequest, IpcCodecError> {
    let count = usize::from(request.affected_output_count);
    if request.connection_epoch == 0
        || request.request_id == 0
        || request.scene_generation == 0
        || request.policy_generation == 0
    {
        return Err(invalid("projection_request_identity", 0));
    }
    if count == 0 || count > crate::POLICY_MAX_OUTPUTS {
        return Err(IpcCodecError::CountTooLarge {
            count,
            max: crate::POLICY_MAX_OUTPUTS,
        });
    }
    if request.affected_outputs.len() != count * OUTPUT_ID_WIRE_SIZE {
        return Err(invalid(
            "affected_output_bytes",
            request.affected_outputs.len() as u32,
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut affected_outputs = Vec::with_capacity(count);
    for bytes in request.affected_outputs.chunks_exact(OUTPUT_ID_WIRE_SIZE) {
        let output = OutputId::from_raw(u64::from_le_bytes(
            bytes.try_into().expect("fixed output-id chunk"),
        ));
        if !output.is_valid() || !seen.insert(output) {
            return Err(invalid("affected_output", output.raw() as u32));
        }
        affected_outputs.push(output);
    }
    let target = || {
        decode_optional_surface(
            request.target_index,
            request.target_generation,
            "request_target",
        )?
        .ok_or_else(|| invalid("request_target", 0))
    };
    let cause = match request.cause_kind {
        0 if request.interaction_phase == 0
            && request.interaction_kind == 0
            && request.interaction_axis == 0
            && request.activation_serial == 0
            && request.action == 0
            && request.target_index == 0
            && request.target_generation == 0
            && request.interaction_x == 0
            && request.interaction_y == 0
            && request.interaction_width == 0
            && request.interaction_height == 0 =>
        {
            PolicyRequestCause::SceneChanged
        }
        1 if request.interaction_phase == 0
            && request.interaction_kind == 0
            && request.interaction_axis == 0
            && request.activation_serial != 0
            && request.action != 0
            && request.target_index == 0
            && request.target_generation == 0
            && request.interaction_x == 0
            && request.interaction_y == 0
            && request.interaction_width == 0
            && request.interaction_height == 0 =>
        {
            PolicyRequestCause::Action {
                activation_serial: request.activation_serial,
                action: WmActionId::from_raw(request.action),
            }
        }
        2 if request.interaction_phase == 0
            && request.interaction_kind == 0
            && request.interaction_axis == 0
            && request.activation_serial == 0
            && request.action == 0
            && request.interaction_x == 0
            && request.interaction_y == 0
            && request.interaction_width == 0
            && request.interaction_height == 0 =>
        {
            PolicyRequestCause::Focus { target: target()? }
        }
        4 if request.interaction_phase == 0
            && request.interaction_kind == 0
            && request.interaction_axis == 0
            && request.activation_serial == 0
            && request.action != 0
            && request.interaction_x == 0
            && request.interaction_y == 0
            && request.interaction_width == 0
            && request.interaction_height == 0 =>
        {
            let output = OutputId::from_raw(request.action);
            if !affected_outputs.contains(&output) {
                return Err(invalid("pointer_focus_output", 0));
            }
            let target = if request.target_index == 0 && request.target_generation == 0 {
                None
            } else {
                let target = target()?;
                if !target.is_valid() {
                    return Err(invalid("pointer_focus_target", 0));
                }
                Some(target)
            };
            PolicyRequestCause::PointerFocus { output, target }
        }
        3 if request.activation_serial == 0 && request.action == 0 => {
            let phase = match request.interaction_phase {
                1 => PolicyInteractionPhase::Begin,
                2 => PolicyInteractionPhase::Update,
                3 => PolicyInteractionPhase::End,
                4 => PolicyInteractionPhase::Cancel,
                other => return Err(invalid("interaction_phase", u32::from(other))),
            };
            let kind = match request.interaction_kind {
                1 => PolicyInteractionKind::Move,
                2 => PolicyInteractionKind::Resize,
                3 => PolicyInteractionKind::Drag,
                4 => PolicyInteractionKind::Scroll,
                other => return Err(invalid("interaction_kind", u32::from(other))),
            };
            let axis = match request.interaction_axis {
                0 => PolicyInteractionAxis::None,
                1 => PolicyInteractionAxis::Horizontal,
                2 => PolicyInteractionAxis::Vertical,
                other => return Err(invalid("interaction_axis", u32::from(other))),
            };
            let geometry = Rect {
                x: request.interaction_x,
                y: request.interaction_y,
                width: request.interaction_width,
                height: request.interaction_height,
            };
            if !valid_policy_interaction_payload(phase, kind, axis, geometry) {
                return Err(invalid("interaction_cause", 0));
            }
            PolicyRequestCause::Interaction {
                phase,
                kind,
                axis,
                target: target()?,
                geometry,
            }
        }
        other => return Err(invalid("projection_request_cause", u32::from(other))),
    };
    Ok(crate::PolicyProjectionRequest {
        connection_epoch: request.connection_epoch,
        request_id: request.request_id,
        scene_generation: request.scene_generation,
        policy_generation: request.policy_generation,
        affected_outputs,
        cause,
    })
}

pub fn encode_wm_v1_policy_projection_outcome(
    connection_epoch: u64,
    request_id: u64,
    scene_generation: u64,
    outcome: PolicyProjectionOutcome,
) -> Result<WmV1ProjectionOutcome, IpcCodecError> {
    if connection_epoch == 0 || request_id == 0 || scene_generation == 0 {
        return Err(invalid("projection_outcome_identity", 0));
    }
    Ok(WmV1ProjectionOutcome {
        connection_epoch,
        request_id,
        scene_generation,
        outcome: match outcome {
            PolicyProjectionOutcome::Committed => SOPHIA_WM_OUTCOME_COMMITTED,
            PolicyProjectionOutcome::RejectedStale => SOPHIA_WM_OUTCOME_REJECTED_STALE,
            PolicyProjectionOutcome::RejectedInvalid => SOPHIA_WM_OUTCOME_REJECTED_INVALID,
            PolicyProjectionOutcome::TimedOut => SOPHIA_WM_OUTCOME_TIMED_OUT,
            PolicyProjectionOutcome::Disconnected => SOPHIA_WM_OUTCOME_DISCONNECTED,
        },
    })
}

pub fn decode_wm_v1_policy_projection_outcome(
    outcome: &WmV1ProjectionOutcome,
) -> Result<PolicyProjectionOutcome, IpcCodecError> {
    if outcome.connection_epoch == 0 || outcome.request_id == 0 || outcome.scene_generation == 0 {
        return Err(invalid("projection_outcome_identity", 0));
    }
    match outcome.outcome {
        SOPHIA_WM_OUTCOME_COMMITTED => Ok(PolicyProjectionOutcome::Committed),
        SOPHIA_WM_OUTCOME_REJECTED_STALE => Ok(PolicyProjectionOutcome::RejectedStale),
        SOPHIA_WM_OUTCOME_REJECTED_INVALID => Ok(PolicyProjectionOutcome::RejectedInvalid),
        SOPHIA_WM_OUTCOME_TIMED_OUT => Ok(PolicyProjectionOutcome::TimedOut),
        SOPHIA_WM_OUTCOME_DISCONNECTED => Ok(PolicyProjectionOutcome::Disconnected),
        other => Err(invalid("projection_outcome", u32::from(other))),
    }
}

pub fn encode_wm_v1_policy_configuration(
    configuration: &PolicyConfiguration,
) -> Result<WmV1PolicyConfiguration, IpcCodecError> {
    if configuration.connection_epoch == 0 || configuration.generation == 0 {
        return Err(invalid("policy_configuration_identity", 0));
    }
    if configuration.actions.len() > crate::POLICY_MAX_BINDINGS {
        return Err(IpcCodecError::CountTooLarge {
            count: configuration.actions.len(),
            max: crate::POLICY_MAX_BINDINGS,
        });
    }
    validate_policy_configuration(configuration)?;
    let records = configuration
        .actions
        .iter()
        .map(|action| {
            let (name_len, name) = encode_action_name(&action.name)?;
            Ok(WmV1SnapshotActionRecord {
                action: action.action.raw(),
                session_operation_slot: action.session_operation_slot.unwrap_or(0),
                name_len,
                name,
            })
        })
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
    let chrome = configuration.chrome;
    Ok(WmV1PolicyConfiguration {
        connection_epoch: configuration.connection_epoch,
        configuration_generation: configuration.generation,
        action_count: records.len() as u16,
        style_bits: u16::from(chrome.focus_ring.enabled) | u16::from(chrome.frame.enabled) << 1,
        focus_ring_width: chrome.focus_ring.width,
        focus_ring_color: encode_rgb(chrome.focus_ring.color),
        frame_width: chrome.frame.width,
        frame_focused_color: encode_rgb(chrome.frame.focused_color),
        frame_unfocused_color: encode_rgb(chrome.frame.unfocused_color),
        actions: encode_wm_v1_snapshot_action_records(&records)?,
    })
}

pub fn decode_wm_v1_policy_configuration(
    configuration: &WmV1PolicyConfiguration,
) -> Result<PolicyConfiguration, IpcCodecError> {
    let count = usize::from(configuration.action_count);
    if configuration.connection_epoch == 0
        || configuration.configuration_generation == 0
        || count > crate::POLICY_MAX_BINDINGS
        || configuration.style_bits & !0b11 != 0
    {
        return Err(invalid("policy_configuration", 0));
    }
    let records = decode_wm_v1_snapshot_action_records(&configuration.actions, count as u32)?;
    require_count(records.len(), count)?;
    let configuration = PolicyConfiguration {
        connection_epoch: configuration.connection_epoch,
        generation: configuration.configuration_generation,
        actions: records
            .into_iter()
            .map(|record| {
                Ok(PolicyActionRegistration {
                    action: WmActionId::from_raw(record.action),
                    name: decode_action_name(record.name_len, &record.name)?,
                    session_operation_slot: (record.session_operation_slot != 0)
                        .then_some(record.session_operation_slot),
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
        chrome: WmChromePolicy {
            focus_ring: WmFocusRingStyle {
                enabled: configuration.style_bits & 1 != 0,
                width: configuration.focus_ring_width,
                color: decode_rgb(configuration.focus_ring_color, "focus_ring_color")?,
            },
            frame: WmFrameStyle {
                enabled: configuration.style_bits & 2 != 0,
                width: configuration.frame_width,
                focused_color: decode_rgb(
                    configuration.frame_focused_color,
                    "frame_focused_color",
                )?,
                unfocused_color: decode_rgb(
                    configuration.frame_unfocused_color,
                    "frame_unfocused_color",
                )?,
            },
        },
    };
    validate_policy_configuration(&configuration)?;
    Ok(configuration)
}

fn validate_policy_configuration(configuration: &PolicyConfiguration) -> Result<(), IpcCodecError> {
    let valid_style = |enabled: bool, width: u32| {
        width <= 64 && ((enabled && width > 0) || (!enabled && width == 0))
    };
    if !valid_style(
        configuration.chrome.focus_ring.enabled,
        configuration.chrome.focus_ring.width,
    ) || !valid_style(
        configuration.chrome.frame.enabled,
        configuration.chrome.frame.width,
    ) {
        return Err(invalid("policy_configuration_chrome", 0));
    }

    let mut action_ids = std::collections::BTreeSet::new();
    let mut action_names = std::collections::BTreeSet::new();
    for action in &configuration.actions {
        if !action.action.is_valid()
            || encode_action_name(&action.name).is_err()
            || !action_ids.insert(action.action)
            || !action_names.insert(action.name.as_str())
        {
            return Err(invalid("policy_configuration_action", 0));
        }
    }
    Ok(())
}

fn encode_action_name(name: &str) -> Result<(u16, [u8; 128]), IpcCodecError> {
    if name.is_empty()
        || name.len() > crate::POLICY_ACTION_NAME_MAX_BYTES
        || name.trim() != name
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b' ' | b'.'))
    {
        return Err(invalid("policy_action_name", 0));
    }
    let mut encoded = [0; 128];
    encoded[..name.len()].copy_from_slice(name.as_bytes());
    Ok((name.len() as u16, encoded))
}

fn decode_action_name(length: u16, encoded: &[u8; 128]) -> Result<String, IpcCodecError> {
    let length = usize::from(length);
    if length == 0
        || length > crate::POLICY_ACTION_NAME_MAX_BYTES
        || encoded[length..].iter().any(|byte| *byte != 0)
    {
        return Err(invalid("policy_action_name", length as u32));
    }
    let name = core::str::from_utf8(&encoded[..length])
        .map_err(|_| invalid("policy_action_name", length as u32))?;
    encode_action_name(name)?;
    Ok(name.to_owned())
}

pub fn encode_wm_v1_policy_dirty(
    request: &PolicyDirtyRequest,
) -> Result<WmV1PolicyDirty, IpcCodecError> {
    if request.connection_epoch == 0 || request.policy_generation == 0 {
        return Err(invalid("policy_dirty_identity", 0));
    }
    let affected_outputs = encode_output_ids(&request.affected_outputs)?;
    Ok(WmV1PolicyDirty {
        connection_epoch: request.connection_epoch,
        policy_generation: request.policy_generation,
        affected_output_count: request.affected_outputs.len() as u16,
        affected_outputs,
    })
}

pub fn decode_wm_v1_policy_dirty(
    request: &WmV1PolicyDirty,
) -> Result<PolicyDirtyRequest, IpcCodecError> {
    if request.connection_epoch == 0 || request.policy_generation == 0 {
        return Err(invalid("policy_dirty_identity", 0));
    }
    Ok(PolicyDirtyRequest {
        connection_epoch: request.connection_epoch,
        policy_generation: request.policy_generation,
        affected_outputs: decode_output_ids(
            request.affected_output_count,
            &request.affected_outputs,
        )?,
    })
}

pub fn encode_wm_v1_policy_session_operation_request(
    request: PolicySessionOperationRequest,
) -> Result<WmV1SessionOperationRequest, IpcCodecError> {
    if request.connection_epoch == 0 || request.request_id == 0 || request.operation == 0 {
        return Err(invalid("session_operation_identity", 0));
    }
    let (target_index, target_generation) = request
        .target
        .map(|target| (target.index(), target.generation()))
        .unwrap_or((0, 0));
    Ok(WmV1SessionOperationRequest {
        connection_epoch: request.connection_epoch,
        request_id: request.request_id,
        operation: request.operation,
        target_index,
        target_generation,
    })
}

pub fn decode_wm_v1_policy_session_operation_request(
    request: &WmV1SessionOperationRequest,
) -> Result<PolicySessionOperationRequest, IpcCodecError> {
    if request.connection_epoch == 0 || request.request_id == 0 || request.operation == 0 {
        return Err(invalid("session_operation_identity", 0));
    }
    Ok(PolicySessionOperationRequest {
        connection_epoch: request.connection_epoch,
        request_id: request.request_id,
        operation: request.operation,
        target: decode_optional_surface(
            request.target_index,
            request.target_generation,
            "session_operation_target",
        )?,
    })
}

pub fn encode_wm_v1_policy_session_operation_outcome(
    outcome: PolicySessionOperationOutcome,
) -> Result<WmV1SessionOperationOutcome, IpcCodecError> {
    if outcome.connection_epoch == 0 || outcome.request_id == 0 {
        return Err(invalid("session_operation_outcome_identity", 0));
    }
    Ok(WmV1SessionOperationOutcome {
        connection_epoch: outcome.connection_epoch,
        request_id: outcome.request_id,
        outcome: encode_outcome(outcome.outcome),
    })
}

pub fn decode_wm_v1_policy_session_operation_outcome(
    outcome: &WmV1SessionOperationOutcome,
) -> Result<PolicySessionOperationOutcome, IpcCodecError> {
    if outcome.connection_epoch == 0 || outcome.request_id == 0 {
        return Err(invalid("session_operation_outcome_identity", 0));
    }
    Ok(PolicySessionOperationOutcome {
        connection_epoch: outcome.connection_epoch,
        request_id: outcome.request_id,
        outcome: decode_outcome(outcome.outcome)?,
    })
}

/// Encodes one complete scene snapshot for a client that negotiated
/// `selected_capabilities`.
///
/// Capability-governed record kinds are omitted, along with their declared
/// counts, when the client did not negotiate them. Omission keeps a frozen client
/// from ever receiving a record kind it must reject, which is what makes
/// server-to-client additions reversible after a revision freezes. Callers must
/// pass the capability set the server actually selected during negotiation, not
/// the set it supports; see `docs/sophia-policy-ipc.md`.
///
/// Scene outputs and surfaces are not gated. They are the interface's core
/// semantics, and a client that negotiated nothing still requires a complete
/// scene to propose against.
pub fn encode_wm_v1_policy_snapshot(
    transaction: TransactionId,
    connection_epoch: u64,
    scene: &PolicySceneSnapshot,
    actions: &[PolicyActionRegistration],
    classifications: &[PolicySurfaceClassification],
    selected_capabilities: u64,
) -> Result<WmV1SnapshotTransfer, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
    }
    if !scene.active_output.is_valid()
        || !scene
            .outputs
            .iter()
            .any(|output| output.output == scene.active_output)
    {
        return Err(invalid("snapshot_active_output", 0));
    }
    validate_wm_v1_snapshot_focus(scene)?;
    let outputs = scene
        .outputs
        .iter()
        .map(|output| {
            let (focus_index, focus_generation) = output
                .focus
                .map(|surface| (surface.index(), surface.generation()))
                .unwrap_or((0, 0));
            WmV1SnapshotOutputRecord {
                output: output.output.raw(),
                generation: output.generation,
                focus_index,
                focus_generation,
                x: output.bounds.x,
                y: output.bounds.y,
                width: output.bounds.width,
                height: output.bounds.height,
                work_x: output.work_area.x,
                work_y: output.work_area.y,
                work_width: output.work_area.width,
                work_height: output.work_area.height,
            }
        })
        .collect::<Vec<_>>();
    let surfaces = scene
        .surfaces
        .iter()
        .map(encode_surface_record)
        .collect::<Vec<_>>();
    let actions = if selected_capabilities & SOPHIA_WM_CAPABILITY_ACTIONS == 0 {
        Vec::new()
    } else {
        actions
            .iter()
            .map(|action| {
                let (name_len, name) = encode_action_name(&action.name)?;
                Ok(WmV1SnapshotActionRecord {
                    action: action.action.raw(),
                    session_operation_slot: action.session_operation_slot.unwrap_or(0),
                    name_len,
                    name,
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?
    };
    let session_operations = if selected_capabilities & SOPHIA_WM_CAPABILITY_SESSION_OPERATIONS == 0
    {
        Vec::new()
    } else {
        scene
            .session_operations
            .iter()
            .map(|operation| WmV1SnapshotSessionOperationRecord {
                operation: operation.token,
                slot: operation.slot,
                target_bits: u16::from(operation.permits_surface_target)
                    * POLICY_SESSION_OPERATION_SURFACE_TARGET,
            })
            .collect::<Vec<_>>()
    };
    let classifications = if selected_capabilities & SOPHIA_WM_CAPABILITY_LAUNCH_PLACEMENT == 0 {
        Vec::new()
    } else {
        let live_surfaces = scene
            .surfaces
            .iter()
            .map(|surface| surface.surface)
            .collect::<std::collections::BTreeSet<_>>();
        let mut seen = std::collections::BTreeSet::new();
        classifications
            .iter()
            .map(|classification| {
                if !classification.surface.is_valid()
                    || classification.classification == 0
                    || !live_surfaces.contains(&classification.surface)
                    || !seen.insert(classification.surface)
                {
                    return Err(invalid(
                        "surface_classification",
                        classification.surface.index(),
                    ));
                }
                Ok(WmV1SnapshotSurfaceClassificationRecord {
                    surface_index: classification.surface.index(),
                    surface_generation: classification.surface.generation(),
                    classification: classification.classification,
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?
    };
    let mut chunks = Vec::new();
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_OUTPUT_RECORD_KIND,
        outputs.len(),
        encode_wm_v1_snapshot_output_records(&outputs)?,
    )?;
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_SURFACE_RECORD_KIND,
        surfaces.len(),
        encode_wm_v1_snapshot_surface_records(&surfaces)?,
    )?;
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_ACTION_RECORD_KIND,
        actions.len(),
        encode_wm_v1_snapshot_action_records(&actions)?,
    )?;
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_SESSION_OPERATION_RECORD_KIND,
        session_operations.len(),
        encode_wm_v1_snapshot_session_operation_records(&session_operations)?,
    )?;
    // The frozen count describes only ordinary record chunks. Negotiated
    // extensions append after them with dense ordinals but do not spend any
    // SnapshotBegin or SnapshotEnd field.
    let chunk_count = u16::try_from(chunks.len()).map_err(|_| IpcCodecError::CountTooLarge {
        count: chunks.len(),
        max: u16::MAX as usize,
    })?;
    push_snapshot_chunk(
        &mut chunks,
        connection_epoch,
        SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND,
        classifications.len(),
        encode_wm_v1_snapshot_surface_classification_records(&classifications)?,
    )?;
    let begin = WmV1SnapshotBegin {
        connection_epoch,
        scene_generation: scene.generation,
        active_output: scene.active_output.raw(),
        chunk_count,
        output_count: u16::try_from(outputs.len()).map_err(|_| IpcCodecError::CountTooLarge {
            count: outputs.len(),
            max: u16::MAX as usize,
        })?,
        surface_count: surfaces.len() as u32,
        action_count: actions.len() as u16,
        session_operation_count: session_operations.len() as u16,
    };
    let end = WmV1SnapshotEnd {
        connection_epoch,
        scene_generation: scene.generation,
        chunk_count,
    };
    let mut transfer = WmV1SnapshotTransfer {
        transaction,
        begin,
        chunks,
        end,
    };
    super::append_wm_output_policy_keys(&mut transfer, &scene.outputs, selected_capabilities)?;
    Ok(transfer)
}

pub fn decode_wm_v1_policy_snapshot(
    transfer: &WmV1SnapshotTransfer,
) -> Result<WmV1DecodedSnapshot, IpcCodecError> {
    if !transfer.transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
    }
    if transfer.begin.connection_epoch != transfer.end.connection_epoch
        || transfer.begin.scene_generation != transfer.end.scene_generation
        || transfer.begin.chunk_count != transfer.end.chunk_count
        || usize::from(transfer.begin.chunk_count) > transfer.chunks.len()
        || transfer.begin.active_output == 0
    {
        return Err(invalid("snapshot_transfer", 0));
    }
    let mut outputs = Vec::new();
    let mut surfaces = Vec::new();
    let mut actions = Vec::new();
    let mut session_operations = Vec::new();
    let mut classifications = Vec::new();
    let ordinary_chunk_count = usize::from(transfer.begin.chunk_count);
    for (ordinal, chunk) in transfer.chunks.iter().enumerate() {
        if chunk.connection_epoch != transfer.begin.connection_epoch
            || usize::from(chunk.ordinal) != ordinal
        {
            return Err(invalid("snapshot_chunk_identity", chunk.ordinal as u32));
        }
        match (ordinal < ordinary_chunk_count, chunk.record_kind) {
            (true, SNAPSHOT_OUTPUT_RECORD_KIND) => outputs.extend(
                decode_wm_v1_snapshot_output_records(&chunk.data, chunk.item_count)?,
            ),
            (true, SNAPSHOT_SURFACE_RECORD_KIND) => surfaces.extend(
                decode_wm_v1_snapshot_surface_records(&chunk.data, chunk.item_count)?,
            ),
            (true, SNAPSHOT_ACTION_RECORD_KIND) => actions.extend(
                decode_wm_v1_snapshot_action_records(&chunk.data, chunk.item_count)?,
            ),
            (true, SNAPSHOT_SESSION_OPERATION_RECORD_KIND) => session_operations.extend(
                decode_wm_v1_snapshot_session_operation_records(&chunk.data, chunk.item_count)?,
            ),
            (false, SNAPSHOT_SURFACE_CLASSIFICATION_RECORD_KIND) => {
                classifications.extend(decode_wm_v1_snapshot_surface_classification_records(
                    &chunk.data,
                    chunk.item_count,
                )?)
            }
            (
                false,
                super::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND
                | super::SNAPSHOT_OUTPUT_POLICY_KEY_RECORD_KIND,
            ) => {}
            (_, other) => return Err(invalid("snapshot_record_kind", u32::from(other))),
        }
    }
    require_count(outputs.len(), transfer.begin.output_count as usize)?;
    require_count(surfaces.len(), transfer.begin.surface_count as usize)?;
    require_count(actions.len(), transfer.begin.action_count as usize)?;
    require_count(
        session_operations.len(),
        transfer.begin.session_operation_count as usize,
    )?;
    let mut scene = PolicySceneSnapshot {
        generation: transfer.begin.scene_generation,
        active_output: OutputId::from_raw(transfer.begin.active_output),
        outputs: outputs
            .into_iter()
            .map(|record| {
                Ok(PolicyOutputSnapshot {
                    policy_key: None,
                    output: OutputId::from_raw(record.output),
                    generation: record.generation,
                    focus: decode_optional_surface(
                        record.focus_index,
                        record.focus_generation,
                        "output_focus",
                    )?,
                    bounds: Rect {
                        x: record.x,
                        y: record.y,
                        width: record.width,
                        height: record.height,
                    },
                    work_area: Rect {
                        x: record.work_x,
                        y: record.work_y,
                        width: record.work_width,
                        height: record.work_height,
                    },
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
        surfaces: surfaces
            .into_iter()
            .map(decode_surface_record)
            .collect::<Result<Vec<_>, _>>()?,
        session_operations: session_operations
            .into_iter()
            .map(|record| {
                if record.operation == 0
                    || record.slot == 0
                    || record.target_bits & !POLICY_SESSION_OPERATION_SURFACE_TARGET != 0
                {
                    return Err(invalid("session_operation", record.target_bits.into()));
                }
                Ok(PolicySessionOperation {
                    token: record.operation,
                    slot: record.slot,
                    permits_surface_target: record.target_bits
                        & POLICY_SESSION_OPERATION_SURFACE_TARGET
                        != 0,
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
    };
    super::apply_wm_output_policy_keys(transfer, &mut scene.outputs)?;
    validate_wm_v1_snapshot_focus(&scene)?;
    let live_surfaces = scene
        .surfaces
        .iter()
        .map(|surface| surface.surface)
        .collect::<std::collections::BTreeSet<_>>();
    let mut seen_classifications = std::collections::BTreeSet::new();
    let classifications = classifications
        .into_iter()
        .map(|record| {
            let surface = SurfaceId::new(record.surface_index, record.surface_generation);
            if !surface.is_valid()
                || record.classification == 0
                || !live_surfaces.contains(&surface)
                || !seen_classifications.insert(surface)
            {
                return Err(invalid("surface_classification", record.surface_index));
            }
            Ok(PolicySurfaceClassification {
                surface,
                classification: record.classification,
            })
        })
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
    let mut launch_origins = Vec::new();
    for chunk in transfer
        .chunks
        .iter()
        .filter(|c| c.record_kind == super::SNAPSHOT_LAUNCH_ORIGIN_RECORD_KIND)
    {
        if chunk.item_count == 0 {
            return Err(invalid("launch_origin_capability", 0));
        }
        launch_origins.extend(super::decode_wm_launch_context_records(
            &chunk.data,
            chunk.item_count,
        )?);
    }
    super::encode_wm_launch_context_records(&launch_origins)?;
    for origin in &launch_origins {
        if origin.epoch != transfer.begin.connection_epoch
            || !live_surfaces.contains(&origin.surface)
        {
            return Err(invalid("launch_origin_surface", 0));
        }
    }
    Ok(WmV1DecodedSnapshot {
        launch_origins,
        scene,
        actions: actions
            .into_iter()
            .map(|record| {
                Ok(PolicyActionRegistration {
                    action: WmActionId::from_raw(record.action),
                    name: decode_action_name(record.name_len, &record.name)?,
                    session_operation_slot: (record.session_operation_slot != 0)
                        .then_some(record.session_operation_slot),
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
        classifications,
    })
}

fn validate_wm_v1_snapshot_focus(scene: &PolicySceneSnapshot) -> Result<(), IpcCodecError> {
    for output in &scene.outputs {
        let Some(focus) = output.focus else {
            continue;
        };
        if !scene.surfaces.iter().any(|surface| {
            surface.surface == focus
                && surface.current_output == Some(output.output)
                && surface.capabilities.focusable
                && !surface.current_state.minimized
        }) {
            return Err(invalid("snapshot_output_focus", focus.index()));
        }
    }
    Ok(())
}

pub fn encode_wm_v1_policy_projection(
    proposal: &PolicyProjectionProposal,
) -> Result<WmV1ProjectionTransfer, IpcCodecError> {
    if !proposal.transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
    }
    if !proposal.active_output.is_valid() {
        return Err(invalid("projection_active_output", 0));
    }
    let outputs = proposal
        .outputs
        .iter()
        .map(|output| {
            let (focus_index, focus_generation) = output
                .focus
                .map(|surface| (surface.index(), surface.generation()))
                .unwrap_or((0, 0));
            Ok(WmV1ProjectionOutputRecord {
                output: output.output.raw(),
                placement_count: u32::try_from(output.placements.len()).map_err(|_| {
                    IpcCodecError::CountTooLarge {
                        count: output.placements.len(),
                        max: u32::MAX as usize,
                    }
                })?,
                focus_index,
                focus_generation,
            })
        })
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
    let placements = proposal
        .outputs
        .iter()
        .flat_map(|output| output.placements.iter())
        .map(encode_placement_record)
        .collect::<Vec<_>>();
    let indicators = proposal
        .indicators
        .iter()
        .map(|indicator| {
            let (label_len, label) = encode_indicator_text(&indicator.label, "indicator_label")?;
            Ok(WmV1ProjectionIndicatorRecord {
                output: indicator.output.raw(),
                slot: indicator.slot,
                indicator: indicator.indicator,
                action: indicator.action.map_or(0, WmActionId::raw),
                state_bits: indicator.state_bits,
                label_len,
                label,
            })
        })
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
    let statuses = proposal
        .output_statuses
        .iter()
        .map(|status| {
            let (layout_len, layout) = encode_indicator_text(&status.layout, "status_layout")?;
            Ok(WmV1ProjectionOutputStatusRecord {
                output: status.output.raw(),
                focus_bits: status.focus_bits,
                layout_len,
                layout,
            })
        })
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
    let mut chunks = Vec::new();
    push_projection_chunk(
        &mut chunks,
        proposal.connection_epoch,
        PROJECTION_OUTPUT_RECORD_KIND,
        outputs.len(),
        encode_wm_v1_projection_output_records(&outputs)?,
    )?;
    push_projection_chunk(
        &mut chunks,
        proposal.connection_epoch,
        PROJECTION_PLACEMENT_RECORD_KIND,
        placements.len(),
        encode_wm_v1_projection_placement_records(&placements)?,
    )?;
    push_projection_chunk(
        &mut chunks,
        proposal.connection_epoch,
        PROJECTION_INDICATOR_RECORD_KIND,
        indicators.len(),
        encode_wm_v1_projection_indicator_records(&indicators)?,
    )?;
    push_projection_chunk(
        &mut chunks,
        proposal.connection_epoch,
        PROJECTION_OUTPUT_STATUS_RECORD_KIND,
        statuses.len(),
        encode_wm_v1_projection_output_status_records(&statuses)?,
    )?;
    let chunk_count = chunks.len() as u16;
    chunks.extend(super::encode_wm_tab_groups(
        &proposal.tab_groups,
        proposal.connection_epoch,
        chunk_count,
    )?);
    chunks.extend(super::encode_wm_translation_groups(
        &proposal.translation_groups,
        proposal.connection_epoch,
        chunks.len() as u16,
    )?);
    chunks.extend(super::encode_wm_launch_contexts(
        &proposal.launch_contexts,
        proposal.connection_epoch,
        chunks.len() as u16,
    )?);
    let begin = WmV1ProjectionBegin {
        connection_epoch: proposal.connection_epoch,
        request_id: proposal.request_id,
        base_generation: proposal.base_generation,
        active_output: proposal.active_output.raw(),
        chunk_count,
        output_count: outputs.len() as u16,
        placement_count: placements.len() as u32,
        indicator_count: indicators.len() as u16,
        status_count: statuses.len() as u16,
    };
    let end = WmV1ProjectionEnd {
        connection_epoch: proposal.connection_epoch,
        request_id: proposal.request_id,
        base_generation: proposal.base_generation,
        chunk_count,
    };
    Ok(WmV1ProjectionTransfer {
        transaction: proposal.transaction,
        begin,
        chunks,
        end,
    })
}

pub fn decode_wm_v1_policy_projection(
    transfer: &WmV1ProjectionTransfer,
) -> Result<PolicyProjectionProposal, IpcCodecError> {
    if !transfer.transaction.is_valid() {
        return Err(IpcCodecError::InvalidTransaction(0));
    }
    if transfer.begin.connection_epoch != transfer.end.connection_epoch
        || transfer.begin.request_id != transfer.end.request_id
        || transfer.begin.base_generation != transfer.end.base_generation
        || transfer.begin.chunk_count != transfer.end.chunk_count
        || usize::from(transfer.begin.chunk_count) > transfer.chunks.len()
        || transfer.begin.active_output == 0
    {
        return Err(invalid("projection_transfer", 0));
    }
    let mut outputs = Vec::new();
    let mut placements = Vec::new();
    let mut indicators = Vec::new();
    let mut statuses = Vec::new();
    for (ordinal, chunk) in transfer.chunks.iter().enumerate() {
        if chunk.connection_epoch != transfer.begin.connection_epoch
            || usize::from(chunk.ordinal) != ordinal
        {
            return Err(invalid("projection_chunk_identity", chunk.ordinal as u32));
        }
        if ordinal >= usize::from(transfer.begin.chunk_count) && chunk.record_kind < 0xff00 {
            return Err(invalid(
                "projection_extension_order",
                u32::from(chunk.record_kind),
            ));
        }
        match chunk.record_kind {
            PROJECTION_OUTPUT_RECORD_KIND => outputs.extend(
                decode_wm_v1_projection_output_records(&chunk.data, chunk.item_count)?,
            ),
            PROJECTION_PLACEMENT_RECORD_KIND => placements.extend(
                decode_wm_v1_projection_placement_records(&chunk.data, chunk.item_count)?,
            ),
            PROJECTION_INDICATOR_RECORD_KIND => indicators.extend(
                decode_wm_v1_projection_indicator_records(&chunk.data, chunk.item_count)?,
            ),
            PROJECTION_OUTPUT_STATUS_RECORD_KIND => statuses.extend(
                decode_wm_v1_projection_output_status_records(&chunk.data, chunk.item_count)?,
            ),
            super::PROJECTION_TAB_GROUP_RECORD_KIND
            | super::PROJECTION_TAB_MEMBER_RECORD_KIND
            | super::PROJECTION_TRANSLATION_GROUP_RECORD_KIND
            | super::PROJECTION_TRANSLATION_MEMBER_RECORD_KIND
            | super::PROJECTION_LAUNCH_CONTEXT_RECORD_KIND
                if ordinal >= usize::from(transfer.begin.chunk_count) => {}
            other => return Err(invalid("projection_record_kind", u32::from(other))),
        }
    }
    require_count(outputs.len(), transfer.begin.output_count as usize)?;
    require_count(placements.len(), transfer.begin.placement_count as usize)?;
    require_count(indicators.len(), transfer.begin.indicator_count as usize)?;
    require_count(statuses.len(), transfer.begin.status_count as usize)?;
    let mut placement_cursor = placements.into_iter();
    let mut projected_outputs = Vec::with_capacity(outputs.len());
    for output in outputs {
        let focus = decode_optional_surface(output.focus_index, output.focus_generation, "focus")?;
        let mut projected = Vec::with_capacity(output.placement_count as usize);
        for _ in 0..output.placement_count {
            let record = placement_cursor
                .next()
                .ok_or_else(|| invalid("placement_count", output.placement_count))?;
            projected.push(decode_placement_record(record)?);
        }
        projected_outputs.push(PolicyOutputProjection {
            output: OutputId::from_raw(output.output),
            placements: projected,
            focus,
        });
    }
    if placement_cursor.next().is_some() {
        return Err(invalid("placement_count", transfer.begin.placement_count));
    }
    Ok(PolicyProjectionProposal {
        launch_contexts: super::decode_wm_launch_contexts(&transfer.chunks)?,
        translation_groups: super::decode_wm_translation_groups(&transfer.chunks)?,
        tab_groups: super::decode_wm_tab_groups(&transfer.chunks)?,
        transaction: transfer.transaction,
        connection_epoch: transfer.begin.connection_epoch,
        request_id: transfer.begin.request_id,
        base_generation: transfer.begin.base_generation,
        active_output: OutputId::from_raw(transfer.begin.active_output),
        outputs: projected_outputs,
        indicators: indicators
            .into_iter()
            .map(|record| {
                Ok(PolicyProjectionIndicator {
                    output: OutputId::from_raw(record.output),
                    slot: record.slot,
                    indicator: record.indicator,
                    action: (record.action != 0).then(|| WmActionId::from_raw(record.action)),
                    state_bits: record.state_bits,
                    label: decode_indicator_text(
                        record.label_len,
                        &record.label,
                        "indicator_label",
                    )?,
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
        output_statuses: statuses
            .into_iter()
            .map(|record| {
                Ok(PolicyProjectionOutputStatus {
                    output: OutputId::from_raw(record.output),
                    focus_bits: record.focus_bits,
                    layout: decode_indicator_text(
                        record.layout_len,
                        &record.layout,
                        "status_layout",
                    )?,
                })
            })
            .collect::<Result<Vec<_>, IpcCodecError>>()?,
    })
}

fn encode_indicator_text(
    text: &str,
    field: &'static str,
) -> Result<(u16, [u8; 32]), IpcCodecError> {
    if text.is_empty() || text.len() > 32 || text.chars().any(char::is_control) {
        return Err(invalid(field, text.len() as u32));
    }
    let mut bytes = [0; 32];
    bytes[..text.len()].copy_from_slice(text.as_bytes());
    Ok((text.len() as u16, bytes))
}

fn decode_indicator_text(
    length: u16,
    bytes: &[u8; 32],
    field: &'static str,
) -> Result<String, IpcCodecError> {
    let length = usize::from(length);
    if length == 0 || length > bytes.len() || bytes[length..].iter().any(|byte| *byte != 0) {
        return Err(invalid(field, length as u32));
    }
    let text = core::str::from_utf8(&bytes[..length]).map_err(|_| invalid(field, length as u32))?;
    if text.chars().any(char::is_control) {
        return Err(invalid(field, length as u32));
    }
    Ok(text.to_owned())
}

fn push_snapshot_chunk(
    chunks: &mut Vec<WmV1SnapshotChunk>,
    connection_epoch: u64,
    record_kind: u16,
    count: usize,
    data: Vec<u8>,
) -> Result<(), IpcCodecError> {
    if count == 0 {
        return Ok(());
    }
    chunks.push(WmV1SnapshotChunk {
        connection_epoch,
        ordinal: chunks.len() as u16,
        record_kind,
        item_count: u32::try_from(count).map_err(|_| IpcCodecError::CountTooLarge {
            count,
            max: u32::MAX as usize,
        })?,
        data,
    });
    Ok(())
}

fn push_projection_chunk(
    chunks: &mut Vec<WmV1ProjectionChunk>,
    connection_epoch: u64,
    record_kind: u16,
    count: usize,
    data: Vec<u8>,
) -> Result<(), IpcCodecError> {
    if count == 0 {
        return Ok(());
    }
    chunks.push(WmV1ProjectionChunk {
        connection_epoch,
        ordinal: chunks.len() as u16,
        record_kind,
        item_count: u32::try_from(count).map_err(|_| IpcCodecError::CountTooLarge {
            count,
            max: u32::MAX as usize,
        })?,
        data,
    });
    Ok(())
}

fn encode_surface_record(surface: &PolicySurfaceSnapshot) -> WmV1SnapshotSurfaceRecord {
    let mut capability_bits = 0;
    capability_bits |= u16::from(surface.capabilities.movable) * POLICY_SURFACE_CAPABILITY_MOVABLE;
    capability_bits |=
        u16::from(surface.capabilities.resizable) * POLICY_SURFACE_CAPABILITY_RESIZABLE;
    capability_bits |=
        u16::from(surface.capabilities.focusable) * POLICY_SURFACE_CAPABILITY_FOCUSABLE;
    capability_bits |=
        u16::from(surface.capabilities.closable) * POLICY_SURFACE_CAPABILITY_CLOSABLE;
    capability_bits |=
        u16::from(surface.capabilities.fullscreenable) * POLICY_SURFACE_CAPABILITY_FULLSCREENABLE;
    let (transient_index, transient_generation) = surface
        .transient_owner
        .map(|owner| (owner.index(), owner.generation()))
        .unwrap_or((0, 0));
    let (min_width, min_height) = encode_optional_size(surface.constraints.min_size);
    let (max_width, max_height) = encode_optional_size(surface.constraints.max_size);
    WmV1SnapshotSurfaceRecord {
        surface_index: surface.surface.index(),
        surface_generation: surface.surface.generation(),
        state_generation: surface.generation,
        current_output: surface.current_output.map_or(0, OutputId::raw),
        capability_bits,
        kind: surface.kind as u16,
        request_state_bits: encode_presentation(surface.requested_state),
        current_state_bits: encode_presentation(surface.current_state),
        transient_index,
        transient_generation,
        x: surface.geometry.x,
        y: surface.geometry.y,
        width: surface.geometry.width,
        height: surface.geometry.height,
        min_width,
        min_height,
        max_width,
        max_height,
        exact_width: surface.exact_size.map_or(0, |size| size.width),
        exact_height: surface.exact_size.map_or(0, |size| size.height),
    }
}

fn decode_surface_record(
    record: WmV1SnapshotSurfaceRecord,
) -> Result<PolicySurfaceSnapshot, IpcCodecError> {
    if record.capability_bits & !POLICY_SURFACE_CAPABILITY_SUPPORTED != 0 {
        return Err(invalid(
            "surface_capabilities",
            u32::from(record.capability_bits),
        ));
    }
    Ok(PolicySurfaceSnapshot {
        surface: SurfaceId::new(record.surface_index, record.surface_generation),
        generation: record.state_generation,
        current_output: (record.current_output != 0)
            .then(|| OutputId::from_raw(record.current_output)),
        kind: match record.kind {
            1 => PolicySurfaceKind::Toplevel,
            2 => PolicySurfaceKind::Dialog,
            3 => PolicySurfaceKind::Utility,
            4 => PolicySurfaceKind::Popup,
            5 => PolicySurfaceKind::Unknown,
            other => return Err(invalid("surface_kind", u32::from(other))),
        },
        capabilities: LayoutNodeCapabilities {
            movable: record.capability_bits & POLICY_SURFACE_CAPABILITY_MOVABLE != 0,
            resizable: record.capability_bits & POLICY_SURFACE_CAPABILITY_RESIZABLE != 0,
            focusable: record.capability_bits & POLICY_SURFACE_CAPABILITY_FOCUSABLE != 0,
            closable: record.capability_bits & POLICY_SURFACE_CAPABILITY_CLOSABLE != 0,
            fullscreenable: record.capability_bits & POLICY_SURFACE_CAPABILITY_FULLSCREENABLE != 0,
        },
        constraints: SurfaceConstraints {
            min_size: decode_optional_size(record.min_width, record.min_height, "min_size")?,
            max_size: decode_optional_size(record.max_width, record.max_height, "max_size")?,
        },
        exact_size: decode_optional_size(record.exact_width, record.exact_height, "exact_size")?,
        requested_state: decode_presentation(record.request_state_bits, "requested_state")?,
        current_state: decode_presentation(record.current_state_bits, "current_state")?,
        transient_owner: decode_optional_surface(
            record.transient_index,
            record.transient_generation,
            "transient_owner",
        )?,
        geometry: Rect {
            x: record.x,
            y: record.y,
            width: record.width,
            height: record.height,
        },
    })
}

fn encode_placement_record(placement: &PolicySurfacePlacement) -> WmV1ProjectionPlacementRecord {
    let (requested_width, requested_height) = encode_optional_size(placement.requested_size);
    let crop = placement.crop.unwrap_or_default();
    WmV1ProjectionPlacementRecord {
        surface_index: placement.surface.index(),
        surface_generation: placement.surface.generation(),
        state_generation: placement.surface_generation,
        x: placement.geometry.x,
        y: placement.geometry.y,
        width: placement.geometry.width,
        height: placement.geometry.height,
        requested_width,
        requested_height,
        crop_x: crop.x,
        crop_y: crop.y,
        crop_width: crop.width,
        crop_height: crop.height,
        transform: placement.transform as u16,
        presentation_bits: encode_presentation(placement.presentation),
    }
}

fn decode_placement_record(
    record: WmV1ProjectionPlacementRecord,
) -> Result<PolicySurfacePlacement, IpcCodecError> {
    let crop = if record.crop_width == 0 && record.crop_height == 0 {
        if record.crop_x != 0 || record.crop_y != 0 {
            return Err(invalid("crop", 0));
        }
        None
    } else if record.crop_width > 0 && record.crop_height > 0 {
        Some(Rect {
            x: record.crop_x,
            y: record.crop_y,
            width: record.crop_width,
            height: record.crop_height,
        })
    } else {
        return Err(invalid("crop", 0));
    };
    Ok(PolicySurfacePlacement {
        surface: SurfaceId::new(record.surface_index, record.surface_generation),
        surface_generation: record.state_generation,
        geometry: Rect {
            x: record.x,
            y: record.y,
            width: record.width,
            height: record.height,
        },
        requested_size: decode_optional_size(
            record.requested_width,
            record.requested_height,
            "requested_size",
        )?,
        crop,
        transform: match record.transform {
            1 => PolicyTransform::Identity,
            other => return Err(invalid("policy_transform", u32::from(other))),
        },
        presentation: decode_presentation(record.presentation_bits, "presentation")?,
    })
}

fn encode_presentation(state: PolicyPresentationState) -> u16 {
    (u16::from(state.fullscreen) * POLICY_PRESENTATION_FULLSCREEN)
        | (u16::from(state.maximized) * POLICY_PRESENTATION_MAXIMIZED)
        | (u16::from(state.minimized) * POLICY_PRESENTATION_MINIMIZED)
}

fn encode_rgb(color: WmRgb8) -> u32 {
    0xff00_0000 | u32::from(color.red) << 16 | u32::from(color.green) << 8 | u32::from(color.blue)
}

fn decode_rgb(value: u32, field: &'static str) -> Result<WmRgb8, IpcCodecError> {
    if value >> 24 != 0xff {
        return Err(invalid(field, value));
    }
    Ok(WmRgb8 {
        red: (value >> 16) as u8,
        green: (value >> 8) as u8,
        blue: value as u8,
    })
}

fn encode_outcome(outcome: PolicyProjectionOutcome) -> u16 {
    match outcome {
        PolicyProjectionOutcome::Committed => SOPHIA_WM_OUTCOME_COMMITTED,
        PolicyProjectionOutcome::RejectedStale => SOPHIA_WM_OUTCOME_REJECTED_STALE,
        PolicyProjectionOutcome::RejectedInvalid => SOPHIA_WM_OUTCOME_REJECTED_INVALID,
        PolicyProjectionOutcome::TimedOut => SOPHIA_WM_OUTCOME_TIMED_OUT,
        PolicyProjectionOutcome::Disconnected => SOPHIA_WM_OUTCOME_DISCONNECTED,
    }
}

fn decode_outcome(outcome: u16) -> Result<PolicyProjectionOutcome, IpcCodecError> {
    match outcome {
        SOPHIA_WM_OUTCOME_COMMITTED => Ok(PolicyProjectionOutcome::Committed),
        SOPHIA_WM_OUTCOME_REJECTED_STALE => Ok(PolicyProjectionOutcome::RejectedStale),
        SOPHIA_WM_OUTCOME_REJECTED_INVALID => Ok(PolicyProjectionOutcome::RejectedInvalid),
        SOPHIA_WM_OUTCOME_TIMED_OUT => Ok(PolicyProjectionOutcome::TimedOut),
        SOPHIA_WM_OUTCOME_DISCONNECTED => Ok(PolicyProjectionOutcome::Disconnected),
        other => Err(invalid("policy_outcome", u32::from(other))),
    }
}

fn encode_output_ids(outputs: &[OutputId]) -> Result<Vec<u8>, IpcCodecError> {
    if outputs.is_empty() || outputs.len() > crate::POLICY_MAX_OUTPUTS {
        return Err(IpcCodecError::CountTooLarge {
            count: outputs.len(),
            max: crate::POLICY_MAX_OUTPUTS,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut encoded = Vec::with_capacity(outputs.len() * OUTPUT_ID_WIRE_SIZE);
    for output in outputs {
        if !output.is_valid() || !seen.insert(*output) {
            return Err(invalid("affected_output", output.raw() as u32));
        }
        encoded.extend_from_slice(&output.raw().to_le_bytes());
    }
    Ok(encoded)
}

fn decode_output_ids(count: u16, bytes: &[u8]) -> Result<Vec<OutputId>, IpcCodecError> {
    let count = usize::from(count);
    if count == 0 || count > crate::POLICY_MAX_OUTPUTS || bytes.len() != count * OUTPUT_ID_WIRE_SIZE
    {
        return Err(invalid("affected_output_bytes", bytes.len() as u32));
    }
    let mut seen = std::collections::BTreeSet::new();
    bytes
        .chunks_exact(OUTPUT_ID_WIRE_SIZE)
        .map(|bytes| {
            let output = OutputId::from_raw(u64::from_le_bytes(
                bytes.try_into().expect("fixed output-id chunk"),
            ));
            if !output.is_valid() || !seen.insert(output) {
                return Err(invalid("affected_output", output.raw() as u32));
            }
            Ok(output)
        })
        .collect()
}

fn decode_presentation(
    bits: u16,
    field: &'static str,
) -> Result<PolicyPresentationState, IpcCodecError> {
    if bits & !POLICY_PRESENTATION_SUPPORTED != 0 {
        return Err(invalid(field, u32::from(bits)));
    }
    Ok(PolicyPresentationState {
        fullscreen: bits & POLICY_PRESENTATION_FULLSCREEN != 0,
        maximized: bits & POLICY_PRESENTATION_MAXIMIZED != 0,
        minimized: bits & POLICY_PRESENTATION_MINIMIZED != 0,
    })
}

fn encode_optional_size(size: Option<Size>) -> (i32, i32) {
    size.map(|size| (size.width, size.height)).unwrap_or((0, 0))
}

fn decode_optional_size(
    width: i32,
    height: i32,
    field: &'static str,
) -> Result<Option<Size>, IpcCodecError> {
    if width == 0 && height == 0 {
        Ok(None)
    } else if width > 0 && height > 0 {
        Ok(Some(Size { width, height }))
    } else {
        Err(invalid(field, 0))
    }
}

fn decode_optional_surface(
    index: u32,
    generation: u32,
    field: &'static str,
) -> Result<Option<SurfaceId>, IpcCodecError> {
    if index == 0 && generation == 0 {
        Ok(None)
    } else if generation != 0 {
        Ok(Some(SurfaceId::new(index, generation)))
    } else {
        Err(invalid(field, index))
    }
}

fn require_count(actual: usize, expected: usize) -> Result<(), IpcCodecError> {
    if actual == expected {
        Ok(())
    } else {
        Err(invalid("record_count", actual as u32))
    }
}

fn invalid(field: &'static str, value: u32) -> IpcCodecError {
    IpcCodecError::InvalidEnum { field, value }
}

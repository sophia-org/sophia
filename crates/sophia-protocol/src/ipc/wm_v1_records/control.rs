pub fn encode_wm_v1_policy_projection_request(
    request: &crate::PolicyProjectionRequest,
) -> Result<WmV1ProjectionRequest, IpcCodecError> {
    super::validate_projection_request_identity(
        request.connection_epoch,
        request.request_id,
        request.scene_generation,
        request.policy_generation,
    )?;
    let affected_outputs = encode_output_ids(&request.affected_outputs)?;
    if matches!(
        request.cause,
        crate::PolicyRequestCause::OutputAction { .. }
            | crate::PolicyRequestCause::PresentationAction { .. }
    ) {
        // A wire-only rule: this scalar message has no room for a target.
        return Err(invalid("targeted_action_requires_separate_message", 0));
    }
    super::validate_request_cause_scalars(&request.cause, &request.affected_outputs)?;
    super::validate_request_cause_targets(&request.cause)?;
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
        crate::PolicyRequestCause::OutputAction { .. }
        | crate::PolicyRequestCause::PresentationAction { .. } => {
            return Err(invalid("targeted_action_requires_separate_message", 0));
        }
        PolicyRequestCause::SceneChanged => (0, 0, 0, 0, 0, 0, 0, 0, Rect::default()),
        PolicyRequestCause::Action {
            activation_serial,
            action,
        } => (
                1,
                0,
                0,
                0,
                activation_serial,
                action.raw(),
                0,
                0,
                Rect::default(),
            ),
        PolicyRequestCause::Focus { target } => (
                2,
                0,
                0,
                0,
                0,
                0,
                target.index(),
                target.generation(),
                Rect::default(),
            ),
        PolicyRequestCause::PointerFocus { output, target } => (
                4,
                0,
                0,
                0,
                0,
                output.raw(),
                target.map_or(0, |t| t.index()),
                target.map_or(0, |t| t.generation()),
                Rect::default(),
            ),
        PolicyRequestCause::Interaction {
            phase,
            kind,
            axis,
            target,
            geometry,
        } => (
                3,
                super::policy_interaction_phase_code(phase),
                super::policy_interaction_kind_code(kind),
                super::policy_interaction_axis_code(axis),
                0,
                0,
                target.index(),
                target.generation(),
                geometry,
            ),
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
    super::validate_projection_request_identity(
        request.connection_epoch,
        request.request_id,
        request.scene_generation,
        request.policy_generation,
    )?;
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
    let affected_outputs = decode_output_ids(request.affected_output_count, &request.affected_outputs)?;
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
            let phase = super::policy_interaction_phase_from_code(request.interaction_phase)
                .ok_or_else(|| invalid("interaction_phase", u32::from(request.interaction_phase)))?;
            let kind = super::policy_interaction_kind_from_code(request.interaction_kind)
                .ok_or_else(|| invalid("interaction_kind", u32::from(request.interaction_kind)))?;
            let axis = super::policy_interaction_axis_from_code(request.interaction_axis)
                .ok_or_else(|| invalid("interaction_axis", u32::from(request.interaction_axis)))?;
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
    // Shared cause semantics. The target validity of Focus and Interaction is
    // deliberately not required here: this decoder has always accepted any
    // target whose generation is nonzero, and it still does.
    super::validate_request_cause_scalars(&cause, &affected_outputs)?;
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
        outcome: super::policy_projection_outcome_code(outcome),
    })
}

pub fn decode_wm_v1_policy_projection_outcome(
    outcome: &WmV1ProjectionOutcome,
) -> Result<PolicyProjectionOutcome, IpcCodecError> {
    if outcome.connection_epoch == 0 || outcome.request_id == 0 || outcome.scene_generation == 0 {
        return Err(invalid("projection_outcome_identity", 0));
    }
    super::policy_projection_outcome_from_code(outcome.outcome)
        .ok_or_else(|| invalid("projection_outcome", u32::from(outcome.outcome)))
}

pub fn encode_wm_v1_policy_configuration(
    configuration: &PolicyConfiguration,
) -> Result<WmV1PolicyConfiguration, IpcCodecError> {
    let sections = encode_policy_configuration_records(configuration)?;
    let actions = sections
        .into_iter()
        .next()
        .map_or_else(Vec::new, |s| s.bytes);
    let chrome = configuration.chrome;
    Ok(WmV1PolicyConfiguration {
        connection_epoch: configuration.connection_epoch,
        configuration_generation: configuration.generation,
        action_count: configuration.actions.len() as u16,
        style_bits: u16::from(chrome.focus_ring.enabled) | u16::from(chrome.frame.enabled) << 1,
        focus_ring_width: chrome.focus_ring.width,
        focus_ring_color: encode_rgb(chrome.focus_ring.color),
        frame_width: chrome.frame.width,
        frame_focused_color: encode_rgb(chrome.frame.focused_color),
        frame_unfocused_color: encode_rgb(chrome.frame.unfocused_color),
        actions,
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
        actions: decode_policy_action_rows(records)?,
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
    super::validate_dirty_identity(request.connection_epoch, request.policy_generation)?;
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
    super::validate_dirty_identity(request.connection_epoch, request.policy_generation)?;
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
    // The target is deliberately not checked: this encoder never has, and a
    // caller of the legacy wire keeps that behaviour. A new transport uses
    // validate_policy_session_operation_request.
    super::validate_session_operation_identity(
        request.connection_epoch,
        request.request_id,
        request.operation,
    )?;
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
    super::validate_session_operation_identity(
        request.connection_epoch,
        request.request_id,
        request.operation,
    )?;
    // Legacy acceptance: any target whose generation is nonzero, valid or not.
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
    super::validate_policy_session_operation_outcome(&outcome)?;
    Ok(WmV1SessionOperationOutcome {
        connection_epoch: outcome.connection_epoch,
        request_id: outcome.request_id,
        outcome: encode_outcome(outcome.outcome),
    })
}

pub fn decode_wm_v1_policy_session_operation_outcome(
    outcome: &WmV1SessionOperationOutcome,
) -> Result<PolicySessionOperationOutcome, IpcCodecError> {
    // Identity first, then the code, as this decoder always ordered them.
    super::validate_policy_session_operation_outcome(&PolicySessionOperationOutcome {
        connection_epoch: outcome.connection_epoch,
        request_id: outcome.request_id,
        outcome: PolicyProjectionOutcome::Committed,
    })?;
    Ok(PolicySessionOperationOutcome {
        connection_epoch: outcome.connection_epoch,
        request_id: outcome.request_id,
        outcome: decode_outcome(outcome.outcome)?,
    })
}

//! Structural descriptor checks. Snapshot membership, freshness and action
//! consumption remain with their owners. These checks grant no authority.
use super::*;
use crate::{InvalidRecord, MAX_CHROME_LABEL_LEN};
use std::collections::BTreeSet;

pub fn validate_toplevel_action(action: ToplevelActionCapabilityRef) -> Result<(), InvalidRecord> {
    if action.token == 0
        || action.issuer_epoch == 0
        || action.issuer_revocation_epoch == 0
        || action.recipient_epoch == 0
        || action.target_slot == 0
        || action.target_generation == 0
    {
        return Err(InvalidRecord("shell_toplevel_action"));
    }
    Ok(())
}

/// Shared by standalone descriptors and tabs, without manufacturing a
/// temporary standalone snapshot (or imposing its sixteen-entry limit).
pub fn validate_shell_descriptor(
    descriptor: &ShellV1Descriptor,
    recipient_epoch: u64,
) -> Result<(), InvalidRecord> {
    validate_toplevel_action(descriptor.action)?;
    if recipient_epoch == 0
        || descriptor.slot == 0
        || descriptor.generation == 0
        || descriptor.action.recipient_epoch != recipient_epoch
        || descriptor.action.target_slot != descriptor.slot
        || descriptor.action.target_generation != descriptor.generation
    {
        return Err(InvalidRecord("shell_descriptor"));
    }
    if let Some(label) = &descriptor.label
        && (label.text.is_empty()
            || label.text.len() > MAX_CHROME_LABEL_LEN
            || label.text.chars().any(char::is_control))
    {
        return Err(InvalidRecord("shell_descriptor_label"));
    }
    Ok(())
}

pub fn validate_shell_descriptor_snapshot(
    snapshot: &ShellV1DescriptorSnapshot,
) -> Result<(), InvalidRecord> {
    if snapshot.connection_epoch == 0
        || snapshot.snapshot_generation == 0
        || !snapshot.output.is_valid()
        || snapshot.output_generation == 0
        || snapshot.broker_epoch == 0
        || snapshot.broker_revocation_epoch == 0
        || snapshot.descriptors.len() > SOPHIA_SHELL_MAX_DESCRIPTORS
    {
        return Err(InvalidRecord("shell_descriptor_snapshot"));
    }
    let mut slots = BTreeSet::new();
    for descriptor in &snapshot.descriptors {
        validate_shell_descriptor(descriptor, snapshot.connection_epoch)?;
        if !slots.insert(descriptor.slot)
            || descriptor.action.issuer_epoch != snapshot.broker_epoch
            || descriptor.action.issuer_revocation_epoch != snapshot.broker_revocation_epoch
        {
            return Err(InvalidRecord("shell_descriptor"));
        }
    }
    Ok(())
}

pub fn validate_shell_descriptor_candidate(
    candidate: &ShellV1Candidate,
) -> Result<(), InvalidRecord> {
    if candidate.connection_epoch == 0
        || candidate.snapshot_generation == 0
        || candidate.candidate_generation == 0
        || !candidate.output.is_valid()
        || candidate.entries.len() > SOPHIA_SHELL_MAX_DESCRIPTORS
    {
        return Err(InvalidRecord("shell_candidate"));
    }
    if candidate.visible == candidate.entries.is_empty() {
        return Err(InvalidRecord("shell_candidate_visibility"));
    }
    if let Some(reservation) = candidate.reservation {
        if !candidate.visible {
            return Err(InvalidRecord("shell_candidate_hidden_reservation"));
        }
        if reservation.thickness_px == 0
            || reservation.thickness_px > SOPHIA_SHELL_MAX_RESERVATION_THICKNESS_PX
        {
            return Err(InvalidRecord("shell_candidate_reservation_thickness"));
        }
    }
    let mut slots = BTreeSet::new();
    for entry in &candidate.entries {
        if entry.slot == 0 || entry.generation == 0 || !slots.insert(entry.slot) {
            return Err(InvalidRecord("shell_candidate_entry"));
        }
    }
    if candidate.visible != candidate.selected_slot.is_some()
        || candidate
            .selected_slot
            .is_some_and(|slot| !slots.contains(&slot))
    {
        return Err(InvalidRecord("shell_candidate_selection"));
    }
    Ok(())
}

pub fn validate_shell_descriptor_outcome(
    outcome: ShellV1CandidateOutcome,
) -> Result<(), InvalidRecord> {
    if outcome.connection_epoch == 0 || outcome.candidate_generation == 0 {
        return Err(InvalidRecord("shell_candidate_outcome"));
    }
    if (outcome.kind == ShellV1CandidateOutcomeKind::Presented) != (outcome.presentation_epoch != 0)
    {
        return Err(InvalidRecord("shell_candidate_outcome_epoch"));
    }
    Ok(())
}

pub fn validate_shell_descriptor_activation(value: ShellV1Activation) -> Result<(), InvalidRecord> {
    validate_toplevel_action(value.action)?;
    if value.connection_epoch == 0
        || value.candidate_generation == 0
        || value.presentation_epoch == 0
        || value.activation == 0
        || value.action.recipient_epoch != value.connection_epoch
    {
        return Err(InvalidRecord("shell_activation"));
    }
    Ok(())
}

pub fn validate_shell_descriptor_activation_ack(
    value: ShellV1ActivationAck,
) -> Result<(), InvalidRecord> {
    if value.connection_epoch == 0 || value.activation == 0 {
        return Err(InvalidRecord("shell_activation_ack"));
    }
    Ok(())
}

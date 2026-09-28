use super::*;
use crate::{
    InvalidRecord, SOPHIA_SHELL_MAX_APPLICATIONS, SOPHIA_SHELL_MAX_LAUNCHER_ROWS,
    shell_launcher_text_valid,
};
use std::collections::BTreeSet;

fn require(ok: bool) -> Result<(), InvalidRecord> {
    if ok {
        Ok(())
    } else {
        Err(InvalidRecord("shell_launcher"))
    }
}

pub fn validate_shell_launcher_request(value: &ShellLauncherRequest) -> Result<(), InvalidRecord> {
    require(
        value.connection_epoch > 0
            && value.catalog_generation > 0
            && value.request_generation > 0
            && value.output.is_valid()
            && value.output_generation > 0
            && shell_launcher_text_valid(&value.query, SOPHIA_SHELL_MAX_QUERY_BYTES),
    )
}

pub fn validate_shell_launcher_candidate(
    value: &ShellLauncherCandidate,
) -> Result<(), InvalidRecord> {
    require(
        value.connection_epoch > 0
            && value.catalog_generation > 0
            && value.request_generation > 0
            && value.candidate_generation > 0
            && value.output.is_valid()
            && value.entries.len() <= SOPHIA_SHELL_MAX_LAUNCHER_ROWS
            && (10..=32).contains(&value.font_size)
            && value.colors[1..].iter().all(|color| color >> 24 == 255),
    )?;
    let mut slots = BTreeSet::new();
    for slot in &value.entries {
        require(
            *slot > 0 && usize::from(*slot) <= SOPHIA_SHELL_MAX_APPLICATIONS && slots.insert(*slot),
        )?;
    }
    require(value.selected == 0 || slots.contains(&value.selected))
}

pub fn validate_shell_launcher_outcome(value: ShellLauncherOutcome) -> Result<(), InvalidRecord> {
    require(
        value.connection_epoch > 0
            && value.request_generation > 0
            && value.candidate_generation > 0
            && (value.kind != ShellV1CandidateOutcomeKind::Presented
                || value.presentation_epoch > 0),
    )
}

pub fn validate_shell_launcher_activation(
    value: ShellLauncherActivation,
) -> Result<(), InvalidRecord> {
    require(
        value.connection_epoch > 0
            && value.catalog_generation > 0
            && value.request_generation > 0
            && value.candidate_generation > 0
            && value.presentation_epoch > 0
            && value.activation > 0
            && value.slot > 0
            && usize::from(value.slot) <= SOPHIA_SHELL_MAX_APPLICATIONS,
    )
}

pub fn validate_shell_launcher_activation_ack(
    value: ShellLauncherActivationAck,
) -> Result<(), InvalidRecord> {
    validate_shell_launcher_activation(value.activation)
}

pub fn validate_shell_launch_outcome(value: ShellLaunchOutcome) -> Result<(), InvalidRecord> {
    validate_shell_launcher_activation(value.activation)
}

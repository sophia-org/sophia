use super::*;
use crate::{InvalidRecord, ShellV1CandidateOutcomeKind, shell_launcher_text_valid};
use std::collections::BTreeSet;

fn require(ok: bool) -> Result<(), InvalidRecord> {
    if ok {
        Ok(())
    } else {
        Err(InvalidRecord("shell_reference"))
    }
}
fn text(value: &str, max: usize) -> bool {
    !value.is_empty() && shell_launcher_text_valid(value, max)
}

pub fn validate_shell_shortcut_catalog(value: &ShellShortcutCatalog) -> Result<(), InvalidRecord> {
    require(
        value.connection_epoch > 0
            && value.generation > 0
            && value.entries.len() <= SOPHIA_SHELL_MAX_SHORTCUTS,
    )?;
    let mut slots = BTreeSet::new();
    for entry in &value.entries {
        require(
            entry.slot > 0
                && slots.insert(entry.slot)
                && text(&entry.chord, 64)
                && text(&entry.action, 128)
                && entry.label.as_ref().is_none_or(|v| text(v, 128))
                && entry.group.as_ref().is_none_or(|v| text(v, 64)),
        )?;
    }
    Ok(())
}

pub fn validate_shell_reference_request(value: ShellReferenceRequest) -> Result<(), InvalidRecord> {
    require(
        value.connection_epoch > 0
            && value.catalog_generation > 0
            && value.request_generation > 0
            && value.output.is_valid()
            && value.output_generation > 0,
    )
}

pub fn validate_shell_reference_candidate(
    value: &ShellReferenceCandidate,
) -> Result<(), InvalidRecord> {
    let style = &value.style;
    require(
        value.connection_epoch > 0
            && value.catalog_generation > 0
            && value.request_generation > 0
            && value.candidate_generation > 0
            && value.output.is_valid()
            && value.entries.len() <= SOPHIA_SHELL_MAX_SHORTCUTS
            && (1..=4).contains(&style.columns)
            && (8..=32).contains(&style.body_size)
            && (8..=48).contains(&style.title_size)
            && style.padding <= 64
            && style.row_gap <= 32
            && style.key_gap <= 64
            && style.column_gap <= 64
            && style.border <= 16
            && style.margin <= 128
            && style.colors[1..].iter().all(|color| color >> 24 == 255)
            && text(&style.title, 128),
    )?;
    let mut slots = BTreeSet::new();
    for entry in &value.entries {
        require(
            entry.slot > 0
                && slots.insert(entry.slot)
                && text(&entry.key, 64)
                && text(&entry.label, 128),
        )?;
    }
    Ok(())
}

pub fn validate_shell_reference_outcome(value: ShellReferenceOutcome) -> Result<(), InvalidRecord> {
    require(
        value.connection_epoch > 0
            && value.catalog_generation > 0
            && value.request_generation > 0
            && value.candidate_generation > 0
            && value.pages > 0
            && value.page < value.pages
            && (value.kind != ShellV1CandidateOutcomeKind::Presented
                || value.presentation_epoch > 0),
    )
}

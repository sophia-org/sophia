use super::*;
use crate::{InvalidRecord, validate_shell_descriptor};
use std::collections::BTreeSet;

pub fn validate_shell_tab_snapshot(snapshot: &ShellTabSnapshot) -> Result<(), InvalidRecord> {
    let bad = || InvalidRecord("shell_tabs");
    if snapshot.connection_epoch == 0
        || snapshot.generation == 0
        || snapshot.groups.len() > SOPHIA_SHELL_MAX_TAB_GROUPS
    {
        return Err(bad());
    }
    // Subtract from a bounded remaining allowance rather than summing
    // caller-owned lengths that could overflow before the limit comparison.
    let mut remaining = SOPHIA_SHELL_MAX_TAB_ENTRIES;
    for group in &snapshot.groups {
        remaining = remaining.checked_sub(group.entries.len()).ok_or_else(bad)?;
    }
    let mut groups = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for group in &snapshot.groups {
        if group.slot == 0
            || !group.output.is_valid()
            || !groups.insert(group.slot)
            || group.entries.is_empty() != group.selected_slot.is_none()
            || group
                .selected_slot
                .is_some_and(|slot| !group.entries.iter().any(|d| d.slot == slot))
        {
            return Err(bad());
        }
        for descriptor in &group.entries {
            validate_shell_descriptor(descriptor, snapshot.connection_epoch)?;
            if !slots.insert(descriptor.slot) {
                return Err(bad());
            }
        }
    }
    Ok(())
}

pub fn validate_shell_tab_candidate(candidate: &ShellTabCandidate) -> Result<(), InvalidRecord> {
    if candidate.connection_epoch == 0
        || candidate.snapshot_generation == 0
        || candidate.candidate_generation == 0
        || candidate.groups.len() > SOPHIA_SHELL_MAX_TAB_GROUPS
        || candidate.groups.contains(&0)
        || candidate.groups.iter().collect::<BTreeSet<_>>().len() != candidate.groups.len()
    {
        return Err(InvalidRecord("shell_tabs"));
    }
    Ok(())
}

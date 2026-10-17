use super::*;
use crate::shell::unique::unique;
use crate::{InvalidRecord, validate_shell_descriptor};

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
    if !unique::<SOPHIA_SHELL_MAX_TAB_GROUPS>(snapshot.groups.iter().map(|g| g.slot))
        || !unique::<SOPHIA_SHELL_MAX_TAB_ENTRIES>(
            snapshot
                .groups
                .iter()
                .flat_map(|g| g.entries.iter().map(|d| u64::from(d.slot))),
        )
    {
        return Err(bad());
    }
    for group in &snapshot.groups {
        if group.slot == 0
            || !group.output.is_valid()
            || group.entries.is_empty() != group.selected_slot.is_none()
            || group
                .selected_slot
                .is_some_and(|slot| !group.entries.iter().any(|d| d.slot == slot))
        {
            return Err(bad());
        }
        for descriptor in &group.entries {
            validate_shell_descriptor(descriptor, snapshot.connection_epoch)?;
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
        || !unique::<SOPHIA_SHELL_MAX_TAB_GROUPS>(candidate.groups.iter().copied())
    {
        return Err(InvalidRecord("shell_tabs"));
    }
    Ok(())
}

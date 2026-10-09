//! Display commands are synchronous on the host, but their effects can be
//! observed in the guest before the host records completion. Bind witnesses
//! to the attempt and require a matching successful completion separately.

use std::collections::BTreeSet;

use super::{Mode, Record, named_fields};

pub(super) struct DisplayActions {
    pub offs: Vec<usize>,
    pub ons: Vec<usize>,
    pub completed: usize,
}

pub(super) fn verify(records: &[Record], mode: Mode, heads: u32) -> Result<DisplayActions, String> {
    let mut actions = DisplayActions {
        offs: Vec::new(),
        ons: Vec::new(),
        completed: 0,
    };
    let mut pending = None;
    let mut removed = BTreeSet::new();
    let mut returned = BTreeSet::new();
    for (index, record) in records.iter().enumerate() {
        if record.name != "sophia_qemu_unplug"
            || !matches!(record.get("status"), Some("sending" | "sent"))
        {
            continue;
        }
        let target = record.get("target").unwrap_or("");
        let console = target
            .strip_prefix("Console_")
            .and_then(|value| value.parse::<u32>().ok());
        if !named_fields(record, &["action", "target"])
            || !matches!(record.get("action"), Some("off" | "on"))
            || !console.is_some_and(|value| value < heads && target == format!("Console_{value}"))
        {
            return Err("display commands: malformed attempt or completion".to_owned());
        }
        let action = record.get("action").unwrap();
        if record.get("status") == Some("sending") {
            if pending.is_some() {
                return Err("display commands: overlapping attempts are out of order".to_owned());
            }
            if action == "off" {
                if !returned.is_empty() || !removed.insert(target) {
                    return Err(
                        "display commands: removals are duplicated or out of order".to_owned()
                    );
                }
                actions.offs.push(index);
            } else {
                if !mode.returns() || !removed.contains(target) || !returned.insert(target) {
                    return Err(
                        "display commands: returns are duplicated or out of order".to_owned()
                    );
                }
                actions.ons.push(index);
            }
            pending = Some((action, target));
        } else {
            if pending != Some((action, target)) {
                return Err("display commands: unmatched completion is out of order".to_owned());
            }
            pending = None;
            actions.completed = index;
        }
    }
    if pending.is_some() {
        return Err("display commands: missing completion".to_owned());
    }
    let expected = if mode.all_heads() { heads } else { 1 };
    if removed.len() != expected as usize {
        return Err(format!(
            "display commands: expected {expected} distinct removals"
        ));
    }
    if mode.returns() && returned != removed {
        return Err("display commands: return targets differ from removals".to_owned());
    }
    Ok(actions)
}

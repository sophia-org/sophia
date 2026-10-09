//! The host endpoint's hard refusals, applied to every mode before its own
//! verdict. The harness records a process it ended but could not reap as
//! `qemu_exit=unreaped`, `logger_exit=unreaped` or `display_bus_exit=unreaped`,
//! with that process's pid, and a guest it had to stop as `status=guest_stopped`.
//! Each says the host lost custody of a process, whatever any later scan finds,
//! so no pixel or topology reading of the same log may pass, or reproduce a
//! failure, beside it. Every field is inspected, so a second, conflicting
//! value of the same key cannot hide the first.

use super::Record;

const HOST: &str = "sophia_qemu_unplug";
const EXITS: [&str; 3] = ["qemu_exit", "logger_exit", "display_bus_exit"];
const PIDS: [&str; 3] = ["qemu_pid", "logger_pid", "display_bus_pid"];

/// The first host record that refuses the run, as the reason it does.
pub(super) fn refusal(records: &[Record]) -> Option<String> {
    records.iter().find_map(|record| {
        if record.name != HOST {
            return None;
        }
        if record
            .fields
            .iter()
            .any(|(key, value)| key == "status" && value == "guest_stopped")
        {
            return Some("the host stopped the guest (status=guest_stopped)".to_owned());
        }
        record.fields.iter().find_map(|(key, value)| {
            if EXITS.contains(&key.as_str()) && value == "unreaped" {
                Some(format!(
                    "the host recorded an unreaped process ({key}=unreaped)"
                ))
            } else if PIDS.contains(&key.as_str()) {
                Some(format!(
                    "the host recorded an unreaped process ({key}={value})"
                ))
            } else {
                None
            }
        })
    })
}

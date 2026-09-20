//! Record-scoped fields for the seat's device evidence: an opaque identity,
//! what the device can do, and what a departure released. The records carry
//! no names or paths to begin with; this admits exactly the vocabulary they
//! do carry, since the general filter keeps only measurements and the words
//! it knows, and had reduced these to their booleans.
pub(super) fn record(name: &str) -> bool {
    matches!(
        name,
        "sophia_live_session_input_device" | "sophia_live_session_keys"
    )
}

pub(super) fn field(record: &str, key: &str, value: &str) -> bool {
    let limit = match key {
        "schema" => Some(1),
        // Minted identities count up from 256 for the life of the process.
        "device" | "released" | "count" | "fallbacks" => Some(u64::MAX),
        "surface" => Some(u64::from(u32::MAX)),
        _ => None,
    };
    if let Some(limit) = limit {
        return !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<u64>().is_ok_and(|number| number <= limit);
    }
    match (record, key) {
        ("sophia_live_session_input_device", "status") => {
            matches!(value, "added" | "removed" | "key_observed" | "summary")
        }
        ("sophia_live_session_input_device", "keyboard" | "pointer" | "touch" | "virtual") => {
            matches!(value, "true" | "false")
        }
        ("sophia_live_session_input_device", "source") => matches!(value, "udev" | "paths"),
        ("sophia_live_session_keys", "status") => value == "released",
        ("sophia_live_session_keys", "reason") => matches!(
            value,
            "clear_focus"
                | "device_removed"
                | "emergency"
                | "focus_handoff"
                | "input_reopened"
                | "logout"
                | "routed_input_saturation"
                | "runtime_deadline"
                | "seat_release"
                | "virtual_terminal"
        ),
        ("sophia_live_session_keys", "scope") => value == "all",
        _ => false,
    }
}

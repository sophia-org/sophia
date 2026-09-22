//! Record-scoped fields for the seat's input evidence: an opaque device
//! identity, what the device can do, what a departure released, and which
//! surface a routed button reached. The records carry
//! no names or paths to begin with; this admits exactly the vocabulary they
//! do carry, since the general filter keeps only measurements and the words
//! it knows, and had reduced these to their booleans.
pub(super) fn record(name: &str) -> bool {
    matches!(
        name,
        "sophia_live_session_input_device"
            | "sophia_live_session_keys"
            | "sophia_live_session_pointer_target"
            | "sophia_live_session_pointer_projection"
    )
}

pub(super) fn field(record: &str, key: &str, value: &str) -> bool {
    let limit = match key {
        "schema" => Some(1),
        // Minted identities count up from 256 for the life of the process.
        "device" | "released" | "count" | "fallbacks" => Some(u64::MAX),
        "surface" | "generation" | "target" => Some(u64::from(u32::MAX)),
        "epoch" | "projections" | "layers" | "contains" => Some(u64::MAX),
        _ => None,
    };
    if let Some(limit) = limit {
        return !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<u64>().is_ok_and(|number| number <= limit);
    }
    match (record, key) {
        // The routed-target record says which surface a button reached and
        // what kind of surface it is. Both vocabularies are closed, and naming
        // them is the point: this record exists to distinguish a click that
        // reached a popup from one that fell through to the window beneath, so
        // an unrecognised value is dropped rather than carried.
        ("sophia_live_session_pointer_target", "status") => value == "button_routed",
        ("sophia_live_session_pointer_projection", "status") => value == "button_routed",
        // A bounded, rank-descending list of surface:rank pairs, or `-` for
        // none. Bounded so a record the redactor trusts cannot grow without
        // limit; validated pairwise so nothing but two numbers rides in each.
        ("sophia_live_session_pointer_projection", "under" | "all") => {
            value == "-"
                || (value.split(',').count() <= 8
                    && value.split(',').all(|pair| {
                        pair.split_once(':').is_some_and(|(surface, rank)| {
                            !surface.is_empty()
                                && !rank.is_empty()
                                && surface.bytes().all(|b| b.is_ascii_digit())
                                && rank.bytes().all(|b| b.is_ascii_digit())
                                && surface.parse::<u32>().is_ok()
                                && rank.parse::<u32>().is_ok()
                        })
                    }))
        }
        ("sophia_live_session_pointer_target", "role") => {
            matches!(value, "client_positioned" | "policy_managed" | "unknown")
        }
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

//! Record-scoped fields for frontend lifecycle evidence. No wire payloads,
//! application strings or raw X resource IDs enter the daily recorder.
pub(super) fn record(name: &str) -> bool {
    matches!(
        name,
        "sophia_x_window_lifecycle"
            | "sophia_x_present_submission"
            | "sophia_x_present_delivery"
            | "sophia_x11_client_output"
    )
}

pub(super) fn field(record: &str, key: &str, value: &str) -> bool {
    let limit = match key {
        "schema" => Some(1),
        "client" | "transaction" | "window_token" | "pixmap_token" | "subscription_token"
        | "pending_count" | "outstanding_bytes" | "limit_bytes" | "silence_msec"
        | "allowance_msec" => Some(u64::MAX),
        "surface" | "generation" | "serial" => Some(u64::from(u32::MAX)),
        "sequence" | "width" | "height" => Some(u64::from(u16::MAX)),
        "major" => Some(u64::from(u8::MAX)),
        _ => None,
    };
    if let Some(limit) = limit {
        return !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.parse::<u64>().is_ok_and(|number| number <= limit);
    }
    match (record, key) {
        ("sophia_x_window_lifecycle", "requested_kind") => matches!(
            value,
            "inherit" | "input_output" | "input_only" | "invalid" | "unspecified"
        ),
        ("sophia_x_window_lifecycle", "role") => {
            matches!(value, "PolicyManaged" | "ClientPositioned")
        }
        ("sophia_x_window_lifecycle", "mapped") => matches!(value, "true" | "false"),
        ("sophia_x_window_lifecycle", "status") => value == "removed",
        // A connection ended by its own output: the two bounds of t165.
        ("sophia_x11_client_output", "status") => value == "ended",
        ("sophia_x11_client_output", "cause") => matches!(value, "saturated" | "silent"),
        ("sophia_x_present_submission", "status") => value == "accepted",
        ("sophia_x_present_delivery", "kind") => matches!(value, "complete" | "idle" | "msc"),
        ("sophia_x_present_delivery", "status") => matches!(
            value,
            "ready" | "queued" | "queue_failed" | "peer_gone" | "write_started" | "written"
        ),
        _ => false,
    }
}

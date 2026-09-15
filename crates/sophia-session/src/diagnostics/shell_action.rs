//! Exact host-owned causal identifiers, never client text or coordinates.

pub(super) fn record(name: &str) -> bool {
    matches!(
        name,
        "sophia_shell_action_receipt"
            | "sophia_shell_action_cause"
            | "sophia_shell_action_policy"
            | "sophia_shell_native_binding"
            | "sophia_shell_native_completion"
            | "sophia_shell_indicator_state"
    )
}

pub(super) fn field(record: &str, key: &str, value: &str) -> bool {
    let number = !value.is_empty()
        && value.bytes().all(|c| c.is_ascii_digit())
        && value.parse::<u64>().is_ok();
    match (record, key) {
        (_, "schema") => value == "1",
        ("sophia_shell_native_completion", "missing_kernel_timestamp") => {
            matches!(value, "0" | "1")
        }
        ("sophia_shell_native_completion", "timestamp_source") => {
            matches!(value, "kernel" | "observation_fallback")
        }
        (
            "sophia_shell_indicator_state",
            "connection_epoch"
            | "indicator_generation"
            | "output"
            | "indicator"
            | "action"
            | "slot"
            | "state_bits"
            | "entries",
        ) => number,
        ("sophia_shell_action_receipt", "status") => matches!(value, "issued" | "acknowledged"),
        ("sophia_shell_action_receipt", "disposition") => matches!(value, "0" | "1" | "2"),
        (
            "sophia_shell_action_receipt",
            "connection_epoch"
            | "content_grant_epoch"
            | "event_id"
            | "output"
            | "candidate_generation"
            | "presentation_epoch"
            | "target_id"
            | "target_generation"
            | "action"
            | "monotonic_usec",
        ) => number,
        ("sophia_shell_action_cause", "admission") => {
            matches!(value, "Admitted" | "Duplicate" | "RejectedCapacity")
        }
        (
            "sophia_shell_action_cause",
            "connection_epoch"
            | "event_id"
            | "output"
            | "action"
            | "activation_serial"
            | "policy_connection_epoch",
        ) => number,
        ("sophia_shell_action_policy", "outcome") => matches!(
            value,
            "Committed" | "RejectedInvalid" | "RejectedStale" | "TimedOut" | "Disconnected"
        ),
        (
            "sophia_shell_action_policy",
            "policy_connection_epoch"
            | "activation_serial"
            | "action"
            | "transaction"
            | "request_id"
            | "indicator_generation",
        ) => number,
        (
            "sophia_shell_native_binding",
            "connection_epoch"
            | "content_grant_epoch"
            | "output"
            | "candidate_generation"
            | "native_owner"
            | "native_frame"
            | "head"
            | "target_generation"
            | "mode_refresh_millihz"
            | "heads",
        ) => number,
        (
            "sophia_shell_native_completion",
            "output" | "native_owner" | "native_frame" | "heads" | "monotonic_usec",
        ) => number,
        _ => false,
    }
}

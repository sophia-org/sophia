//! Exact host-owned causal identifiers, never client text or coordinates.

pub(super) fn record(name: &str) -> bool {
    matches!(
        name,
        "sophia_shell_action_receipt" | "sophia_shell_action_cause" | "sophia_shell_action_policy"
    )
}

pub(super) fn field(record: &str, key: &str, value: &str) -> bool {
    let number = !value.is_empty()
        && value.bytes().all(|c| c.is_ascii_digit())
        && value.parse::<u64>().is_ok();
    match (record, key) {
        (_, "schema") => value == "1",
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
        _ => false,
    }
}

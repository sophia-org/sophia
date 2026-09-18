//! Bounded host-owned component evidence; no catalog/query/client payloads.
pub(super) fn record(name: &str) -> bool {
    matches!(
        name,
        "sophia_shell_component"
            | "sophia_shell_component_catalog"
            | "sophia_native_launcher"
            | "sophia_shell_components_shutdown"
    )
}
pub(super) fn field(record: &str, key: &str, value: &str) -> bool {
    let number = |maximum: u64| {
        !value.is_empty()
            && value.bytes().all(|c| c.is_ascii_digit())
            && value.parse::<u64>().is_ok_and(|v| v <= maximum)
    };
    if key == "schema" {
        return value == "1";
    }
    match (record, key) {
        ("sophia_shell_components_shutdown", "status") => value == "quiescent",
        ("sophia_shell_component", "status") => matches!(
            value,
            "negotiated"
                | "negotiation_failed"
                | "process_retired"
                | "process_failed"
                | "stop_failed"
                | "poll_failed"
                | "start_failed"
                | "catalog_failed"
                | "service_failed"
                | "input_failed"
                | "input_rejected"
        ),
        ("sophia_shell_component", "role") => matches!(value, "bar" | "application_launcher"),
        ("sophia_shell_component", "gpu_mode") => matches!(value, "direct" | "denied"),
        ("sophia_shell_component", "endpoint_released") => matches!(value, "true" | "false"),
        ("sophia_shell_component", "slot") => number(1),
        ("sophia_shell_component", "revision") => number(u16::MAX.into()),
        ("sophia_shell_component", "device_major" | "device_minor") => number(u32::MAX.into()),
        (
            "sophia_shell_component",
            "connection_epoch" | "content_grant_epoch" | "gpu_grant_epoch",
        ) => number(u64::MAX),
        ("sophia_shell_component_catalog", "status") => value == "built",
        ("sophia_shell_component_catalog", "generation" | "entries") => number(u64::MAX),
        ("sophia_native_launcher", "status") => matches!(
            value,
            "open_expired"
                | "process_started"
                | "execution_rejected"
                | "spawn_failed"
                | "input_failed"
        ),
        ("sophia_native_launcher", "slot") => number(1),
        ("sophia_native_launcher", "transaction") => number(u64::MAX),
        _ => false,
    }
}

//! Retain bounded completion facts when the native watchdog stops a session.
pub(super) fn reduce<'a>(fields: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut result = String::from("sophia_native_stall");
    let mut schema = false;
    for field in fields.take(24) {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        if !seen.insert(key) {
            continue;
        }
        let valid = match key {
            "schema" => {
                schema = value == "1";
                schema
            }
            "head"
            | "output"
            | "age_ms"
            | "retirements"
            | "out_fence_retirements"
            | "late_events"
            | "poller_pending"
            | "decoded"
            | "rejected" => {
                !value.is_empty()
                    && value.bytes().all(|b| b.is_ascii_digit())
                    && value.parse::<u64>().is_ok()
            }
            "ledger_pending" => matches!(value, "true" | "false"),
            "completion_fence" => matches!(value, "Unsupported" | "Pending" | "Signaled"),
            "completion_mode" => matches!(value, "PageFlipPreferred" | "OutFenceAuthoritative"),
            _ => false,
        };
        if valid {
            result.push(' ');
            result.push_str(field);
        }
    }
    schema.then_some(result)
}

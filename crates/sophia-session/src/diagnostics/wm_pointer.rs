//! Retained fields of `sophia_live_wm_pointer`: what a Super+button gesture
//! on a managed window became, and why one went nowhere.
//!
//! Without this the record was reduced to its schema and surface, so a
//! retained log could show that a gesture happened and nothing about its
//! fate. The dropped case exists because a gesture on a surface between two
//! committed layouts used to end the session; its record has to survive
//! reduction or the next such gap is invisible again.

pub(super) fn record(name: &str) -> bool {
    name == "sophia_live_wm_pointer"
}

pub(super) fn field(_record: &str, key: &str, value: &str) -> bool {
    match key {
        "schema" => matches!(value, "1" | "2"),
        "status" => matches!(
            value,
            "interaction_admitted" | "interaction_dropped" | "request_rejected"
        ),
        "reason" => matches!(value, "outside_outputs" | "target_unplaced" | "capacity"),
        "phase" => matches!(value, "Begin" | "Update" | "End" | "Cancel"),
        "mode" => matches!(value, "Move" | "Resize"),
        "surface" => !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

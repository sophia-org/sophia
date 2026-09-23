//! What the selection record may say in the reduced evidence. Without this the
//! general filter keeps its schema and status and drops the counts, and a
//! retained session could not say whether any client ever took a selection
//! or asked for one -- which is the whole of what the record reports. The
//! counts come from the wire opcodes (SetSelectionOwner, ConvertSelection),
//! never from selection contents, which the record already redacts.
pub(super) fn record(name: &str) -> bool {
    name == "sophia_live_selection"
}

pub(super) fn field(_record: &str, key: &str, value: &str) -> bool {
    match key {
        "schema" => value == "1",
        "status" => value == "complete",
        "content" => value == "redacted",
        "owner_changes" | "conversions" => {
            !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && value.parse::<u64>().is_ok()
        }
        _ => false,
    }
}

//! What the XTEST records may say in the reduced evidence. Without this the
//! general filter keeps their schema and drops the rest, and a reader could
//! not tell a session that admitted XTEST from one that did not, or how much
//! was injected -- which is the whole point of the records.
pub(super) fn record(name: &str) -> bool {
    name == "sophia_live_session_xtest"
}

pub(super) fn field(_record: &str, key: &str, value: &str) -> bool {
    match key {
        "schema" => value == "1",
        "status" => matches!(value, "admitted" | "absent" | "complete"),
        "admitted" => matches!(value, "true" | "false"),
        "group" | "issued" | "denied" | "injected_keys" | "injected_buttons"
        | "injected_motions" | "refused" => {
            !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && value.parse::<u64>().is_ok()
        }
        _ => false,
    }
}

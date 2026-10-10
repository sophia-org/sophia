//! Renderer-image handoff evidence (t322), reduced before any other rule.
//!
//! A handoff record says whether the retained images of a retired owner were
//! captured, kept, discarded or lost to a failed export, how many, and on
//! which path. Its fields are admitted only with a known status; otherwise the
//! record keeps only its name. Every value is an unsigned integer or a fixed
//! word, and an error is never copied.

const HANDOFF: &str = "sophia_live_renderer_handoff";
/// More fields than any producer prints; the rest are not read.
const FIELD_LIMIT: usize = 10;

pub(super) fn record(name: &str) -> bool {
    name == HANDOFF
}

pub(super) fn reduce<'a>(name: &str, fields: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut pairs: Vec<(&str, &str, &str)> = Vec::with_capacity(FIELD_LIMIT);
    for field in fields.take(FIELD_LIMIT) {
        let Some((key, value)) = field.split_once('=') else {
            continue;
        };
        if pairs.iter().all(|(seen, _, _)| *seen != key) {
            pairs.push((key, value, field));
        }
    }
    let mut result = name.to_owned();
    // A record without a known status keeps only its name: it still counts as a
    // handoff event, and none of its values can be trusted to be bounded.
    if !pairs
        .iter()
        .any(|(key, value, _)| *key == "status" && status(value))
    {
        return Some(result);
    }
    for (key, value, field) in pairs {
        if field_allowed(key, value) {
            result.push(' ');
            result.push_str(field);
        }
    }
    Some(result)
}

fn status(value: &str) -> bool {
    matches!(value, "captured" | "retained" | "discarded" | "failed")
}

fn field_allowed(key: &str, value: &str) -> bool {
    match key {
        "schema" => value == "1",
        "status" => status(value),
        "phase" => value == "export_images",
        // The seat and topology paths that settle a handoff.
        "source" => matches!(
            value,
            "terminal_switch"
                | "switch_rejected"
                | "disable_timeout"
                | "forced_detach"
                | "seat_resume"
        ),
        "failure_code" => super::failure::approved_failure_code(value),
        "images" | "retained_count" => integer(value),
        _ => false,
    }
}

fn integer(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

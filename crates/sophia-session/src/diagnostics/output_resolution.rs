//! Output resolution evidence (t310), reduced before any other rule.
//!
//! A resolution record is admitted only with a known phase and status, an
//! adjustment record only with a known phase and adjustment reason; anything
//! else is dropped whole, so chatter and misspelled statuses cannot ride this
//! route. In an admitted record every value is an unsigned integer or a fixed
//! word. A repeated key keeps its first value, and free-form text -- a
//! connector, an error, a path or a profile -- is never copied.
use sophia_config::DesktopOutputAdjustmentReason as Reason;

const RESOLUTION: &str = "sophia_live_output_resolution";
const ADJUSTMENT: &str = "sophia_live_output_adjustment";
/// More fields than either producer prints; the rest are not read.
const FIELD_LIMIT: usize = 16;

pub(super) fn record(name: &str) -> bool {
    matches!(name, RESOLUTION | ADJUSTMENT)
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
    let value = |wanted: &str| {
        pairs
            .iter()
            .find(|(key, _, _)| *key == wanted)
            .map(|(_, value, _)| *value)
    };
    value("phase").filter(|phase| matches!(*phase, "startup" | "runtime"))?;
    match name {
        RESOLUTION => value("status").filter(|status| resolution_status(status))?,
        ADJUSTMENT => value("reason").filter(|reason| adjustment_reason(reason))?,
        _ => return None,
    };
    let mut result = name.to_owned();
    for (key, value, field) in pairs {
        if field_allowed(name, key, value) {
            result.push(' ');
            result.push_str(field);
        }
    }
    Some(result)
}

fn resolution_status(value: &str) -> bool {
    matches!(
        value,
        "waiting" | "resolved" | "committed" | "refused" | "construction_refused" | "uncommitted"
    )
}

fn field_allowed(name: &str, key: &str, value: &str) -> bool {
    match (name, key) {
        (_, "schema") => value == "1",
        (_, "phase") => matches!(value, "startup" | "runtime"),
        (RESOLUTION, "status") => resolution_status(value),
        (RESOLUTION, "reason") => matches!(
            value,
            "presented_settings_differ"
                | "profile_changed"
                | "hardware"
                | "unavailable"
                | "stale"
                | "none"
        ),
        (
            RESOLUTION,
            "generation" | "transition" | "notice" | "owner" | "outputs" | "adjustments"
            | "attempt",
        ) => integer(value),
        (ADJUSTMENT, "reason") => adjustment_reason(value),
        (ADJUSTMENT, "head" | "output") => integer(value),
        _ => false,
    }
}

fn integer(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

/// Lists every adjustment reason exactly once. The exhaustive match fails to
/// compile when configuration adds a reason this list does not name, so the
/// admitted vocabulary cannot drift from the enum the producer formats. The
/// enum's derived Debug prints a unit variant's name, which `stringify!` gives.
macro_rules! reasons {
    ($($variant:ident),* $(,)?) => {
        const REASON_NAMES: &[&str] = &[$(stringify!($variant)),*];

        #[allow(dead_code)]
        fn listed(reason: &Reason) {
            match reason {
                $(Reason::$variant)|* => {}
            }
        }
    };
}

reasons!(
    Mode,
    Scale,
    Transform,
    Vrr,
    Position,
    Unavailable,
    MirrorUnavailable,
    Fallback,
);

/// Whether `value` is exactly the Debug name of an adjustment reason.
fn adjustment_reason(value: &str) -> bool {
    REASON_NAMES.contains(&value)
}

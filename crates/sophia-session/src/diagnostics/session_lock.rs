//! Session lock evidence (t297), reduced before any other rule.
//!
//! A lock record is admitted only with a known status and is otherwise dropped
//! whole, so a misspelled or new status cannot ride the numeric filter. In an
//! admitted record every value is an unsigned integer or a fixed word: the
//! lock's state, what asked for it, why it was refused and how an attempt was
//! judged. An error, an outcome dump or any other free text is never copied.

const LOCK: &str = "sophia_live_session_lock";
/// More fields than any producer prints; the rest are not read.
const FIELD_LIMIT: usize = 12;

pub(super) fn record(name: &str) -> bool {
    name == LOCK
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
    pairs
        .iter()
        .find(|(key, _, _)| *key == "status")
        .filter(|(_, value, _)| status(value))?;
    let mut result = name.to_owned();
    for (key, value, field) in pairs {
        if field_allowed(key, value) {
            result.push(' ');
            result.push_str(field);
        }
    }
    Some(result)
}

/// Every status the lock phase, coverage, input and authenticator supervisor
/// print. A source guard keeps this list equal to the producers.
fn status(value: &str) -> bool {
    matches!(
        value,
        "locking"
            | "locked"
            | "already_locked"
            | "covered"
            | "key_held"
            | "checking"
            | "failed"
            | "stale_verdict"
            | "unavailable"
            | "unlocking"
            | "unlocked"
            | "refused"
            | "repaint_deferred"
            | "unlock_repaint_failed"
            | "authenticator_ready"
            | "authenticator_unavailable"
            | "authenticator_failed"
    )
}

fn field_allowed(key: &str, value: &str) -> bool {
    match key {
        "schema" => value == "1",
        "status" => status(value),
        // The literals `begin_session_lock!` is invoked with.
        "source" => matches!(value, "shortcut" | "proof" | "unlock_repaint_failed"),
        // Fixed refusal words, and `SessionLockError`'s Debug name.
        "reason" => matches!(
            value,
            "no_authenticator" | "no_native_presentation" | "lock_input" | "EpochExhausted"
        ),
        "verdict" => matches!(value, "Accepted" | "Rejected" | "Unavailable"),
        "epoch" | "input_epoch" | "topology_epoch" | "outputs" | "heads" | "attempt"
        | "revoked_leases" | "device" => integer(value),
        _ => false,
    }
}

fn integer(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<u64>().is_ok()
}

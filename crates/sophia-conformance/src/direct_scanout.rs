//! Evidence verification that shares a vocabulary with the emitter.
//!
//! The shell verifier this replaces extracted fields with `grep -oE` and knew
//! the verdict columns by having them written out. That is the drift this
//! removes: the column names come from `DirectScanoutVerdict::VERDICTS`, so a
//! verdict added without a column is a compile error rather than a silently
//! missing number. The same class of bug produced a nine-slot histogram
//! against eleven verdicts, which built clean and would have panicked at the
//! index.

use sophia_engine::DirectScanoutVerdict;
use std::collections::BTreeSet;

pub fn verify_logs(logs: &[String]) -> Result<Vec<String>, String> {
    direct_scanout(logs)
}

/// One session's direct-scanout counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Counters {
    attempts: usize,
    flips: usize,
    tests: usize,
    test_rejections: usize,
    refusals: usize,
    unsupported: usize,
    fallbacks: usize,
}

/// One counter's name and where it lands.
type CounterField = (&'static str, fn(&mut Counters) -> &mut usize);

impl Counters {
    const FIELDS: [CounterField; 7] = [
        ("direct_scanout_attempts", |counters| &mut counters.attempts),
        ("direct_scanout_flips", |counters| &mut counters.flips),
        ("direct_scanout_tests", |counters| &mut counters.tests),
        ("direct_scanout_test_rejections", |counters| {
            &mut counters.test_rejections
        }),
        ("direct_scanout_refusals", |counters| &mut counters.refusals),
        ("direct_scanout_unsupported", |counters| {
            &mut counters.unsupported
        }),
        ("direct_scanout_fallbacks", |counters| {
            &mut counters.fallbacks
        }),
    ];

    fn from_record(record: &str) -> Result<Self, String> {
        let mut counters = Self::default();
        for (name, field) in Self::FIELDS {
            let value = field_value(record, name)
                .ok_or_else(|| format!("resource record is missing {name}"))?;
            *field(&mut counters) = value
                .parse()
                .map_err(|_| format!("{name} is not numeric: {value}"))?;
        }
        Ok(counters)
    }

    fn add(&mut self, other: Self) {
        self.attempts += other.attempts;
        self.flips += other.flips;
        self.tests += other.tests;
        self.test_rejections += other.test_rejections;
        self.refusals += other.refusals;
        self.unsupported += other.unsupported;
        self.fallbacks += other.fallbacks;
    }

    /// What a session's counters must be true of each other, whatever they say.
    fn check(self, log: &str) -> Result<(), String> {
        // A refusal is Engine's proof disagreeing with the pixels that proof
        // was computed from. There is no benign nonzero value: an ineligible
        // frame never becomes an attempt at all.
        if self.refusals != 0 {
            return Err(format!(
                "the eligibility proof disagreed with the frame it lowered ({} times): {log}",
                self.refusals
            ));
        }
        if self.flips + self.fallbacks + self.unsupported > self.attempts {
            return Err(format!(
                "more direct attempts settled than were made: {log}"
            ));
        }
        if self.test_rejections > self.tests {
            return Err(format!(
                "more validating commits were refused than issued: {log}"
            ));
        }
        // A client buffer reaching a plane without the driver having been
        // asked is the one failure this row exists to make impossible.
        if self.flips > 0 && self.tests == 0 {
            return Err(format!(
                "a client buffer reached a plane with no validating commit: {log}"
            ));
        }
        Ok(())
    }
}

fn field_value<'a>(record: &'a str, name: &str) -> Option<&'a str> {
    record
        .split_whitespace()
        .filter_map(|field| field.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

pub(crate) fn reject_duplicate_fields(record: &str, kind: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for name in record
        .split_whitespace()
        .filter_map(|field| field.split_once('=').map(|(name, _)| name))
    {
        if !seen.insert(name) {
            return Err(format!("{kind} record repeats field {name}"));
        }
    }
    Ok(())
}

fn verdicts_from_record(record: &str) -> Result<[usize; DirectScanoutVerdict::COUNT], String> {
    let mut counts = [0usize; DirectScanoutVerdict::COUNT];
    for (verdict, count) in DirectScanoutVerdict::VERDICTS.iter().zip(&mut counts) {
        let name = verdict.reduced_name();
        let value =
            field_value(record, name).ok_or_else(|| format!("verdict record is missing {name}"))?;
        *count = value
            .parse()
            .map_err(|_| format!("verdict {name} is not numeric: {value}"))?;
    }
    Ok(counts)
}

fn direct_scanout(logs: &[String]) -> Result<Vec<String>, String> {
    if logs.is_empty() {
        return Err("no session logs were given".to_owned());
    }
    let mut totals = Counters::default();
    let mut verdict_totals = [0usize; DirectScanoutVerdict::COUNT];
    let mut per_head = Vec::new();
    let mut geometry = Vec::new();
    let mut sessions = 0usize;
    let mut episode_sessions = 0usize;

    for log in logs {
        let text = std::fs::read_to_string(log)
            .map_err(|error| format!("could not read {log}: {error}"))?;
        if text.trim().is_empty() {
            continue;
        }
        sessions += 1;

        let resources = last_record(
            &text,
            "sophia_live_native_resources schema=12 status=complete ",
        )
        .ok_or_else(|| {
            format!(
                "session reported no schema-12 resource record, so it did not run this build: {log}"
            )
        })?;
        reject_duplicate_fields(resources, "resource")
            .map_err(|error| format!("{error}: {log}"))?;
        let counters =
            Counters::from_record(resources).map_err(|error| format!("{error}: {log}"))?;
        counters.check(log)?;
        totals.add(counters);

        let verdicts = last_record(
            &text,
            "sophia_live_direct_scanout_verdicts schema=2 status=complete ",
        )
        .ok_or_else(|| format!("session reported no eligibility verdicts: {log}"))?;
        reject_duplicate_fields(verdicts, "verdict").map_err(|error| format!("{error}: {log}"))?;
        let counts = verdicts_from_record(verdicts).map_err(|error| format!("{error}: {log}"))?;
        for (total, count) in verdict_totals.iter_mut().zip(counts) {
            *total += count;
        }

        for line in text.lines() {
            // Per-head verdicts come through `tracing` and carry its prefix.
            if let Some(head) = record_after_marker(
                line,
                "sophia_live_direct_scanout_verdicts schema=2 status=head ",
            ) {
                per_head.push(head.trim().to_owned());
            }
            // Geometry records come through `tracing` and carry its prefix.
            if let Some(measured) =
                record_after_marker(line, "sophia_live_direct_scanout_geometry schema=2 ")
            {
                geometry.push(measured.trim().to_owned());
            }
        }

        if check_episode_order(&text, log)? {
            episode_sessions += 1;
        }
    }

    if sessions == 0 {
        return Err("no session produced evidence".to_owned());
    }

    // Every attempt emits an `exported` episode record, so attempts without a
    // single session whose episodes were seen means this reader is blind --
    // not that nothing happened. A matcher that cannot find its records does
    // not fail; it passes, or refuses, vacuously, and reports neither.
    //
    // This is the rule that was missing. `episode_sessions=0` sat in every
    // gate summary from archive 0001 onward while the episode-order checks
    // never once ran against hardware evidence, because the records arrive
    // decorated by `tracing` and the reader was anchored to the line start.
    if totals.attempts > 0 && episode_sessions == 0 {
        return Err(format!(
            "{} direct-scanout attempts produced no readable episode records across {sessions} sessions: the reader is not matching this evidence",
            totals.attempts
        ));
    }

    let histogram = DirectScanoutVerdict::VERDICTS
        .iter()
        .zip(verdict_totals)
        .map(|(verdict, count)| format!(" {}={count}", verdict.reduced_name()))
        .collect::<String>();
    let mut report = vec![
        format!(
            "sophia_direct_scanout_gate schema=1 sessions={sessions} attempts={} flips={} tests={} test_rejections={} refusals={} unsupported={} fallbacks={} episode_sessions={episode_sessions}",
            totals.attempts,
            totals.flips,
            totals.tests,
            totals.test_rejections,
            totals.refusals,
            totals.unsupported,
            totals.fallbacks,
        ),
        format!("sophia_direct_scanout_verdicts schema=1 sessions={sessions}{histogram}"),
    ];

    if totals.flips == 0 {
        let eligible = verdict_totals[DirectScanoutVerdict::Eligible.reduced_index()];
        let lowered: usize = verdict_totals.iter().sum();
        let mut explanation = vec!["No client buffer reached a plane. Per head:".to_owned()];
        explanation.extend(per_head.iter().map(|head| format!("  {head}")));
        explanation.extend(geometry.iter().map(|line| format!("  measured: {line}")));
        explanation.push(format!(
            "  eligible={eligible} of {lowered} lowered frames across {sessions} sessions."
        ));
        return Err(format!(
            "{}\ndirect scanout never engaged",
            explanation.join("\n")
        ));
    }

    report.push(format!(
        "direct scanout verification passed: {sessions} sessions, {} flips",
        totals.flips
    ));
    Ok(report)
}

/// Verify the single-client, WM-free session shape needed by direct scanout.
pub fn verify_standalone_logs(logs: &[String]) -> Result<Vec<String>, String> {
    verify_standalone_logs_with_overlay(logs, false)
}

/// The same, additionally requiring that an overlay opened over a directly
/// scanned frame and that eligibility returned afterwards.
///
/// Off by default so every existing caller and archive keeps verifying: a run
/// that never opened an overlay is not a failed run, it is a different proof.
pub fn verify_standalone_logs_with_overlay(
    logs: &[String],
    require_overlay: bool,
) -> Result<Vec<String>, String> {
    verify_standalone_logs_with(logs, require_overlay, false)
}

/// The same, additionally requiring both cost populations.
///
/// Off by default so archives written before the instrumentation keep
/// verifying: a run that measured nothing is a different proof, not a failed
/// one. A run that was *asked* to measure and did not is a stale binary,
/// which is a failure.
pub fn verify_standalone_logs_with(
    logs: &[String],
    require_overlay: bool,
    require_cost: bool,
) -> Result<Vec<String>, String> {
    verify_standalone_logs_proving(logs, require_overlay, require_cost, false)
}

/// The same, additionally requiring a cursor that moved over direct frames.
pub fn verify_standalone_logs_proving(
    logs: &[String],
    require_overlay: bool,
    require_cost: bool,
    require_cursor: bool,
) -> Result<Vec<String>, String> {
    if logs.is_empty() {
        return Err("no standalone session logs were given".to_owned());
    }
    for log in logs {
        let text = std::fs::read_to_string(log)
            .map_err(|error| format!("could not read {log}: {error}"))?;
        let session = last_record_at_any(&text, SESSION_COMPLETION_MARKER)
            .ok_or_else(|| format!("the session did not reach a bounded completion: {log}"))?;
        if field_value(session, "wm_policy") != Some("disabled") {
            return Err(format!(
                "a window manager ran; its chrome makes every frame ineligible: {log}"
            ));
        }
        if !text.lines().any(|line| {
            record_after_marker(line, "sophia_live_session_present schema=2 status=retired ")
                .is_some()
        }) {
            return Err(format!("the client never presented a frame: {log}"));
        }
    }
    let mut report = verify_logs(logs)?;
    if require_overlay || require_cost || require_cursor {
        for log in logs {
            let text = std::fs::read_to_string(log)
                .map_err(|error| format!("could not read {log}: {error}"))?;
            if require_overlay {
                report.extend(crate::direct_scanout_overlay::check(&text, log)?);
            }
            if require_cost {
                report.extend(crate::direct_scanout_cost::check(&text, log)?);
            }
            if require_cursor {
                report.extend(crate::direct_scanout_cursor::check(&text, log)?);
            }
        }
    }
    report.extend(
        logs.iter()
            .map(|log| format!("direct scanout standalone probe passed: {log}")),
    );
    Ok(report)
}

/// The last record carrying this marker, from the marker onward.
///
/// Marker-based like every other reader here. These particular records happen
/// to arrive bare today, because the session prints them itself rather than
/// emitting them through `tracing` -- but that is a fact about which printer
/// owns each record, not a property of the evidence, and reading them as
/// though it were guaranteed is what made the episode rules vacuous.
/// The completion this reader accepts, as one expression the schema tripwire
/// can read: the current proof schema and the one before it, because a
/// promoted archive is a recorded session and stays at the schema it had.
/// Evidence too old to carry a field is refused by the field check, never by
/// the schema -- this reader used to name a single schema and refused every
/// archive the moment the emitter moved on, while the tripwire read the
/// single number as correct.
const SESSION_COMPLETION_MARKER: &str = "sophia_live_session schema=(16|18) ";

/// `last_record` over a marker whose one `(a|b|..)` group names the schemas
/// accepted, expanded into the literal markers a substring match needs.
fn last_record_at_any<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let (Some(open), Some(close)) = (marker.find('('), marker.find(')')) else {
        return last_record(text, marker);
    };
    let (head, tail) = (&marker[..open], &marker[close + 1..]);
    marker[open + 1..close]
        .split('|')
        .find_map(|schema| last_record(text, &format!("{head}{schema}{tail}")))
}

fn last_record<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.lines()
        .rev()
        .find_map(|line| record_after_marker(line, marker))
}

/// The remainder of a record line after its marker, wherever the marker sits.
///
/// Records emitted through `tracing` carry a timestamp and ANSI colour ahead
/// of the marker in a verbose session log; records emitted through the
/// session's own printer start at column zero. A reader that anchors to the
/// line start silently sees only the second kind, and a rule that sees no
/// records passes -- or refuses -- vacuously.
pub(crate) fn record_after_marker<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.find(marker).map(|start| &line[start + marker.len()..])
}

/// Readers that must stay anchored, and why.
///
/// `profile.rs` reads a captured child's stdout, which is bare by
/// construction -- no `tracing` layer decorates it, and matching mid-line
/// there would accept a record quoted inside another line. Everything that
/// reads a *session log* must be marker-based; `cargo xtask check` enforces
/// that with an allowlist naming this one exception.
pub const ANCHORED_READER_ALLOWLIST: [&str; 1] = ["profile.rs"];

/// Whether the episode records are in a lawful order, and whether there were
/// any. The counters cannot rule out a flip for a scene that was never
/// exported, or a validating commit that passed for one.
fn check_episode_order(text: &str, log: &str) -> Result<bool, String> {
    let mut exported = false;
    let mut seen = false;
    for line in text.lines() {
        let Some(rest) = record_after_marker(line, "sophia_live_direct_scanout schema=1 status=")
        else {
            continue;
        };
        seen = true;
        let status = rest.split_whitespace().next().unwrap_or_default();
        match status {
            "exported" => exported = true,
            "test_passed" => {
                if !exported {
                    return Err(format!(
                        "a validating commit passed for a scene never exported: {log}"
                    ));
                }
            }
            "flipped" => {
                if !exported {
                    return Err(format!(
                        "a direct flip happened for a scene never exported: {log}"
                    ));
                }
                exported = false;
            }
            "fell_back" | "test_rejected" => exported = false,
            _ => {}
        }
    }
    Ok(seen)
}

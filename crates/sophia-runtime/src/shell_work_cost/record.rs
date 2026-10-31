use super::{BOUNDARIES, CLASSES, STAGES, state::State};
use std::fmt::Write;

const COMMON: &[&str] = &[
    "schema",
    "tid",
    "thread_sequence",
    "snapshot",
    "final",
    "observed_monotonic_usec",
    "cpu_first_nsec",
    "cpu_now_nsec",
    "clock_reads",
    "monotonic_reads",
    "clock_regressions",
    "saturations",
    "valid",
];
const SUMMARY: &[&str] = &[
    "service_turns",
    "service_empty",
    "service_records",
    "flush_turns",
    "flush_empty",
    "flush_records",
    "staged_attempts",
    "staged_errors",
    "ring_attempts",
    "ring_errors",
    "ack_attempts",
    "ack_errors",
    "staged_writes",
    "staged_refused",
    "ring_writes",
    "ring_refused",
    "ack_writes",
    "ack_refused",
    "jobs",
    "job_errors",
    "job_bytes",
    "server_frame_permits",
];
const STAGE: &[&str] = &[
    "inclusive",
    "calls",
    "errors",
    "empty",
    "aborted",
    "cpu_nsec",
    "max_nsec",
    "bytes",
    "ok_bytes",
];
const BOUNDARY: &[&str] = &[
    "export_polls",
    "file_checks",
    "custody_checks",
    "image_process_checks",
];

const CLASS: &[&str] = &[
    "decoded",
    "admitted",
    "service_records",
    "service_turns",
    "flush_records",
    "flush_turns",
];

pub(super) fn rows(
    s: &State,
    cpu: u64,
    tid: u64,
    mono: u64,
    worker: bool,
    final_record: bool,
) -> Vec<String> {
    let common = format!(
        "sophia_shell_work_cost schema=1 role={} tid={tid} thread_sequence={} snapshot={} final={} observed_monotonic_usec={mono} cpu_first_nsec={} cpu_now_nsec={cpu} clock_reads={} monotonic_reads={} clock_regressions={} saturations={} valid={}",
        if worker { "validation" } else { "owner" },
        s.thread_sequence,
        s.snapshots,
        u8::from(final_record),
        s.first.unwrap_or(cpu),
        s.reads,
        s.snapshots,
        s.regressions,
        s.saturations,
        u8::from(s.valid())
    );
    let values = [
        s.turns[0],
        s.empty_turns[0],
        s.turn_records[0],
        s.turns[1],
        s.empty_turns[1],
        s.turn_records[1],
        s.decodes[0],
        s.decode_errors[0],
        s.decodes[1],
        s.decode_errors[1],
        s.decodes[2],
        s.decode_errors[2],
        s.admissions[0],
        s.admission_errors[0],
        s.admissions[1],
        s.admission_errors[1],
        s.admissions[2],
        s.admission_errors[2],
        s.jobs,
        s.job_errors,
        s.job_bytes,
        s.server_frame_permits,
    ];
    let mut summary = format!("{common} kind=summary");
    for (key, value) in SUMMARY.iter().zip(values) {
        write!(summary, " {key}={value}").expect("String write");
    }
    let mut rows = vec![summary];
    for (name, stage) in STAGES.iter().zip(&s.stages) {
        rows.push(format!("{common} kind=stage stage={name} parent={} inclusive=1 calls={} errors={} empty={} aborted={} cpu_nsec={} max_nsec={} bytes={} ok_bytes={}", parent(name), stage.calls, stage.errors, stage.empty, stage.aborted, stage.cpu, stage.maximum, stage.bytes, stage.ok_bytes));
    }
    for (name, calls) in BOUNDARIES.iter().zip(&s.calls) {
        rows.push(format!("{common} kind=boundary boundary={name} export_polls={} file_checks={} custody_checks={} image_process_checks={}", calls.poll, calls.check, calls.custody, calls.image_alive));
    }
    for (name, class) in CLASSES.iter().zip(&s.classes) {
        rows.push(format!("{common} kind=record_class class={name} direction=incoming decoded={} admitted={} service_records={} service_turns={} flush_records={} flush_turns={}", class.decoded, class.admitted, class.records[0], class.turns[0], class.records[1], class.turns[1]));
    }
    rows
}

/// Exact vocabulary; valid=0 stays visible but cannot support a cost/rate.
pub fn valid_record(line: &str) -> bool {
    let mut fields = line.split_whitespace();
    if fields.next() != Some("sophia_shell_work_cost") {
        return false;
    }
    let mut values = std::collections::BTreeMap::new();
    for field in fields {
        let Some((key, value)) = field.split_once('=') else {
            return false;
        };
        if values.insert(key, value).is_some() {
            return false;
        }
    }
    let (extra, named) = match values.get("kind").copied() {
        Some("summary") => (SUMMARY, 2),
        Some("stage") => {
            let Some(stage) = values.get("stage").copied() else {
                return false;
            };
            if !STAGES.contains(&stage)
                || values.get("parent") != Some(&parent(stage))
                || values.get("inclusive") != Some(&"1")
            {
                return false;
            }
            (STAGE, 4)
        }
        Some("boundary") => {
            if !values
                .get("boundary")
                .is_some_and(|v| BOUNDARIES.contains(v))
            {
                return false;
            }
            (BOUNDARY, 3)
        }
        Some("record_class") => {
            if !values.get("class").is_some_and(|v| CLASSES.contains(v))
                || values.get("direction") != Some(&"incoming")
            {
                return false;
            }
            (CLASS, 4)
        }
        _ => return false,
    };
    if values.len() != COMMON.len() + extra.len() + named
        || !matches!(values.get("role"), Some(&"owner" | &"validation"))
        || values.get("schema") != Some(&"1")
        || !matches!(values.get("final"), Some(&"0" | &"1"))
        || !matches!(values.get("valid"), Some(&"0" | &"1"))
    {
        return false;
    }
    COMMON.iter().chain(extra).all(|key| {
        values.get(key).is_some_and(|value| {
            !value.is_empty()
                && value.bytes().all(|b| b.is_ascii_digit())
                && value.parse::<u64>().is_ok()
        })
    })
}

fn parent(stage: &str) -> &str {
    match stage {
        "copy" | "pixels" => "validate",
        "allocate" | "read" => "copy",
        "reserve" => "admission",
        "collect" => "retire_or_other",
        "remove" => "collect_or_confirm",
        _ => "none",
    }
}

//! Local owner for one prepared desktop-comparison schedule row.
//!
//! Stack startup and tracefs privilege remain narrow OS adapters. This owner
//! admits an owner-only session attestation, launches the repository workload,
//! samples the common process population, normalizes kernel DRM completion
//! events, and binds only a replayable raw attempt.

mod crtc;
mod qualification;
mod reference;
mod trace;
mod visibility;
mod workload;

pub use qualification::qualify;
pub use reference::install_reference;
use trace::{TraceOwner, probe_tracefs};
use visibility::{VisibilityProbe, format_record, process_descends_from};
use workload::WorkloadOwner;
pub use workload::run_stream;

use super::{
    FIREFOX_VERSION, KITTY_VERSION, NIRI_VERSION, ScheduledSample, TOPOLOGY, XLIBRE_COMMIT,
    XMONAD_CONTRIB_VERSION, XMONAD_VERSION, bind_attempt, current_uid, next_scheduled,
    source_commit, status, verify_host_tool_versions,
};
use sha2::Digest as _;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// A fixed set of resource snapshots beside the window/serial pairs observed
/// with them.
type ResourceSnapshotsWithWindows<const N: usize> = ([ResourceSnapshot; N], Vec<(u32, u64)>);

const SESSION_PREFIX: &str = "desktop_comparison_session schema=1 status=ready ";
const RESOURCE_INTERVAL: Duration = Duration::from_secs(1);
const READY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionAttestation {
    stack: String,
    stack_version: String,
    topology: String,
    supervisor_pid: u32,
    supervisor_start_ticks: u64,
    crtc: u64,
    native_timing: String,
    native_source: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ResourceSnapshot {
    processes: u64,
    pss_kib: u64,
    rss_kib: u64,
    anonymous_kib: u64,
    private_dirty_kib: u64,
    cpu_ticks: u64,
    minor_faults: u64,
    major_faults: u64,
    threads: u64,
    fds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StackProcessIdentity {
    label: &'static str,
    pid: u32,
    start_ticks: u64,
    executable: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ProcStat {
    ppid: u32,
    start_ticks: u64,
    cpu_ticks: u64,
    minor_faults: u64,
    major_faults: u64,
    threads: u64,
}

pub fn attest_session_auto(
    repo: &Path,
    run: &Path,
    supervisor_pid: u32,
) -> Result<Vec<String>, String> {
    let crtc = crtc::resolve_dp1_crtc()?;
    attest_session(repo, run, supervisor_pid, crtc)
}

pub fn attest_session(
    repo: &Path,
    run: &Path,
    supervisor_pid: u32,
    crtc: u64,
) -> Result<Vec<String>, String> {
    let _ = status(repo, run)?;
    let scheduled = next_scheduled(run)?
        .ok_or_else(|| "desktop comparison matrix is already complete".to_owned())?;
    let proc_root = PathBuf::from(format!("/proc/{supervisor_pid}"));
    let proc_status = fs::read_to_string(proc_root.join("status"))
        .map_err(|error| format!("could not inspect session supervisor: {error}"))?;
    let uid = proc_status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_ascii_whitespace().next())
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("session supervisor does not expose a numeric UID")?;
    if uid != current_uid()? {
        return Err("session supervisor is not owned by the invoking user".to_owned());
    }
    let executable = fs::read_link(proc_root.join("exe"))
        .map_err(|error| format!("could not identify session supervisor executable: {error}"))?;
    let executable_name = executable
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("session supervisor executable name is not UTF-8")?;
    let expected_names: &[&str] = match scheduled.stack.as_str() {
        "sophia" => &["sophia"],
        "xlibre-xmonad" => &["Xorg", "XLibre"],
        "niri" => &["niri"],
        _ => return Err("prepared schedule contains an unknown stack".to_owned()),
    };
    if !expected_names.contains(&executable_name) {
        return Err(format!(
            "session supervisor executable {executable_name:?} does not match stack {}",
            scheduled.stack
        ));
    }
    let identity_field = match scheduled.stack.as_str() {
        "sophia" => "sophia_sha256",
        "xlibre-xmonad" => "xlibre_sha256",
        "niri" => "niri_sha256",
        _ => unreachable!(),
    };
    require_executable_digest(&executable, &manifest_identity(run, identity_field)?)?;
    if scheduled.stack == "xlibre-xmonad" {
        let prefix = executable
            .parent()
            .and_then(Path::parent)
            .ok_or("XLibre executable does not live under a versioned prefix")?;
        let identity =
            fs::read_to_string(prefix.join("share/sophia-desktop-comparison/xlibre-commit"))
                .map_err(|error| format!("XLibre source identity is missing: {error}"))?;
        if identity.trim() != XLIBRE_COMMIT {
            return Err("XLibre source identity does not match the pinned commit".to_owned());
        }
    }
    let stat = fs::read_to_string(proc_root.join("stat"))
        .map_err(|error| format!("could not read session supervisor start identity: {error}"))?;
    let start_ticks = parse_proc_stat(&stat)?.start_ticks;
    let stack_version = match scheduled.stack.as_str() {
        "sophia" => source_commit(run)?,
        "xlibre-xmonad" => XLIBRE_COMMIT.to_owned(),
        "niri" => NIRI_VERSION.to_owned(),
        _ => unreachable!(),
    };
    let native_source = match scheduled.stack.as_str() {
        "sophia" => "sophia-engine",
        "xlibre-xmonad" => "x-present",
        "niri" => "presentation-time",
        _ => unreachable!(),
    };
    let path = session_attestation_path()?;
    let parent = path
        .parent()
        .ok_or("session attestation path has no parent")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create session attestation directory: {error}"))?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not protect session attestation directory: {error}"))?;
    let partial = parent.join(format!("session.kdl.{supervisor_pid}.partial"));
    write_new(
        &partial,
        format!(
            "desktop_comparison_session schema=1 status=ready stack={} stack_version={} topology={} supervisor_pid={} supervisor_start_ticks={} crtc={} native_timing=not_exposed native_source={}\n",
            scheduled.stack,
            stack_version,
            TOPOLOGY,
            supervisor_pid,
            start_ticks,
            crtc,
            native_source,
        )
        .as_bytes(),
    )?;
    fs::set_permissions(&partial, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("could not protect session attestation: {error}"))?;
    fs::rename(&partial, &path)
        .map_err(|error| format!("could not publish session attestation: {error}"))?;
    Ok(vec![format!(
        "desktop_comparison_attest schema=1 status=ready order={} stack={} supervisor_pid={} crtc={} path={}",
        scheduled.order,
        scheduled.stack,
        supervisor_pid,
        crtc,
        path.display(),
    )])
}

pub fn preflight(repo: &Path, run: &Path) -> Result<Vec<String>, String> {
    let _ = status(repo, run)?;
    let scheduled = next_scheduled(run)?
        .ok_or_else(|| "desktop comparison matrix is already complete".to_owned())?;
    let attestation_path = session_attestation_path()?;
    let attestation = read_attestation(&attestation_path)?;
    validate_session(run, &scheduled, &attestation)?;
    validate_active_profile(repo, run, &attestation)?;
    verify_host_tool_versions()?;
    let helper = repo.join("tools/desktop_comparison_tracefs.sh");
    if !helper.is_file() {
        return Err(format!(
            "kernel timing adapter is missing: {}",
            helper.display()
        ));
    }
    let tracefs = probe_tracefs(&helper)?;
    Ok(vec![format!(
        "desktop_comparison_preflight schema=1 status=ready order={} stack={} workload={} repetition={} session={} tracefs={} frame_source=kernel_drm",
        scheduled.order,
        scheduled.stack,
        scheduled.workload,
        scheduled.repetition,
        attestation_path.display(),
        tracefs.display(),
    )])
}

pub fn capture_next(repo: &Path, run: &Path) -> Result<Vec<String>, String> {
    preflight(repo, run)?;
    let scheduled = next_scheduled(run)?
        .ok_or_else(|| "desktop comparison matrix is already complete".to_owned())?;
    let attestation = read_attestation(&session_attestation_path()?)?;
    let incoming = run.join("incoming");
    if incoming.exists()
        && fs::read_dir(&incoming)
            .map_err(|error| format!("could not inspect pending captures: {error}"))?
            .next()
            .is_some()
    {
        return Err(format!(
            "a partial capture needs diagnosis before retry: {}",
            incoming.display()
        ));
    }
    // Cursor qualification belongs to the live session, not the durable run.
    // Admit and snapshot it before creating a partial or starting the timed
    // measurement so runtime cleanup cannot invalidate an otherwise complete
    // row after the workload has run.
    let qualification = qualification::measurement_fields(run, &scheduled, &attestation)?;
    fs::create_dir_all(&incoming)
        .map_err(|error| format!("could not create pending capture root: {error}"))?;
    protect_owner_directory(&incoming)?;
    let attempt = incoming.join(format!(
        "{:02}-{}-{}-{}.partial",
        scheduled.order, scheduled.stack, scheduled.workload, scheduled.repetition
    ));
    fs::create_dir(&attempt)
        .map_err(|error| format!("could not create pending capture: {error}"))?;
    protect_owner_directory(&attempt)?;

    let measured = measure(
        repo,
        run,
        &attempt,
        &scheduled,
        &attestation,
        &qualification,
    );
    match measured {
        Ok(()) => Ok(vec![format!(
            "desktop_comparison_capture schema=2 status=staged order={} stack={} workload={} repetition={} path={}",
            scheduled.order,
            scheduled.stack,
            scheduled.workload,
            scheduled.repetition,
            attempt.display(),
        )]),
        Err(error) => Err(format!(
            "{error}; partial capture retained at {}",
            attempt.display()
        )),
    }
}

/// Seal a staged row only after its graphical supervisor has exited.
///
/// Capture cannot truthfully attest teardown while it is still running under
/// that stack. The TTY adapter calls this after Xorg, niri, or Sophia has
/// returned and restored the operator terminal.
pub fn finalize_next(repo: &Path, run: &Path) -> Result<Vec<String>, String> {
    let scheduled = next_scheduled(run)?
        .ok_or_else(|| "desktop comparison matrix is already complete".to_owned())?;
    let attestation = read_attestation(&session_attestation_path()?)?;
    if attestation.stack != scheduled.stack {
        return Err("staged comparison session does not match the next row".to_owned());
    }
    if supervisor_identity_is_live(&attestation)? {
        return Err("comparison teardown cannot finalize while its supervisor is alive".to_owned());
    }
    let incoming = run.join("incoming");
    let entries = fs::read_dir(&incoming)
        .map_err(|error| format!("could not inspect staged comparison capture: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not inspect staged comparison capture: {error}"))?;
    if entries.len() != 1
        || !entries[0]
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
    {
        return Err("comparison finalization requires exactly one staged capture".to_owned());
    }
    let attempt = entries[0].path();
    let source = fs::read_to_string(attempt.join("measurement.kdl"))
        .map_err(|error| format!("staged comparison measurement is missing: {error}"))?;
    let finalized = finalize_measurement_record(&source, &scheduled)?;
    write_new(&attempt.join("attempt.kdl"), finalized.as_bytes())?;
    fs::remove_file(attempt.join("measurement.kdl"))
        .map_err(|error| format!("could not retire staged measurement: {error}"))?;
    let result = bind_attempt(repo, run, &attempt)?;
    fs::remove_dir_all(&attempt)
        .map_err(|error| format!("capture bound but staged copy was not removed: {error}"))?;
    Ok(result)
}

pub(super) fn finalize_measurement_record(
    source: &str,
    scheduled: &ScheduledSample,
) -> Result<String, String> {
    let measurement = one_record(
        source,
        "desktop_comparison_measurement schema=1 status=complete ",
    )?;
    let fields = record_fields(measurement)?;
    for (name, expected) in [
        ("order", scheduled.order.to_string()),
        ("stack", scheduled.stack.clone()),
        ("workload", scheduled.workload.clone()),
        ("repetition", scheduled.repetition.to_string()),
    ] {
        if fields.get(name) != Some(&expected) {
            return Err(format!("staged comparison measurement mismatches {name}"));
        }
    }
    if fields.contains_key("teardown") || fields.contains_key("supervisor_exited") {
        return Err("live measurement cannot predeclare teardown state".to_owned());
    }
    let body = measurement
        .strip_prefix("desktop_comparison_measurement schema=1 status=complete ")
        .expect("measurement prefix checked above");
    Ok(format!(
        "desktop_comparison_attempt schema=3 status=measured {body} teardown=clean supervisor_exited=true\n"
    ))
}

fn measure(
    repo: &Path,
    run: &Path,
    attempt: &Path,
    scheduled: &ScheduledSample,
    attestation: &SessionAttestation,
    qualification: &str,
) -> Result<(), String> {
    let duration = if scheduled.workload == "soak-2h" {
        Duration::from_secs(2 * 60 * 60)
    } else {
        Duration::from_secs(60)
    };
    if process_descends_from(std::process::id(), attestation.supervisor_pid)? {
        return Err(
            "comparison capture controller must run outside the measured supervisor tree"
                .to_owned(),
        );
    }
    let mut visibility = VisibilityProbe::connect(&scheduled.stack)?;
    let baseline_visibility = visibility.observe(&[])?;
    baseline_visibility.require_empty()?;
    let mut visibility_log = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(attempt.join("visibility.log"))
        .map_err(|error| format!("could not create visibility evidence: {error}"))?;
    visibility_log
        .write_all(format_record("baseline", 0, 0, baseline_visibility).as_bytes())
        .map_err(|error| format!("could not write visibility baseline: {error}"))?;

    let raw_trace = attempt.join("kernel-trace.raw");
    let mut trace = TraceOwner::start(
        &repo.join("tools/desktop_comparison_tracefs.sh"),
        attempt,
        &raw_trace,
    )?;
    let mut workload = WorkloadOwner::launch(repo, attempt, scheduled, duration)?;
    let workload_identities = workload.root_identities()?;
    for identity in &workload_identities {
        if process_descends_from(identity.pid(), attestation.supervisor_pid)? {
            return Err(
                "comparison workload launcher entered the measured supervisor tree".to_owned(),
            );
        }
    }
    let settled_visibility = visibility.wait_visible(&workload_identities, READY_TIMEOUT)?;
    workload.mark_visible();
    visibility_log
        .write_all(format_record("settled", 0, 0, settled_visibility).as_bytes())
        .map_err(|error| format!("could not write settled visibility evidence: {error}"))?;

    trace.begin()?;
    workload.begin_measured_work()?;

    let started = Instant::now();
    let clock_ticks = clock_ticks_per_second()?;
    let stack_identities = required_stack_identities(repo, run, scheduled, attestation)?;
    let stack_roots = stack_identities
        .iter()
        .map(|identity| identity.pid)
        .collect::<BTreeSet<_>>();
    let workload_roots = workload.root_pids().collect::<BTreeSet<_>>();
    let roots = stack_roots
        .union(&workload_roots)
        .copied()
        .collect::<BTreeSet<_>>();
    let ([baseline, stack_baseline, workload_baseline], observed_workload) =
        sample_process_populations(
            Path::new("/proc"),
            [&roots, &stack_roots, &workload_roots],
            &workload,
        )?;
    workload.retain_processes(observed_workload);
    let mut resources = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(attempt.join("resources.log"))
        .map_err(|error| format!("could not create resource evidence: {error}"))?;
    let samples = duration.as_secs();
    for sequence in 1..=samples {
        let due = started + RESOURCE_INTERVAL.saturating_mul(sequence as u32);
        sleep_until(due);
        validate_stack_identities(&stack_identities)?;
        if workload.exited_early()? {
            return Err("comparison workload exited before its measured window".to_owned());
        }
        let monotonic_usec = elapsed_micros(started);
        let observed_visibility = visibility.observe(&workload_identities)?;
        observed_visibility.require_visible()?;
        visibility_log
            .write_all(
                format_record("sample", sequence, monotonic_usec, observed_visibility).as_bytes(),
            )
            .map_err(|error| format!("could not append visibility evidence: {error}"))?;
        let ([snapshot, stack_snapshot, workload_snapshot], observed_workload) =
            sample_process_populations(
                Path::new("/proc"),
                [&roots, &stack_roots, &workload_roots],
                &workload,
            )?;
        workload.retain_processes(observed_workload);
        writeln!(
            resources,
            "desktop_comparison_resource schema=2 seq={sequence} monotonic_usec={} processes={} pss_kib={} rss_kib={} anonymous_kib={} private_dirty_kib={} cpu_msec={} minor_faults={} major_faults={} threads={} fds={} stack_processes={} stack_pss_kib={} stack_rss_kib={} stack_cpu_msec={} stack_threads={} stack_fds={} workload_processes={} workload_pss_kib={} workload_rss_kib={} workload_cpu_msec={} workload_threads={} workload_fds={}",
            monotonic_usec,
            snapshot.processes,
            snapshot.pss_kib,
            snapshot.rss_kib,
            snapshot.anonymous_kib,
            snapshot.private_dirty_kib,
            snapshot.cpu_ticks.saturating_sub(baseline.cpu_ticks).saturating_mul(1_000) / clock_ticks,
            snapshot.minor_faults.saturating_sub(baseline.minor_faults),
            snapshot.major_faults.saturating_sub(baseline.major_faults),
            snapshot.threads,
            snapshot.fds,
            stack_snapshot.processes,
            stack_snapshot.pss_kib,
            stack_snapshot.rss_kib,
            stack_snapshot.cpu_ticks.saturating_sub(stack_baseline.cpu_ticks).saturating_mul(1_000) / clock_ticks,
            stack_snapshot.threads,
            stack_snapshot.fds,
            workload_snapshot.processes,
            workload_snapshot.pss_kib,
            workload_snapshot.rss_kib,
            workload_snapshot.cpu_ticks.saturating_sub(workload_baseline.cpu_ticks).saturating_mul(1_000) / clock_ticks,
            workload_snapshot.threads,
            workload_snapshot.fds,
        )
        .map_err(|error| format!("could not append resource evidence: {error}"))?;
    }
    visibility_log
        .sync_all()
        .map_err(|error| format!("could not sync visibility evidence: {error}"))?;
    resources
        .sync_all()
        .map_err(|error| format!("could not sync resource evidence: {error}"))?;

    let duration_msec = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    // The probe is a trusted controller connection, not part of the workload.
    // Close it as soon as the last sample is durable so session quiescence does
    // not depend on later trace normalization or workload cleanup.
    drop(visibility);
    trace.finish()?;
    workload.finish(attempt)?;
    normalize_kernel_trace(
        &raw_trace,
        &attempt.join("kernel-frames.log"),
        attestation.crtc,
    )?;
    fs::remove_file(&raw_trace)
        .map_err(|error| format!("could not remove normalized trace source: {error}"))?;
    write_new(
        &attempt.join("native.log"),
        format!(
            "desktop_comparison_native_timing schema=1 availability={} source={} samples=0\n",
            attestation.native_timing, attestation.native_source,
        )
        .as_bytes(),
    )?;
    let candidate = source_commit(run)?;
    let version = match scheduled.stack.as_str() {
        "sophia" => candidate.as_str(),
        "xlibre-xmonad" => XLIBRE_COMMIT,
        "niri" => NIRI_VERSION,
        _ => return Err("prepared schedule contains an unknown stack".to_owned()),
    };
    write_new(
        &attempt.join("measurement.kdl"),
        format!(
            "desktop_comparison_measurement schema=1 status=complete order={} stack={} workload={} repetition={} backend=native stack_version={} topology={} kitty={} firefox={} duration_msec={} controller_outside_supervisor=true visibility_samples={} crashes=0 sample_loss=0 {qualification}\n",
            scheduled.order,
            scheduled.stack,
            scheduled.workload,
            scheduled.repetition,
            version,
            TOPOLOGY,
            KITTY_VERSION,
            FIREFOX_VERSION,
            duration_msec,
            samples,
        )
        .as_bytes(),
    )
}

include!("capture_owner/attestation.rs");

include!("capture_owner/process_population.rs");

pub(super) fn normalize_kernel_trace(
    source: &Path,
    destination: &Path,
    expected_crtc: u64,
) -> Result<(), String> {
    let trace = fs::read_to_string(source)
        .map_err(|error| format!("could not read kernel timing trace: {error}"))?;
    if trace
        .lines()
        .any(|line| line.contains("LOST") && line.contains("EVENTS"))
    {
        return Err("kernel DRM trace reports lost events".to_owned());
    }
    let mut frames = Vec::<(u64, u64, u64)>::new();
    let mut seen_sequences = BTreeSet::new();
    for line in trace.lines() {
        let Some((prefix, payload)) = line.split_once("drm_vblank_event_delivered:") else {
            continue;
        };
        let crtc = payload
            .split_ascii_whitespace()
            .find_map(|token| token.trim_end_matches(',').strip_prefix("crtc="))
            .and_then(|value| value.parse::<u64>().ok());
        if crtc != Some(expected_crtc) {
            continue;
        }
        let kernel_sequence = payload
            .split_ascii_whitespace()
            .find_map(|token| token.trim_end_matches(',').strip_prefix("seq="))
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or("kernel DRM trace contains an invalid kernel sequence")?;
        let timestamp = prefix
            .split_ascii_whitespace()
            .rev()
            .find_map(|token| token.strip_suffix(':'))
            .and_then(|value| value.parse::<f64>().ok())
            .map(|seconds| (seconds * 1_000_000.0).round() as u64)
            .ok_or("kernel DRM trace contains an invalid event timestamp")?;
        if let Some((previous_sequence, _, deliveries)) = frames.last_mut()
            && *previous_sequence == kernel_sequence
        {
            *deliveries = deliveries.saturating_add(1);
            continue;
        }
        if !seen_sequences.insert(kernel_sequence) {
            return Err("kernel DRM sequence reappeared after a different completion".to_owned());
        }
        if frames
            .last()
            .is_some_and(|(_, previous, _)| *previous >= timestamp)
        {
            return Err("kernel DRM completion timestamps are not strictly monotonic".to_owned());
        }
        frames.push((kernel_sequence, timestamp, 1));
    }
    if frames.len() < 3 {
        return Err(format!(
            "kernel DRM timing population is incomplete: found {} delivered events for CRTC {expected_crtc}",
            frames.len()
        ));
    }
    let mut output = String::new();
    for (index, (kernel_sequence, timestamp, deliveries)) in frames.into_iter().enumerate() {
        output.push_str(&format!(
            "desktop_comparison_kernel_frame schema=2 seq={} crtc={} kernel_sequence={} deliveries={} ust_usec={}\n",
            index + 1,
            expected_crtc,
            kernel_sequence,
            deliveries,
            timestamp,
        ));
    }
    write_new(destination, output.as_bytes())
}

fn timing_population(values: &[u64]) -> (u64, u64, u64, u64) {
    if values.is_empty() {
        return (0, 0, 0, 0);
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let pick = |percentile: usize| {
        let rank = sorted.len().saturating_mul(percentile).saturating_add(99) / 100;
        sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
    };
    (pick(50), pick(95), pick(99), *sorted.last().unwrap_or(&0))
}

fn one_record<'a>(source: &'a str, prefix: &str) -> Result<&'a str, String> {
    let records = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if records.len() != 1 || !records[0].starts_with(prefix) {
        return Err(format!(
            "comparison session requires exactly one {prefix:?} record"
        ));
    }
    Ok(records[0])
}

fn record_fields(line: &str) -> Result<BTreeMap<String, String>, String> {
    let mut fields = BTreeMap::new();
    for token in line.split_ascii_whitespace() {
        let Some((name, value)) = token.split_once('=') else {
            continue;
        };
        if fields.insert(name.to_owned(), value.to_owned()).is_some() {
            return Err(format!("comparison record repeats field {name}"));
        }
    }
    Ok(fields)
}

fn require_token(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(char::is_whitespace) || value.contains('=') {
        Err(format!("{name} must be one nonempty key-value-safe token"))
    } else {
        Ok(())
    }
}

fn protect_owner_directory(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("could not protect {}: {error}", path.display()))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("could not create {}: {error}", path.display()))
}

fn elapsed_micros(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn sleep_until(deadline: Instant) {
    if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        thread::sleep(remaining);
    }
}

mod tests;

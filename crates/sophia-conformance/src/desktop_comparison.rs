//! Typed orchestration and evidence reduction for the diagnostic desktop matrix.

mod capture;
mod capture_owner;
mod host;

pub use capture::{CaptureReplay, replay_attempt};
pub use capture_owner::{
    attest_session, attest_session_auto, capture_next, finalize_next, install_reference, preflight,
    qualify, run_stream,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const XLIBRE_COMMIT: &str = "56be9f4320ef121dc5d4bc40a6365d995512d3bc";
pub const NIRI_VERSION: &str = "26.04";
pub const KITTY_VERSION: &str = "0.48.2";
pub const FIREFOX_VERSION: &str = "155";
pub const TOPOLOGY: &str = "DP-1:2560x1440@60000+DP-2:1920x1080@60000";
pub const XMONAD_VERSION: &str = "0.18.1";
pub const XMONAD_CONTRIB_VERSION: &str = "0.18.2";

const OWNER_DIRECTORY_MODE: u32 = 0o700;
const OWNER_FILE_MODE: u32 = 0o600;

pub(crate) const STACKS: [&str; 3] = ["sophia", "xlibre-xmonad", "niri"];
pub(crate) const SHORT_WORKLOADS: [&str; 4] =
    ["kitty-60s", "firefox-local", "resize", "kitty-burst-16"];
pub(crate) const CONFIGS: [&str; 15] = [
    "validation/desktop-comparison/config/sophia.kdl",
    "validation/desktop-comparison/config/xlibre-xmonad.kdl",
    "validation/desktop-comparison/config/niri.kdl",
    "validation/desktop-comparison/firefox/index.html",
    "validation/desktop-comparison/firefox/user.js",
    "tools/desktop_comparison_tracefs.sh",
    "tools/desktop_comparison_tty3.sh",
    "tools/start_sophia_tty3.sh",
    "tools/run_sophia_session.sh",
    "tools/sophia_tty_mode.py",
    "tools/lib/session_terminal.sh",
    "validation/desktop-comparison/profiles/core.kdl",
    "validation/desktop-comparison/profiles/hagia.kdl",
    "validation/desktop-comparison/profiles/niri.kdl",
    "validation/desktop-comparison/profiles/xmonad.hs",
];

/// Materializes the canonical Engine cursor as a standard Xcursor theme for
/// comparison stacks that consume the freedesktop cursor interface.
pub fn write_x11_core_cursor_theme(root: &Path) -> Result<Vec<String>, String> {
    let asset = sophia_engine::x11_core_left_ptr_cursor(1);
    let theme_root = root.join("sophia-x11-core");
    let theme = theme_root.join("cursors");
    for directory in [root, theme_root.as_path(), theme.as_path()] {
        fs::DirBuilder::new()
            .recursive(false)
            .mode(OWNER_DIRECTORY_MODE)
            .create(directory)
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(|error| format!("could not create comparison cursor theme: {error}"))?;
        let metadata = fs::symlink_metadata(directory)
            .map_err(|error| format!("could not inspect comparison cursor theme: {error}"))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(format!(
                "comparison cursor theme path is not a real directory: {}",
                directory.display()
            ));
        }
        fs::set_permissions(directory, fs::Permissions::from_mode(OWNER_DIRECTORY_MODE))
            .map_err(|error| format!("could not protect comparison cursor theme: {error}"))?;
    }

    let mut bytes = Vec::with_capacity(28 + 36 + asset.pixels().len());
    bytes.extend_from_slice(b"Xcur");
    push_xcursor_u32(&mut bytes, 16);
    push_xcursor_u32(&mut bytes, 0x0001_0000);
    push_xcursor_u32(&mut bytes, 1);
    push_xcursor_u32(&mut bytes, 0xfffd_0002);
    push_xcursor_u32(&mut bytes, 16);
    push_xcursor_u32(&mut bytes, 28);
    push_xcursor_u32(&mut bytes, 36);
    push_xcursor_u32(&mut bytes, 0xfffd_0002);
    push_xcursor_u32(&mut bytes, 16);
    push_xcursor_u32(&mut bytes, 1);
    push_xcursor_u32(&mut bytes, asset.width());
    push_xcursor_u32(&mut bytes, asset.height());
    push_xcursor_u32(&mut bytes, asset.hotspot().0);
    push_xcursor_u32(&mut bytes, asset.hotspot().1);
    push_xcursor_u32(&mut bytes, 0);
    bytes.extend_from_slice(asset.pixels());

    for name in ["left_ptr", "default", "arrow"] {
        let path = theme.join(name);
        let created = OpenOptions::new()
            .create(true)
            .create_new(true)
            .write(true)
            .mode(OWNER_FILE_MODE)
            .open(&path);
        match created {
            Ok(mut file) => {
                file.write_all(&bytes)
                    .and_then(|()| file.sync_all())
                    .map_err(|error| format!("could not seal {}: {error}", path.display()))?;
                fs::set_permissions(&path, fs::Permissions::from_mode(OWNER_FILE_MODE))
                    .map_err(|error| format!("could not protect {}: {error}", path.display()))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&path)
                    .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.permissions().mode() & 0o777 != OWNER_FILE_MODE
                    || fs::read(&path)
                        .map_err(|error| format!("could not verify {}: {error}", path.display()))?
                        != bytes
                {
                    return Err(format!(
                        "existing comparison cursor asset is not the prepared asset: {}",
                        path.display()
                    ));
                }
            }
            Err(error) => {
                return Err(format!("could not create {}: {error}", path.display()));
            }
        }
    }
    Ok(vec![format!(
        "desktop_comparison_cursor schema=1 status=materialized theme=sophia-x11-core size=16 shape=left_ptr digest={}",
        asset.digest(),
    )])
}

fn push_xcursor_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ScheduledSample {
    pub order: usize,
    pub stack: String,
    pub workload: String,
    pub repetition: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComparisonLane {
    Interactive,
    OptionalSoak,
}

impl ComparisonLane {
    const fn token(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::OptionalSoak => "optional-soak",
        }
    }
}

#[derive(Clone, Debug)]
struct Sample {
    scheduled: ScheduledSample,
    duration_msec: u64,
    processes: u64,
    pss_peak_kib: u64,
    rss_peak_kib: u64,
    anonymous_peak_kib: u64,
    private_dirty_peak_kib: u64,
    cpu_msec: u64,
    minor_faults: u64,
    major_faults: u64,
    threads_peak: u64,
    fds_peak: u64,
    stack_processes: u64,
    stack_pss_peak_kib: u64,
    stack_rss_peak_kib: u64,
    stack_cpu_msec: u64,
    stack_threads_peak: u64,
    stack_fds_peak: u64,
    workload_processes: u64,
    workload_pss_peak_kib: u64,
    workload_rss_peak_kib: u64,
    workload_cpu_msec: u64,
    workload_threads_peak: u64,
    workload_fds_peak: u64,
    launch_msec: u64,
    settle_msec: u64,
    resize_msec: u64,
    frame_mean_usec: u64,
    frame_p50_usec: u64,
    frame_p95_usec: u64,
    frame_p99_usec: u64,
    frame_max_usec: u64,
    frame_deliveries: u64,
    frame_duplicates: u64,
}

fn comparison_binaries(repo: &Path) -> Result<[(&'static str, PathBuf); 6], String> {
    let configured =
        |name: &str, fallback: PathBuf| std::env::var_os(name).map_or(fallback, PathBuf::from);
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is unset; reference binary defaults cannot be resolved")?;
    let xlibre_prefix = configured(
        "SOPHIA_DESKTOP_COMPARISON_XLIBRE_PREFIX",
        home.join(".local/opt/xlibre-56be9f4320ef"),
    );
    let binaries = [
        (
            "sophia_sha256",
            configured(
                "SOPHIA_DESKTOP_COMPARISON_SOPHIA_BIN",
                repo.join("target/release/sophia"),
            ),
        ),
        (
            "hagia_sha256",
            configured(
                "SOPHIA_DESKTOP_COMPARISON_HAGIA_BIN",
                repo.join("../hagia/hagia"),
            ),
        ),
        (
            "narthex_sha256",
            configured(
                "SOPHIA_DESKTOP_COMPARISON_NARTHEX_BIN",
                repo.join("../narthex/narthex"),
            ),
        ),
        ("xlibre_sha256", xlibre_prefix.join("bin/Xorg")),
        ("xmonad_sha256", xlibre_prefix.join("bin/xmonad")),
        (
            "niri_sha256",
            configured(
                "SOPHIA_DESKTOP_COMPARISON_NIRI_BIN",
                PathBuf::from("/usr/bin/niri"),
            ),
        ),
    ];
    for (name, path) in &binaries {
        let metadata = fs::metadata(path).map_err(|error| {
            format!(
                "comparison binary {name} is unavailable at {}: {error}",
                path.display()
            )
        })?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!(
                "comparison binary {name} is not executable: {}",
                path.display()
            ));
        }
    }
    Ok(binaries)
}

pub fn prepare(repo: &Path, run: &Path) -> Result<Vec<String>, String> {
    verify_host_tool_versions()?;
    let identity = host::detect()?;
    prepare_with_identity(
        repo,
        run,
        &identity.kernel,
        &identity.mesa,
        &identity.gpu,
        ComparisonLane::Interactive,
    )
}

pub fn prepare_optional_soak(repo: &Path, run: &Path) -> Result<Vec<String>, String> {
    verify_host_tool_versions()?;
    let identity = host::detect()?;
    prepare_with_identity(
        repo,
        run,
        &identity.kernel,
        &identity.mesa,
        &identity.gpu,
        ComparisonLane::OptionalSoak,
    )
}

/// Verifies every mutable host executable used by a prepared comparison run.
///
/// This belongs before preparation and before graphical takeover. Capture also
/// repeats it so a package update cannot silently mix versions within a run.
pub fn verify_host_tool_versions() -> Result<(), String> {
    require_tool_version("kitty", &["--version"], KITTY_VERSION)?;
    require_tool_version("firefox", &["--version"], FIREFOX_VERSION)?;
    require_tool_version("niri", &["--version"], NIRI_VERSION)
}

fn require_tool_version(program: &str, arguments: &[&str], expected: &str) -> Result<(), String> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run {program} version preflight: {error}"))?;
    let observed = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() || !version_output_matches(&observed, expected) {
        return Err(format!(
            "{program} version mismatch: expected token {expected:?}, observed {:?}",
            observed.trim()
        ));
    }
    Ok(())
}

fn version_output_matches(observed: &str, expected: &str) -> bool {
    observed.split_ascii_whitespace().any(|token| {
        token == expected
            || token
                .strip_prefix(expected)
                .is_some_and(|suffix| suffix.starts_with('.'))
    })
}

fn prepare_with_identity(
    repo: &Path,
    run: &Path,
    kernel: &str,
    mesa: &str,
    gpu: &str,
    lane: ComparisonLane,
) -> Result<Vec<String>, String> {
    if run.exists() {
        return Err(format!("comparison run already exists: {}", run.display()));
    }
    for (name, value) in [("kernel", kernel), ("mesa", mesa), ("gpu", gpu)] {
        require_token(name, value)?;
    }
    require_clean_worktree(&git_output(repo, &["status", "--porcelain"])?)?;
    let source_commit = git_output(repo, &["rev-parse", "HEAD"])?;
    let signature = Command::new("git")
        .args([
            "-C",
            repo.to_string_lossy().as_ref(),
            "verify-commit",
            "HEAD",
        ])
        .output()
        .map_err(|error| format!("could not verify candidate signature: {error}"))?;
    if !signature.status.success() {
        return Err("desktop comparison requires a signed Sophia candidate".to_owned());
    }

    create_private_run_storage(run)?;
    let mut binary_fields = String::new();
    for (name, path) in comparison_binaries(repo)? {
        binary_fields.push_str(&format!(" {name}={}", digest_file(&path)?));
    }
    let cursor_digest = sophia_engine::x11_core_left_ptr_cursor(1).digest();
    let mut manifest = format!(
        "desktop_comparison_manifest schema=4 status=prepared diagnostic_only=true raw_capture_required=true acquisition=terminal_free_visible lane={} optional_soak=separate source_commit={source_commit} candidate_signature=verified kernel={kernel} mesa={mesa} gpu={gpu} topology={TOPOLOGY} kitty={KITTY_VERSION} firefox={FIREFOX_VERSION} cursor_theme=sophia-x11-core cursor_size=16 cursor_shape=left_ptr cursor_sha256={cursor_digest}{binary_fields}\n",
        lane.token(),
    );
    manifest.push_str(&format!(
        "desktop_comparison_stack schema=2 id=sophia version={source_commit} backend=native\n"
    ));
    manifest.push_str(&format!(
        "desktop_comparison_stack schema=2 id=xlibre-xmonad version={XLIBRE_COMMIT} xmonad={XMONAD_VERSION} xmonad_contrib={XMONAD_CONTRIB_VERSION} backend=native\n"
    ));
    manifest.push_str(&format!(
        "desktop_comparison_stack schema=2 id=niri version={NIRI_VERSION} path=/usr/bin/niri backend=native\n"
    ));
    for config in CONFIGS {
        let path = repo.join(config);
        manifest.push_str(&format!(
            "desktop_comparison_input schema=2 path={config} sha256={}\n",
            digest_file(&path)?
        ));
    }
    write_new(&run.join("manifest.kdl"), manifest.as_bytes())?;

    let schedule = schedule_for_lane(lane);
    let mut encoded = String::new();
    for item in &schedule {
        encoded.push_str(&format!(
            "desktop_comparison_schedule schema=2 order={} stack={} workload={} repetition={} backend=native\n",
            item.order, item.stack, item.workload, item.repetition
        ));
    }
    write_new(&run.join("schedule.kdl"), encoded.as_bytes())?;
    rewrite_checksums(run, &[])?;
    Ok(vec![format!(
        "desktop_comparison_prepare schema=4 status=complete run={} source_commit={} lane={} samples={}",
        run.display(),
        source_commit,
        lane.token(),
        schedule.len()
    )])
}
pub fn require_candidate_checkout(repo: &Path, run: &Path) -> Result<(), String> {
    let expected = source_commit(run)?;
    let observed = git_output(repo, &["rev-parse", "HEAD"])?;
    if observed != expected {
        return Err(format!(
            "comparison candidate checkout mismatch: expected {expected}, observed {observed}"
        ));
    }
    require_clean_worktree(&git_output(repo, &["status", "--porcelain"])?)
}

pub fn verify_prepared_binaries(repo: &Path, run: &Path) -> Result<(), String> {
    let manifest = fs::read_to_string(run.join("manifest.kdl"))
        .map_err(|error| format!("comparison manifest is missing: {error}"))?;
    let first = manifest
        .lines()
        .next()
        .ok_or("comparison manifest is empty")?;
    let identities = fields(first)?;
    for (name, path) in comparison_binaries(repo)? {
        let expected = identities
            .get(name)
            .ok_or_else(|| format!("comparison manifest lacks {name}"))?;
        if digest_file(&path)? != *expected {
            return Err(format!(
                "prepared comparison binary changed: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

pub fn status(repo: &Path, run: &Path) -> Result<Vec<String>, String> {
    verify_prepared_inputs(repo, run)?;
    verify_checksums(run)?;
    verify_no_pending_capture(run)?;
    let observed = observed_samples(run)?;
    verify_raw_attempts(run, &observed)?;
    let expected = schedule_for_run(run)?;
    match next_scheduled(run)? {
        Some(item) => Ok(vec![format!(
            "desktop_comparison_status schema=1 status=pending completed={} total={} next_order={} next_stack={} next_workload={} next_repetition={}",
            item.order.saturating_sub(1),
            expected.len(),
            item.order,
            item.stack,
            item.workload,
            item.repetition,
        )]),
        None => Ok(vec![format!(
            "desktop_comparison_status schema=1 status=complete completed={} total={}",
            expected.len(),
            expected.len(),
        )]),
    }
}

fn verify_no_pending_capture(run: &Path) -> Result<(), String> {
    let incoming = run.join("incoming");
    if incoming.exists()
        && fs::read_dir(&incoming)
            .map_err(|error| format!("could not inspect pending captures: {error}"))?
            .next()
            .is_some()
    {
        return Err(format!(
            "partial comparison capture requires diagnosis: {}",
            incoming.display()
        ));
    }
    Ok(())
}

fn observed_samples(run: &Path) -> Result<BTreeSet<ScheduledSample>, String> {
    let candidate = source_commit(run)?;
    let mut observed = BTreeSet::new();
    for entry in sample_paths(run)? {
        let source = fs::read_to_string(&entry)
            .map_err(|error| format!("could not read {}: {error}", entry.display()))?;
        let sample = parse_sample(&source, &candidate)?;
        if !observed.insert(sample.scheduled.clone()) {
            return Err("comparison contains a duplicate scheduled sample".to_owned());
        }
    }
    Ok(observed)
}

pub fn next_scheduled(run: &Path) -> Result<Option<ScheduledSample>, String> {
    let expected = schedule_for_run(run)?;
    let observed = observed_samples(run)?;
    for (index, item) in expected.iter().enumerate() {
        if !observed.contains(item) {
            if expected[index.saturating_add(1)..]
                .iter()
                .any(|later| observed.contains(later))
            {
                return Err("comparison samples are not a contiguous schedule prefix".to_owned());
            }
            return Ok(Some(item.clone()));
        }
    }
    if observed.len() != expected.len() {
        return Err("comparison contains an unexpected scheduled sample".to_owned());
    }
    Ok(None)
}

/// Ingest one native-stack adapter log and bind it to the prepared schedule.
fn run_sample(repo: &Path, run: &Path, raw_log: &Path) -> Result<Vec<String>, String> {
    verify_prepared_inputs(repo, run)?;
    verify_checksums(run)?;
    let source = fs::read_to_string(raw_log)
        .map_err(|error| format!("could not read sample log {}: {error}", raw_log.display()))?;
    let sample = parse_sample(&source, &source_commit(run)?)?;
    let scheduled = schedule_for_run(run)?;
    if !scheduled.iter().any(|item| item == &sample.scheduled) {
        return Err(format!(
            "sample is not in the prepared schedule: {}/{}/{} order={}",
            sample.scheduled.stack,
            sample.scheduled.workload,
            sample.scheduled.repetition,
            sample.scheduled.order
        ));
    }
    let relative = PathBuf::from("samples")
        .join(&sample.scheduled.stack)
        .join(format!(
            "{}-{}.log",
            sample.scheduled.workload, sample.scheduled.repetition
        ));
    let destination = run.join(&relative);
    if destination.exists() {
        return Err(format!(
            "comparison sample already exists: {}",
            destination.display()
        ));
    }
    fs::create_dir_all(destination.parent().expect("sample has a parent"))
        .map_err(|error| format!("could not create sample directory: {error}"))?;
    fs::copy(raw_log, &destination)
        .map_err(|error| format!("could not bind sample log: {error}"))?;
    append_checksum(run, &relative)?;
    Ok(vec![format!(
        "desktop_comparison_run schema=1 status=recorded order={} stack={} workload={} repetition={} sha256={}",
        sample.scheduled.order,
        sample.scheduled.stack,
        sample.scheduled.workload,
        sample.scheduled.repetition,
        digest_file(&destination)?
    )])
}

pub fn bind_attempt(repo: &Path, run: &Path, raw_attempt: &Path) -> Result<Vec<String>, String> {
    verify_prepared_inputs(repo, run)?;
    verify_checksums(run)?;
    let observed = observed_samples(run)?;
    verify_raw_attempts(run, &observed)?;
    let next = next_scheduled(run)?
        .ok_or_else(|| "desktop comparison matrix is already complete".to_owned())?;
    let preview = replay_attempt(run, raw_attempt)?;
    if preview.order != next.order
        || preview.stack != next.stack
        || preview.workload != next.workload
        || preview.repetition != next.repetition
    {
        return Err(format!(
            "capture does not match next schedule row: expected order={} stack={} workload={} repetition={}",
            next.order, next.stack, next.workload, next.repetition
        ));
    }
    let attempt_root = run.join("attempts");
    fs::create_dir_all(&attempt_root)
        .map_err(|error| format!("could not create attempt root: {error}"))?;
    let destination = attempt_root.join(format!(
        "{:02}-{}-{}-{}",
        next.order, next.stack, next.workload, next.repetition
    ));
    let archived = capture::archive_attempt(run, raw_attempt, &destination)?;
    if archived != preview {
        return Err("archived comparison attempt differs from its source replay".to_owned());
    }
    let mut lines = run_sample(repo, run, &destination.join("result.kdl"))?;
    lines.insert(
        0,
        format!(
            "desktop_comparison_bind schema=2 status=complete order={} stack={} workload={} repetition={} attempt={}",
            next.order,
            next.stack,
            next.workload,
            next.repetition,
            destination.display(),
        ),
    );
    Ok(lines)
}
fn raw_capture_required(run: &Path) -> Result<bool, String> {
    let manifest = fs::read_to_string(run.join("manifest.kdl"))
        .map_err(|error| format!("comparison manifest is missing: {error}"))?;
    Ok(manifest.lines().next().is_some_and(|line| {
        line.split_ascii_whitespace()
            .any(|token| token == "raw_capture_required=true")
    }))
}

fn verify_raw_attempts(
    run: &Path,
    observed_samples: &BTreeSet<ScheduledSample>,
) -> Result<(), String> {
    if !raw_capture_required(run)? {
        return Ok(());
    }
    let root = run.join("attempts");
    let entries = fs::read_dir(&root)
        .map_err(|error| format!("comparison attempt root is missing: {error}"))?;
    let mut observed_attempts = BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read comparison attempt: {error}"))?;
        if !entry.path().is_dir()
            || entry
                .file_name()
                .to_str()
                .is_none_or(|name| name.ends_with(".partial"))
        {
            return Err("comparison attempt root contains an unsealed entry".to_owned());
        }
        let replay = capture::verify_archived_attempt(run, &entry.path())?;
        let scheduled = ScheduledSample {
            order: replay.order,
            stack: replay.stack.clone(),
            workload: replay.workload.clone(),
            repetition: replay.repetition,
        };
        if !observed_attempts.insert(scheduled.clone()) {
            return Err("comparison contains duplicate raw attempt evidence".to_owned());
        }
        let sample_path = run.join("samples").join(&scheduled.stack).join(format!(
            "{}-{}.log",
            scheduled.workload, scheduled.repetition
        ));
        let bound = fs::read_to_string(&sample_path)
            .map_err(|error| format!("bound sample is missing for raw attempt: {error}"))?;
        if bound != format!("{}\n", replay.sample_record) {
            return Err("bound sample does not match its raw attempt replay".to_owned());
        }
    }
    if observed_attempts != *observed_samples {
        return Err("raw attempt set does not cover the bound sample matrix".to_owned());
    }
    Ok(())
}

pub fn verify(repo: &Path, run: &Path) -> Result<Vec<String>, String> {
    verify_prepared_inputs(repo, run)?;
    verify_checksums(run)?;
    verify_no_pending_capture(run)?;
    let expected = schedule_for_run(run)?;
    let expected_set = expected.iter().cloned().collect::<BTreeSet<_>>();
    let observed = observed_samples(run)?;
    if observed != expected_set {
        let missing = expected_set.difference(&observed).count();
        let unexpected = observed.difference(&expected_set).count();
        return Err(format!(
            "comparison matrix is incomplete: missing={missing} unexpected={unexpected}"
        ));
    }
    verify_raw_attempts(run, &observed)?;
    Ok(vec![format!(
        "desktop_comparison_verify schema=2 status=complete diagnostic_only=true samples={} relative_performance_gate=false",
        observed.len()
    )])
}

pub fn report(repo: &Path, run: &Path) -> Result<Vec<String>, String> {
    let mut lines = verify(repo, run)?;
    let candidate = source_commit(run)?;
    let mut groups = BTreeMap::<(String, String), Vec<Sample>>::new();
    for entry in sample_paths(run)? {
        let source = fs::read_to_string(&entry)
            .map_err(|error| format!("could not read {}: {error}", entry.display()))?;
        let sample = parse_sample(&source, &candidate)?;
        groups
            .entry((
                sample.scheduled.stack.clone(),
                sample.scheduled.workload.clone(),
            ))
            .or_default()
            .push(sample);
    }
    for ((stack, workload), samples) in groups {
        let count = u64::try_from(samples.len()).unwrap_or(u64::MAX);
        let sum = |field: fn(&Sample) -> u64| {
            samples.iter().map(field).fold(0u64, u64::saturating_add) / count
        };
        lines.push(format!(
            "desktop_comparison_report schema=3 status=diagnostic stack={stack} workload={workload} samples={count} duration_mean_msec={} processes_peak_mean={} pss_peak_mean_kib={} rss_peak_mean_kib={} anonymous_peak_mean_kib={} private_dirty_peak_mean_kib={} cpu_mean_msec={} minor_faults_mean={} major_faults_mean={} threads_peak_mean={} fds_peak_mean={} stack_processes_peak_mean={} stack_pss_peak_mean_kib={} stack_rss_peak_mean_kib={} stack_cpu_mean_msec={} stack_threads_peak_mean={} stack_fds_peak_mean={} workload_processes_peak_mean={} workload_pss_peak_mean_kib={} workload_rss_peak_mean_kib={} workload_cpu_mean_msec={} workload_threads_peak_mean={} workload_fds_peak_mean={} launch_mean_msec={} settle_mean_msec={} resize_p95_mean_msec={} frame_deliveries_mean={} frame_duplicates_mean={} frame_mean_usec={} frame_p50_mean_usec={} frame_p95_mean_usec={} frame_p99_mean_usec={} frame_max_mean_usec={} crashes=0 sample_loss=0 verdict=none",
            sum(|sample| sample.duration_msec),
            sum(|sample| sample.processes),
            sum(|sample| sample.pss_peak_kib),
            sum(|sample| sample.rss_peak_kib),
            sum(|sample| sample.anonymous_peak_kib),
            sum(|sample| sample.private_dirty_peak_kib),
            sum(|sample| sample.cpu_msec),
            sum(|sample| sample.minor_faults),
            sum(|sample| sample.major_faults),
            sum(|sample| sample.threads_peak),
            sum(|sample| sample.fds_peak),
            sum(|sample| sample.stack_processes),
            sum(|sample| sample.stack_pss_peak_kib),
            sum(|sample| sample.stack_rss_peak_kib),
            sum(|sample| sample.stack_cpu_msec),
            sum(|sample| sample.stack_threads_peak),
            sum(|sample| sample.stack_fds_peak),
            sum(|sample| sample.workload_processes),
            sum(|sample| sample.workload_pss_peak_kib),
            sum(|sample| sample.workload_rss_peak_kib),
            sum(|sample| sample.workload_cpu_msec),
            sum(|sample| sample.workload_threads_peak),
            sum(|sample| sample.workload_fds_peak),
            sum(|sample| sample.launch_msec),
            sum(|sample| sample.settle_msec),
            sum(|sample| sample.resize_msec),
            sum(|sample| sample.frame_deliveries),
            sum(|sample| sample.frame_duplicates),
            sum(|sample| sample.frame_mean_usec),
            sum(|sample| sample.frame_p50_usec),
            sum(|sample| sample.frame_p95_usec),
            sum(|sample| sample.frame_p99_usec),
            sum(|sample| sample.frame_max_usec),
        ));
    }
    Ok(lines)
}

pub fn schedule() -> Vec<ScheduledSample> {
    let mut result = Vec::new();
    let mut order = 1usize;
    for workload in SHORT_WORKLOADS {
        for repetition in 1..=3u8 {
            let rotation = usize::from(repetition - 1);
            for offset in 0..STACKS.len() {
                result.push(ScheduledSample {
                    order,
                    stack: STACKS[(rotation + offset) % STACKS.len()].to_owned(),
                    workload: workload.to_owned(),
                    repetition,
                });
                order += 1;
            }
        }
    }
    result
}

pub fn optional_soak_schedule() -> Vec<ScheduledSample> {
    vec![ScheduledSample {
        order: 1,
        stack: "sophia".to_owned(),
        workload: "soak-2h".to_owned(),
        repetition: 1,
    }]
}

fn schedule_for_lane(lane: ComparisonLane) -> Vec<ScheduledSample> {
    match lane {
        ComparisonLane::Interactive => schedule(),
        ComparisonLane::OptionalSoak => optional_soak_schedule(),
    }
}

fn schedule_for_run(run: &Path) -> Result<Vec<ScheduledSample>, String> {
    Ok(schedule_for_lane(prepared_lane(run)?))
}

fn prepared_lane(run: &Path) -> Result<ComparisonLane, String> {
    let manifest = fs::read_to_string(run.join("manifest.kdl"))
        .map_err(|error| format!("comparison manifest is missing: {error}"))?;
    match fields(manifest.lines().next().unwrap_or_default())?
        .get("lane")
        .copied()
    {
        Some("interactive") => Ok(ComparisonLane::Interactive),
        Some("optional-soak") => Ok(ComparisonLane::OptionalSoak),
        Some(lane) => Err(format!("comparison manifest names unknown lane {lane:?}")),
        None => Err("comparison manifest lacks lane".to_owned()),
    }
}

include!("desktop_comparison/sample_record.rs");

include!("desktop_comparison/storage.rs");

fn git_output(repo: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        return Err(format!("git {} failed", arguments.join(" ")));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|_| "git output was not UTF-8".to_owned())
}

#[path = "../tests/support/desktop_comparison.rs"]
mod tests;

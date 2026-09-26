//! Canonical deterministic repository checks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Runs the offline gate and returns what it has to say.
///
/// Returning the summary rather than printing it keeps the printing in the
/// binary, where the layout rule puts it: a library that prints has decided
/// for every caller how its result is presented.
pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    match arguments {
        [] => all(repo),
        [subject, rest @ ..] if subject == "native-protocol-family" => {
            crate::native_protocol_family::run(repo, rest)
        }
        [subject, rest @ ..] if subject == "m3-acceptance" => crate::m3_acceptance::run(repo, rest),
        [subject, rest @ ..] if subject == "m4-acceptance" => {
            crate::m3_acceptance::run_m4(repo, rest)
        }
        [subject, rest @ ..] if subject == "m5-acceptance" => {
            crate::m3_acceptance::run_m5(repo, rest)
        }
        [subject, rest @ ..] if subject == "m6-evidence" => {
            crate::m3_acceptance::run_m6(repo, rest)
        }
        [subject, rest @ ..] if subject == "x11-profile" => {
            crate::m3_acceptance::run_profiles(repo, rest)
        }
        [subject, rest @ ..] if subject == "m3-components" => {
            crate::m3_acceptance::run_components(repo, rest)
        }
        [subject, rest @ ..] if subject == "xtest-selection" => {
            crate::xtest_selection::run(repo, rest)
        }
        [subject, rest @ ..] if subject == "xterm-pointer-oracle" => {
            crate::xterm_pointer_oracle::run(repo, rest)
        }
        [subject, rest @ ..] if subject == "9p-conformance" => {
            crate::nine_p_conformance::run(repo, rest)
        }
        [subject] if subject == "layout" => layout(repo).map(|()| Vec::new()),
        [subject] => Err(format!("unknown check subject {subject:?}")),
        _ => Err("check accepts at most one subject".to_owned()),
    }
}

fn all(repo: &Path) -> Result<Vec<String>, String> {
    command(repo, "cargo", &["fmt", "--all", "--check"])?;
    command(repo, "git", &["diff", "--check"])?;
    command_quiet(
        repo,
        "cargo",
        &[
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
        ],
    )?;
    workspace_tests(repo)?;
    // `clippy.toml` sits at the workspace root, and clippy resolves it from
    // the crate being linted rather than walking up to find it, so a
    // workspace run would silently use the defaults instead. Pointing it here
    // is what makes the two recorded thresholds apply.
    clippy(repo)?;
    sophia_conformance::profile::check_every_profile(&[])?;
    layout(repo)?;
    command(repo, "sh", &["tools/check_shell_c_wire.sh"])?;
    anchored_readers(repo)?;
    for pattern in [
        "layout_comparison_test.py",
        "dri3_layout_probe_test.py",
        "physical_gate_identity_test.py",
    ] {
        command(
            repo,
            "python3",
            &[
                "-B",
                "-m",
                "unittest",
                "discover",
                "-s",
                "tools/tests",
                "-p",
                pattern,
            ],
        )?;
    }
    let mut report = vec![archives(repo)?];
    report.push(hardware_proof(
        repo,
        "tools/check_buffer_age_equivalence.sh",
        "buffer-age pixel equivalence",
    )?);
    report.push(hardware_proof(
        repo,
        "tools/check_client_first_frame.sh",
        "GLX/EGL first-frame and pixmap-export pixels",
    )?);
    command(
        repo,
        "tools/run_sophia_terminal_gate_tty3.sh",
        &["--self-test"],
    )?;
    command(
        repo,
        "tools/check_live_record_schema_readers.sh",
        &["--self-test"],
    )?;
    for tool in [
        "tools/check_hagia_profile_preflight.sh",
        "tools/check_installed_session_type.sh",
        "tools/check_bounded_xterm_geometry.sh",
        "tools/check_live_record_schema_readers.sh",
        "tools/check_retired_milestone_launchers.sh",
        "tools/check_live_session_milestone4_verifier.sh",
        "tools/check_sophia_firefox_physical_verifier.sh",
        "tools/check_direct_scanout_verifier.sh",
        "tools/check_direct_scanout_archive_verifier.sh",
        "tools/check_sophia_standalone_vkcube_verifier.sh",
        "tools/check_hagia_native_matchers.sh",
        "tools/check_firefox_m10_rendering_page.sh",
        "tools/check_sophia_firefox_rendering_verifier.sh",
        "tools/check_mirror_group_physical_verifier.sh",
        "tools/check_keyboard_independence_verifier.sh",
        "tools/check_keyboard_independence_session_verifier.sh",
        "tools/check_sophia_terminal_performance_reporter.sh",
        "tools/check_installed_native_verifiers.sh",
        "tools/check_lom_gpu_content_proof_verifiers.sh",
    ] {
        command(repo, tool, &[])?;
    }
    Ok(report)
}

/// Promoted archives, re-verified as a regression corpus.
///
/// These are the only decorated, real-hardware evidence the repo owns, and
/// the verifiers that read them are the code most likely to rot silently: a
/// reader that stops matching still returns Ok on a synthetic fixture built
/// from the same assumption it just broke. Re-verifying the archives is how a
/// broken reader is caught by a machine rather than by a burned TTY.
///
/// Absent families are reported and never fail. This runs on machines that
/// have never promoted anything, and a missing corpus is not a defect.
fn archives(repo: &Path) -> Result<String, String> {
    let Some(root) = promotion_root() else {
        return Ok("archives: no state home, corpus skipped".to_owned());
    };
    let families: [(&str, ArchiveVerifier); 3] = [
        (
            "hagia-native-runs",
            ArchiveVerifier::Tool("tools/verify_hagia_native_session_archive.sh"),
        ),
        (
            "mirror-group-runs",
            ArchiveVerifier::Tool("tools/verify_mirror_group_physical_archive.sh"),
        ),
        ("direct-scanout-runs", ArchiveVerifier::DirectScanout),
    ];
    let mut summary = Vec::new();
    let mut absent = Vec::new();
    for (family, verifier) in families {
        let directory = root.join(family);
        let Ok(entries) = std::fs::read_dir(&directory) else {
            absent.push(family);
            continue;
        };
        let mut runs = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        runs.sort();
        if runs.is_empty() {
            absent.push(family);
            continue;
        }
        let total = runs.len();
        for run in runs {
            let outcome = match verifier {
                ArchiveVerifier::Tool(tool) => command_quiet(
                    repo,
                    &repo.join(tool).display().to_string(),
                    &[&run.display().to_string()],
                ),
                ArchiveVerifier::DirectScanout => {
                    sophia_conformance::direct_scanout_archive::verify_archive(repo, &run)
                }
            };
            outcome.map_err(|error| {
                format!(
                    "promoted archive {} no longer verifies: {error}\nEither this change broke a verifier, or the archive was altered. Both are worth stopping for.",
                    run.display()
                )
            })?;
        }
        summary.push(format!("{family} {total}/{total}"));
    }
    if !absent.is_empty() {
        summary.push(format!("(absent: {})", absent.join(" ")));
    }
    Ok(format!("archives: {}", summary.join("  ")))
}

enum ArchiveVerifier {
    Tool(&'static str),
    DirectScanout,
}

fn promotion_root() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/state"))
        })?;
    Some(state.join("sophia/promotion"))
}

/// Session-log readers must find their records by marker, not by line start.
///
/// Records reach a session log two ways: printed by the session itself, bare;
/// or emitted through `tracing`, decorated with a timestamp, level, module
/// path, and ANSI colour. A reader anchored to the line start sees only the
/// first kind -- and which kind carries a given record is a fact about
/// plumbing, not about evidence, so it changes without anyone deciding to
/// change it.
///
/// That is not hypothetical. The episode-order rules were anchored, saw none
/// of their records, and reported `episode_sessions=0` in every gate summary
/// from archive 0001 onward while never once running. A passing physical run
/// was then refused by a rule that had never worked.
///
/// One reader stays anchored on purpose, and this names it rather than
/// letting an allowlist grow silently.
fn anchored_readers(repo: &Path) -> Result<(), String> {
    let source = repo.join("crates/sophia-conformance/src");
    let mut offenders = BTreeSet::new();
    for entry in std::fs::read_dir(&source)
        .map_err(|error| format!("could not read {}: {error}", source.display()))?
    {
        let path = entry
            .map_err(|error| format!("could not read a conformance source entry: {error}"))?
            .path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        if sophia_conformance::direct_scanout::ANCHORED_READER_ALLOWLIST.contains(&name.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        for (index, line) in text.lines().enumerate() {
            if line.contains("starts_with(\"sophia_") || line.contains("strip_prefix(\"sophia_") {
                offenders.insert(format!("{name}:{}", index + 1));
            }
        }
    }
    if offenders.is_empty() {
        return Ok(());
    }
    Err(format!(
        "these conformance readers anchor a session record to the line start, so a \n`tracing`-decorated record is invisible to them:\n  {}\nUse `record_after_marker`. If a reader genuinely parses bare stdout rather \nthan a session log, add its file to ANCHORED_READER_ALLOWLIST with the reason.",
        offenders.into_iter().collect::<Vec<_>>().join("\n  ")
    ))
}

fn layout(repo: &Path) -> Result<(), String> {
    let output = Command::new(repo.join("tools/audit_source_layout.sh"))
        .current_dir(repo)
        .output()
        .map_err(|error| format!("could not run source-layout audit: {error}"))?;
    let mut text = String::from_utf8(output.stdout)
        .map_err(|error| format!("source-layout audit emitted non-UTF-8: {error}"))?;
    text.push_str(
        &String::from_utf8(output.stderr)
            .map_err(|error| format!("source-layout audit emitted non-UTF-8: {error}"))?,
    );
    // AN ERROR THIS CANNOT READ IS NOT AN ERROR THIS MAY IGNORE. Unrecognised
    // failures used to be dropped by the filter below and the audit's own exit
    // status is never decisive here -- it is non-zero whenever any debt stands,
    // which is always -- so a check added with a message of its own would have
    // been invisible to this gate rather than enforced by it.
    let unreadable = text
        .lines()
        .filter(|line| line.starts_with("error: "))
        .filter(|line| normalize_layout_error(line).is_none())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !unreadable.is_empty() {
        return Err(format!(
            "source-layout audit reported failures this gate cannot read, so it \
             cannot say whether they are known:\n{}",
            unreadable.join("\n")
        ));
    }
    let mut observed_flags = BTreeSet::new();
    let mut observed_sizes = BTreeMap::new();
    for failure in text.lines().filter_map(normalize_layout_error) {
        match failure {
            LayoutFailure::Flag(identity) => {
                observed_flags.insert(identity);
            }
            LayoutFailure::Size { path, lines } => {
                // One path can be reported by more than one pass; the largest
                // reading is the one the ceiling answers.
                let entry = observed_sizes.entry(path).or_insert(lines);
                *entry = (*entry).max(lines);
            }
        }
    }
    let ledger_path = repo.join("docs/source-layout-debt.txt");
    let ledger_text = std::fs::read_to_string(&ledger_path)
        .map_err(|error| format!("could not read {}: {error}", ledger_path.display()))?;
    let mut ledger_flags = BTreeSet::new();
    let mut ledger_ceilings = BTreeMap::new();
    for row in ledger_text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        match parse_layout_row(row)? {
            LayoutFailure::Flag(identity) => {
                ledger_flags.insert(identity);
            }
            LayoutFailure::Size { path, lines } => {
                ledger_ceilings.insert(path, lines);
            }
        }
    }

    let mut introduced = observed_flags
        .difference(&ledger_flags)
        .cloned()
        .collect::<Vec<_>>();
    let mut retired = ledger_flags
        .difference(&observed_flags)
        .cloned()
        .collect::<Vec<_>>();
    // GROWTH IS ITS OWN FAILURE, and the reason this gate exists in this form.
    // A ledgered file that is allowed to grow without limit is a file nobody
    // is watching; recording the size it was admitted at is what makes every
    // later commit answer for making it worse.
    let mut grown = Vec::new();
    for (path, lines) in &observed_sizes {
        match ledger_ceilings.get(path) {
            None => introduced.push(format!("large {path} ({lines} lines)")),
            Some(ceiling) if lines > ceiling => {
                grown.push(format!(
                    "{path} is {lines} lines, past its recorded {ceiling}"
                ));
            }
            Some(_) => {}
        }
    }
    for path in ledger_ceilings.keys() {
        if !observed_sizes.contains_key(path) {
            retired.push(format!("large {path}"));
        }
    }
    if introduced.is_empty() && retired.is_empty() && grown.is_empty() {
        return Ok(());
    }
    Err(format!(
        "source-layout debt ledger changed\nnew: {}\nretired: {}\ngrown: {}",
        display_set(&introduced),
        display_set(&retired),
        display_set(&grown)
    ))
}

/// One audit failure, in the form the ledger records it.
///
/// A size failure keeps its count, and that is the whole point of the split.
/// The identity used to be the path alone, so a file already in the ledger had
/// its size recorded once and never checked again -- which is how one test
/// file reached forty thousand lines with this gate green over every commit
/// that grew it. A flag failure has no magnitude and keeps exact identity.
enum LayoutFailure {
    Flag(String),
    Size { path: String, lines: usize },
}

fn normalize_layout_error(line: &str) -> Option<LayoutFailure> {
    let message = line.strip_prefix("error: ")?;
    if let Some(path) = message.strip_prefix("inline tests in ") {
        return Some(LayoutFailure::Flag(format!("inline-tests {path}")));
    }
    if let Some(path) = message.strip_prefix("direct library printing in ") {
        return Some(LayoutFailure::Flag(format!("direct-print {path}")));
    }
    let (path, rest) = message.split_once(" has ")?;
    let (count, _) = rest.split_once(" lines")?;
    let lines = count.parse::<usize>().ok()?;
    Some(LayoutFailure::Size {
        path: path.to_owned(),
        lines,
    })
}

/// A ledger row: `large <path> <ceiling>` for a size, `<category> <path>` for
/// a flag. The ceiling is a cap and not a measurement, so a file that shrinks
/// needs no edit; only growing past it does.
fn parse_layout_row(row: &str) -> Result<LayoutFailure, String> {
    if let Some(rest) = row.strip_prefix("large ") {
        let (path, ceiling) = rest
            .rsplit_once(' ')
            .ok_or_else(|| format!("ledger row needs a line ceiling: {row}"))?;
        let lines = ceiling
            .parse::<usize>()
            .map_err(|_| format!("ledger row has a non-numeric ceiling: {row}"))?;
        return Ok(LayoutFailure::Size {
            path: path.to_owned(),
            lines,
        });
    }
    Ok(LayoutFailure::Flag(row.to_owned()))
}

fn display_set(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_owned()
    } else {
        format!("\n  {}", values.join("\n  "))
    }
}

/// A proof that needs real hardware, reported when the hardware is absent.
///
/// The wrapper exits 2 when it cannot run, which is not a pass and not a
/// failure: on a machine with no writable render node the question was never
/// asked. Treating that as success would let the proof rot exactly the way an
/// unreferenced script does, and treating it as failure would make the offline
/// gate unrunnable on a build host. So it is reported by name instead, and the
/// operator reads whether their machine answered.
fn hardware_proof(repo: &Path, tool: &str, subject: &str) -> Result<String, String> {
    let status = Command::new(tool)
        .current_dir(repo)
        .status()
        .map_err(|error| format!("could not run {tool}: {error}"))?;
    match status.code() {
        Some(0) => Ok(format!("{subject}: proved on this host")),
        Some(2) => Ok(format!(
            "{subject}: not proved here, this host has no device"
        )),
        _ => Err(format!("{tool} exited with {status}")),
    }
}

fn clippy(repo: &Path) -> Result<(), String> {
    let status = Command::new("cargo")
        .current_dir(repo)
        .env("CLIPPY_CONF_DIR", repo)
        .args([
            "clippy",
            "--offline",
            "--workspace",
            "--all-features",
            "--all-targets",
            // Without a lint level this step exits zero on any number of
            // lints and can only fail on a compile error, so a green gate
            // never supported the claim it was read as making. The cargo
            // flags above already matched the invocation the plan names;
            // one separator did not, and a lint that the plan's own command
            // refuses reached master behind a green check.
            "--",
            "-D",
            "warnings",
        ])
        .status()
        .map_err(|error| format!("could not run cargo clippy: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo clippy exited with {status}"))
    }
}

fn command(repo: &Path, program: &str, arguments: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .current_dir(repo)
        .args(arguments)
        .status()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {arguments:?} exited with {status}"))
    }
}

fn workspace_tests(repo: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let config =
        std::env::temp_dir().join(format!("sophia-test-config-{}-{nonce}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&config)
        .map_err(|error| format!("could not create isolated test config: {error}"))?;
    // Tests that exercise discovery provide their own fixtures. Every other
    // test must see compiled defaults, not the developer's current desktop.
    //
    // run_sophia_session.sh puts SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE in the
    // session environment, so every terminal opened inside a running Sophia
    // session inherits permission to attempt a destructive atomic modeset on
    // the card that session is already driving. The smoke then fails on a
    // card it was never going to get, and the gate's result depends on which
    // terminal invoked it. Ask for that smoke deliberately through
    // tools/atomic_scanout_smoke.sh instead.
    let result = Command::new("cargo")
        .current_dir(repo)
        .args(["test", "--offline", "--workspace", "--all-features"])
        .env("XDG_CONFIG_HOME", &config)
        .env_remove("SOPHIA_SHELL_CONFIG")
        .env_remove("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE")
        .status();
    let cleanup = std::fs::remove_dir_all(&config);
    let status = result.map_err(|error| format!("could not run workspace tests: {error}"))?;
    cleanup.map_err(|error| format!("could not remove isolated test config: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("workspace tests exited with {status}"))
    }
}

fn command_quiet(repo: &Path, program: &str, arguments: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .current_dir(repo)
        .args(arguments)
        .stdout(Stdio::null())
        .status()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {arguments:?} exited with {status}"))
    }
}

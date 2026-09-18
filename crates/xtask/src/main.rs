//! The workspace's human- and CI-facing development command dispatcher.
//!
//! Two seams live here. Session arguments were built in bash and consumed by
//! `PersistentXtermSessionConfig::from_args`; evidence records are emitted by
//! Rust and were parsed by `grep` in the verifiers. Both were untyped, and
//! three physical runs died in the first because nothing asked whether a
//! vector was acceptable until the display manager was already down.
//!
//! Shell keeps `sudo sv`, `chvt`, traps, and process waits. That code has been
//! reliable; the string-building has not.

use std::path::{Path, PathBuf};
use std::process::Command;

mod check;
mod m3_acceptance;
mod panel;

use sophia_conformance::{
    desktop_comparison, direct_scanout, direct_scanout_archive, direct_scanout_gate, profile,
};

fn main() -> std::process::ExitCode {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    match run(&arguments) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(arguments: &[String]) -> Result<(), String> {
    match arguments.first().map(String::as_str) {
        Some("check") => check::run(&workspace_root()?, &arguments[1..]).map(print_lines),
        Some("panel") => panel::run(&workspace_root()?, &arguments[1..]),
        Some("dock") => match &arguments[1..] {
            [command, paths @ ..] if command == "profile" => {
                print!("{}", sophia_conformance::dock::profile(paths)?);
                Ok(())
            }
            [command, path] if command == "verify" => {
                use std::io::Read;
                let mut text = String::new();
                std::fs::File::open(path)
                    .map_err(|e| e.to_string())?
                    .take(64 * 1024 * 1024 + 1)
                    .read_to_string(&mut text)
                    .map_err(|e| e.to_string())?;
                println!("{}", sophia_conformance::dock::verify(&text)?);
                Ok(())
            }
            _ => Err(
                "usage: xtask dock profile LOM CONFIG BEMENU PROVLITA CONFIG | dock verify LOG"
                    .into(),
            ),
        },
        Some("profile") => run_profile(&arguments[1..]),
        Some("conformance") => run_conformance(&arguments[1..]),
        // Compatibility aliases for callers introduced before grouping.
        Some("session-args") => print_profile_args(&arguments[1..]),
        Some("check-profiles") => check_profiles(&arguments[1..]),
        Some("verify") => legacy_verify(&arguments[1..]).map(print_lines),
        Some("--help" | "-h") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

fn print_profile_args(arguments: &[String]) -> Result<(), String> {
    profile::resolve(arguments).map(|vector| {
        for argument in vector {
            println!("{argument}");
        }
    })
}

fn check_profiles(arguments: &[String]) -> Result<(), String> {
    profile::check_every_profile(arguments).map(print_accepted_profiles)
}

fn print_accepted_profiles(accepted: Vec<(&'static str, usize)>) {
    for (name, arguments) in accepted {
        println!(
            "sophia_xtask_profile schema=1 status=accepted profile={name} arguments={arguments}"
        );
    }
}

fn run_profile(arguments: &[String]) -> Result<(), String> {
    match arguments.first().map(String::as_str) {
        Some("args") => print_profile_args(&arguments[1..]),
        Some("check") => check_profiles(&arguments[1..]),
        Some(other) => Err(format!("unknown profile command {other:?}")),
        None => Err("profile needs a command".to_owned()),
    }
}

fn run_conformance(arguments: &[String]) -> Result<(), String> {
    match arguments {
        [command, subject, log] if command == "verify" && subject == "panel" => {
            let log = std::fs::read_to_string(log).map_err(|error| error.to_string())?;
            sophia_conformance::panel::verify(&log).map(|line| println!("{line}"))
        }
        [subject, rest @ ..] if subject == "desktop-comparison" => run_desktop_comparison(rest),
        [command, subject, logs @ ..] if command == "verify" && subject == "direct-scanout" => {
            direct_scanout::verify_logs(logs).map(print_lines)
        }
        [command, subject, logs @ ..]
            if command == "verify" && subject == "direct-scanout-standalone" =>
        {
            verify_direct_scanout_standalone(logs)
        }
        // The overlay-requiring verification the gate runs, callable on its
        // own so a refused gate can be diagnosed against the evidence it
        // bound instead of re-deriving the rules by hand.
        [command, subject, logs @ ..]
            if command == "verify" && subject == "direct-scanout-overlay" =>
        {
            direct_scanout::verify_standalone_logs_with_overlay(logs, true).map(print_lines)
        }
        [command, subject, logs @ ..]
            if command == "verify" && subject == "direct-scanout-cost" =>
        {
            direct_scanout::verify_standalone_logs_with(logs, true, true).map(print_lines)
        }
        [command, subject, logs @ ..]
            if command == "verify" && subject == "direct-scanout-cursor" =>
        {
            direct_scanout::verify_standalone_logs_proving(logs, false, false, true)
                .map(print_lines)
        }
        [command, subject, rest @ ..]
            if command == "verify" && subject == "direct-scanout-archive" =>
        {
            verify_direct_scanout_archive(rest)
        }
        [command, subject, rest @ ..] if command == "bind" && subject == "direct-scanout" => {
            bind_direct_scanout(rest)
        }
        [command, subject, rest @ ..] if command == "archive" && subject == "direct-scanout" => {
            archive_direct_scanout(rest)
        }
        [command, subject, rest @ ..] if command == "run" && subject == "direct-scanout" => {
            run_direct_scanout(rest)
        }
        [command, subject, rest @ ..] if command == "gate" && subject == "direct-scanout" => {
            gate_direct_scanout(rest)
        }
        [command, subject, ..] if command == "verify" => {
            Err(format!("unknown conformance subject {subject:?}"))
        }
        [command, subject, ..] => Err(format!(
            "unknown conformance command {command:?} for {subject:?}"
        )),
        [command] => Err(format!("conformance command {command:?} needs a subject")),
        [] => Err("conformance needs a command".to_owned()),
    }
}

fn run_desktop_comparison(arguments: &[String]) -> Result<(), String> {
    let repo = workspace_root()?;
    match arguments {
        [command, source, prefix] if command == "install-reference" => {
            desktop_comparison::install_reference(
                &repo,
                Path::new(source),
                Path::new(prefix),
            )
            .map(print_lines)
        }
        [command, run] if command == "prepare" => {
            build_release_sophia(&repo)?;
            desktop_comparison::prepare(&repo, Path::new(run)).map(print_lines)
        }
        [command, run] if command == "prepare-soak" => {
            build_release_sophia(&repo)?;
            desktop_comparison::prepare_optional_soak(&repo, Path::new(run)).map(print_lines)
        }
        [command, root] if command == "cursor-theme" => {
            desktop_comparison::write_x11_core_cursor_theme(Path::new(root)).map(print_lines)
        }
        [command, run] if command == "gate" => {
            gate_desktop_comparison(&repo, Path::new(run))
        }
        [command, run] if command == "status" => {
            desktop_comparison::status(&repo, Path::new(run)).map(print_lines)
        }
        [command, run] if command == "preflight" => {
            desktop_comparison::preflight(&repo, Path::new(run)).map(print_lines)
        }
        [command, run] if command == "capture" => {
            desktop_comparison::capture_next(&repo, Path::new(run)).map(print_lines)
        }
        [command, run] if command == "qualify" => {
            desktop_comparison::qualify(&repo, Path::new(run)).map(print_lines)
        }
        [command, run] if command == "finalize" => {
            desktop_comparison::finalize_next(&repo, Path::new(run)).map(print_lines)
        }
        [command, run, pid] if command == "attest" => {
            let supervisor_pid = pid
                .parse::<u32>()
                .map_err(|_| "session supervisor PID is not an integer".to_owned())?;
            desktop_comparison::attest_session_auto(&repo, Path::new(run), supervisor_pid)
                .map(print_lines)
        }
        [command, run, pid, crtc] if command == "attest" => {
            let supervisor_pid = pid
                .parse::<u32>()
                .map_err(|_| "session supervisor PID is not an integer".to_owned())?;
            let crtc = crtc
                .parse::<u64>()
                .map_err(|_| "session CRTC is not an integer".to_owned())?;
            desktop_comparison::attest_session(
                &repo,
                Path::new(run),
                supervisor_pid,
                crtc,
            )
            .map(print_lines)
        }
        [command, run, attempt] if command == "replay" => {
            desktop_comparison::replay_attempt(Path::new(run), Path::new(attempt))
                .map(|replay| vec![replay.sample_record])
                .map(print_lines)
        }
        [command, kind, seconds] if command == "workload" && kind == "kitty-stream" => {
            let seconds = seconds
                .parse::<u64>()
                .map_err(|_| "kitty-stream duration is not an integer".to_owned())?;
            desktop_comparison::run_stream(seconds)
        }
        [command, run] if command == "verify" => {
            desktop_comparison::verify(&repo, Path::new(run)).map(print_lines)
        }
        [command, run] if command == "report" => {
            desktop_comparison::report(&repo, Path::new(run)).map(print_lines)
        }
        [command, ..] => Err(format!(
            "desktop-comparison {command:?} has invalid arguments; expected install-reference, prepare, prepare-soak, cursor-theme, gate, status, attest, preflight, qualify, capture, finalize, replay, verify, or report"
        )),
        [] => Err("desktop-comparison needs install-reference, prepare, prepare-soak, cursor-theme, gate, status, attest, preflight, qualify, capture, finalize, replay, verify, or report".to_owned()),
    }
}

fn gate_desktop_comparison(repo: &Path, run: &Path) -> Result<(), String> {
    desktop_comparison::status(repo, run)?;
    desktop_comparison::require_candidate_checkout(repo, run)?;
    desktop_comparison::verify_host_tool_versions()?;
    build_release_sophia(repo)?;
    desktop_comparison::require_candidate_checkout(repo, run)?;
    desktop_comparison::verify_prepared_binaries(repo, run)?;
    let adapter = repo.join("tools/desktop_comparison_tty3.sh");
    if !adapter.is_file() {
        return Err(format!(
            "desktop-comparison TTY adapter is missing: {}",
            adapter.display()
        ));
    }
    let xtask = std::env::current_exe()
        .map_err(|error| format!("could not identify the running xtask: {error}"))?;
    let status = Command::new(&adapter)
        .arg(run)
        .env("SOPHIA_DESKTOP_COMPARISON_XTASK", xtask)
        .status()
        .map_err(|error| format!("could not start {}: {error}", adapter.display()))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "desktop-comparison one-row gate exited with {status}"
        ))
    }
}

fn build_release_sophia(repo: &Path) -> Result<(), String> {
    let build = Command::new("cargo")
        .current_dir(repo)
        .args([
            "build",
            "--quiet",
            "--offline",
            "--release",
            "-p",
            "sophia-cli",
            "--features",
            "native-session",
        ])
        .status()
        .map_err(|error| format!("could not build the release Sophia candidate: {error}"))?;
    if !build.success() {
        return Err(format!(
            "release Sophia candidate build exited with {build}"
        ));
    }
    Ok(())
}

fn run_direct_scanout(arguments: &[String]) -> Result<(), String> {
    let probe = direct_scanout_gate::Probe::from_arguments(arguments)?;
    println!(
        "Direct scanout probe: {} at {}x{}, holding {}s, no window manager or chrome.",
        probe.workload, probe.width, probe.height, probe.hold_seconds
    );
    println!("Confirm that the client fills the screen edge to edge.");
    println!("Ctrl+Alt+Backspace is the only early exit; the client ends the normal session.");
    let repo = workspace_root()?;
    let client = std::env::var_os("SOPHIA_STANDALONE_APP_BIN").map(PathBuf::from);
    direct_scanout_gate::run_probe(&repo, &probe, client.as_deref())
}

fn verify_direct_scanout_standalone(arguments: &[String]) -> Result<(), String> {
    let logs = match arguments {
        [] => vec![default_standalone_log()?.display().to_string()],
        [log] if log.is_empty() => vec![default_standalone_log()?.display().to_string()],
        logs => logs.to_vec(),
    };
    direct_scanout::verify_standalone_logs(&logs).map(print_lines)
}

fn default_standalone_log() -> Result<PathBuf, String> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state).join("sophia/standalone-session/session.log"));
    }
    let home = std::env::var_os("HOME").ok_or("HOME and XDG_STATE_HOME are unset")?;
    Ok(PathBuf::from(home).join(".local/state/sophia/standalone-session/session.log"))
}

fn gate_direct_scanout(arguments: &[String]) -> Result<(), String> {
    // Parsed by `Probe`, which owns the argument vocabulary: the gate and the
    // probe run the same session, and two spellings of the same options would
    // let them drift.
    let probe = direct_scanout_gate::Probe::from_arguments(arguments)?;
    let repo = workspace_root()?;
    println!("Building and running the exact physical-proof binary...");
    if probe.overlay_proof {
        println!("Overlay proof: the session will open an overlay over a direct frame.");
    }
    if probe.cost {
        println!("Cost run: the overlay holds long enough to measure composed frames.");
    }
    if probe.cursor {
        println!("Cursor proof: the session moves a cursor over directly scanned frames.");
    }
    if probe.atomic_cursor {
        println!("Atomic cursor: the cursor rides a plane rather than the legacy ioctl.");
    }
    let report = direct_scanout_gate::run_gate_with(&repo, &probe)?;
    println!("Sophia commit:  {}", report.source_commit);
    println!("Sophia binary:  {}", report.sophia_sha256);
    println!(
        "Client:         {} ({})",
        report.client.display(),
        report.client_sha256
    );
    println!("Direct scanout gate passed: {}", report.archive.display());
    Ok(())
}

fn bind_direct_scanout(arguments: &[String]) -> Result<(), String> {
    let [session, evidence, commit, sophia, client, core, desktop] = arguments else {
        return Err(
            "bind direct-scanout expects SESSION_LOG EVIDENCE COMMIT SOPHIA CLIENT CORE_CONFIG DESKTOP_PROFILE"
                .to_owned(),
        );
    };
    direct_scanout_archive::bind_evidence(&direct_scanout_archive::BindEvidence {
        session_log: Path::new(session),
        evidence: Path::new(evidence),
        source_commit: commit,
        sophia_binary: Path::new(sophia),
        client_binary: Path::new(client),
        core_config: Path::new(core),
        desktop_profile: Path::new(desktop),
    })?;
    println!("Bound direct-scanout evidence: {evidence}");
    Ok(())
}

fn archive_direct_scanout(arguments: &[String]) -> Result<(), String> {
    let [evidence, run_root, sophia, client] = arguments else {
        return Err("archive direct-scanout expects EVIDENCE RUN_ROOT SOPHIA CLIENT".to_owned());
    };
    let repo = workspace_root()?;
    let run = direct_scanout_archive::create_archive(&direct_scanout_archive::CreateArchive {
        repo: &repo,
        evidence: Path::new(evidence),
        run_root: Path::new(run_root),
        sophia_binary: Path::new(sophia),
        client_binary: Path::new(client),
    })?;
    println!("Recorded verified direct-scanout run: {}", run.display());
    Ok(())
}

fn verify_direct_scanout_archive(arguments: &[String]) -> Result<(), String> {
    let repo = workspace_root()?;
    let run = match arguments {
        [] => direct_scanout_archive::newest_archive(&default_direct_scanout_run_root()?)?,
        [run] if run.is_empty() => {
            direct_scanout_archive::newest_archive(&default_direct_scanout_run_root()?)?
        }
        [run] => PathBuf::from(run),
        _ => return Err("verify direct-scanout-archive accepts at most one RUN".to_owned()),
    };
    direct_scanout_archive::verify_archive(&repo, &run)?;
    println!("Direct-scanout archive verified: {}", run.display());
    Ok(())
}

fn workspace_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "the xtask manifest has no workspace root".to_owned())
}

fn default_direct_scanout_run_root() -> Result<PathBuf, String> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(state)
            .join("sophia")
            .join("promotion")
            .join("direct-scanout-runs"));
    }
    let home = std::env::var_os("HOME")
        .ok_or("HOME is not set and XDG_STATE_HOME does not select an archive root")?;
    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("sophia")
        .join("promotion")
        .join("direct-scanout-runs"))
}

fn legacy_verify(arguments: &[String]) -> Result<Vec<String>, String> {
    match arguments {
        [subject, logs @ ..] if subject == "direct-scanout" => direct_scanout::verify_logs(logs),
        [subject, ..] => Err(format!("unknown verification {subject:?}")),
        [] => Err("verify needs a subject".to_owned()),
    }
}

fn print_lines(lines: Vec<String>) {
    for line in lines {
        println!("{line}");
    }
}

const USAGE: &str = "\
usage: cargo xtask <command>

  panel --quickshell=/PATH [--renderer=gpu|software]
        [--probe --renderer=software --wm=/PATH --display=:298] [--output=/NEW/DIR]
      Launch the opt-in X11 panel with renderer and binary identity logs.

  check [layout]
      Run the full offline gate, or only the exact source-layout debt gate.

  check m3-acceptance --output=/NEW/DIR --target-dir=/OWNED/TARGET [--self-test]
  check m4-acceptance --output=/NEW/DIR --target-dir=/OWNED/TARGET [--self-test]
  check m3-components --suite=NAME --output=/NEW/DIR --target-dir=/OWNED/TARGET
      Run only the device-hidden M3 acceptance harness; missing cases fail the gate.

  profile args --profile=<name> [--display=<name>] [key=value ...]
      Print the validated live-session argument vector for one profile.

  profile check
      Build and validate every profile's argument vector.

  conformance verify direct-scanout[-standalone] <log>...
      Verify typed direct-scanout evidence and optional session shape.

  conformance verify direct-scanout-{overlay,cost,cursor} <log>...
      Verify the effect-fallback, direct-versus-composed cost, and hardware
      cursor evidence a probe run produces.

  conformance run direct-scanout WIDTH HEIGHT HOLD WORKLOAD [PROOF]
  conformance gate direct-scanout [PROOF]
      Drive one probe session, or run the signed physical gate around it.
      PROOF is one of --overlay-proof, --cost, --cursor, --atomic-cursor.

  conformance bind direct-scanout SESSION_LOG EVIDENCE COMMIT SOPHIA CLIENT CORE DESKTOP
      Copy session evidence and append its typed source/binary identity.

  conformance archive direct-scanout EVIDENCE RUN_ROOT SOPHIA CLIENT
      Verify and record one immutable direct-scanout archive.

  conformance verify direct-scanout-archive [RUN]
      Re-verify one archive, or the newest archive when RUN is omitted.

  conformance desktop-comparison install-reference XLIBRE_SOURCE PREFIX
  conformance desktop-comparison prepare RUN
  conformance desktop-comparison prepare-soak RUN
  conformance desktop-comparison cursor-theme ROOT
  conformance desktop-comparison gate RUN
  conformance desktop-comparison status RUN
  conformance desktop-comparison attest RUN SUPERVISOR_PID [CRTC]
  conformance desktop-comparison preflight RUN
  conformance desktop-comparison qualify RUN
  conformance desktop-comparison capture RUN
  conformance desktop-comparison finalize RUN
  conformance desktop-comparison replay RUN ATTEMPT
  conformance desktop-comparison verify RUN
  conformance desktop-comparison report RUN
      Prepare, inspect, replay, verify, and reduce the diagnostic native desktop matrix.

compatibility aliases: session-args, check-profiles, verify direct-scanout
  dock profile LOM LOM_CONFIG BEMENU PROVLITA DOCK_CONFIG
  dock verify HOST_LOG
      Generate WM-preserving three-component overrides or check a dock smoke transcript.
profiles: hagia native standalone kitty
";

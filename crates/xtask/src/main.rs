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

mod c_desktop_sdk;
mod check;
mod git_tree;
mod headless_client_gate;
mod m3_acceptance;
mod native_protocol_family;
mod nine_p_conformance;
mod rust_desktop_sdk;
mod xterm_pointer_oracle;
mod xtest_selection;

use sophia_conformance::{direct_scanout, direct_scanout_archive, profile};

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
        Some("vendor-rust-desktop-sdk") => {
            rust_desktop_sdk::vendor(&workspace_root()?, &arguments[1..]).map(print_lines)
        }
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

  check [layout]
      Run the full offline gate, or only the exact source-layout debt gate.

  check rust-desktop-sdk
      Verify the vendored Rust desktop SDK snapshot, then run its own tests.

  vendor-rust-desktop-sdk SDK_CHECKOUT REVISION
      Replace the vendored Rust desktop SDK with a signed revision, offline; the
      new snapshot must verify before the old one is replaced.

  check native-protocol-family --output=/NEW/DIR --target-dir=/OWNED/TARGET
        [--timeout=3600]
      Run all retained role corpora and independent clients with devices hidden.

  check m3-acceptance --output=/NEW/DIR --target-dir=/OWNED/TARGET [--self-test]
  check m4-acceptance --output=/NEW/DIR --target-dir=/OWNED/TARGET [--self-test]
  check m5-acceptance --output=/NEW/DIR --target-dir=/OWNED/TARGET [--self-test]
  check m3-components --suite=NAME --output=/NEW/DIR --target-dir=/OWNED/TARGET
      Run only the device-hidden M3 acceptance harness; missing cases fail the gate.

  check x11-profile --profile=xtest|native-input|all --output=/NEW/DIR --target-dir=/OWNED/TARGET
        [--timeout=SECONDS] [--xts-root=/XTS --xts-expected=/PURPOSES.json [--xts-scenario=NAME]
        [--xts-timeout=SECONDS] [--xts-admit-xtest=yes|no]]
        [--x11bench-bin=/X11BENCH --x11bench-expected=/TESTS.json [--x11bench-timeout=SECONDS]]
      Run the X11 conformance profiles through the probe's own entry on the committed
      source; XTS5 and x11bench are each BLOCKED unless their checkout and manifest are supplied.
      --xts-admit-xtest=yes starts the XTS host with XTEST admitted, for a scenario whose
      purposes inject input (the event section).

  check 9p-conformance [--self-test]
      Serve the sophia-9p static test export and judge it with the independent Go oracle
      (tools/9p-oracle, pinned hugelgupf/p9 v0.4.1); --self-test requires every harness
      mutation to fail. Headless: no role, admission, mount or performance claim.

  check m6-evidence --output=/NEW/DIR --target-dir=/OWNED/TARGET [--timeout=SECONDS]
        [--core-report=/PATH/report.json] [--canonical-report=/PATH/report.json]
        [--xts-root=/XTS --xts-expected=/PURPOSES.json]
      Run m3, m4 and m5 acceptance and both X11 profiles on one committed source, cite
      the core baseline and the contained canonical run by path, and compose one verdict;
      anything absent, errored or on other bytes is NORESULT, never a pass.

  profile args --profile=<name> [--display=<name>] [key=value ...]
      Print the validated live-session argument vector for one profile.

  profile check
      Build and validate every profile's argument vector.

  conformance verify direct-scanout[-standalone] <log>...
      Verify typed direct-scanout evidence and optional session shape.

  conformance verify direct-scanout-{overlay,cost,cursor} <log>...
      Verify the effect-fallback, direct-versus-composed cost, and hardware
      cursor evidence a probe run produces.

  conformance bind direct-scanout SESSION_LOG EVIDENCE COMMIT SOPHIA CLIENT CORE DESKTOP
      Copy session evidence and append its typed source/binary identity.

  conformance archive direct-scanout EVIDENCE RUN_ROOT SOPHIA CLIENT
      Verify and record one immutable direct-scanout archive.

  conformance verify direct-scanout-archive [RUN]
      Re-verify one archive, or the newest archive when RUN is omitted.

compatibility aliases: session-args, check-profiles, verify direct-scanout
profiles: managed native standalone
";

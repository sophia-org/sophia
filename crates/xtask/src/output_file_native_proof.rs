//! Build the generic SDK peer and exercise it without devices. Native desktop
//! assembly and attended execution belong to external integration tooling.
use crate::output_file_performance::{artifact_for, digest, identity, process, sandbox, write};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(crate) const EXPORT_TESTS: &[&str] = &[
    "proof::fixtures_are_valid_and_b_differs_verifiably",
    "validate_stage_receives_exactly_one_validated_outcome",
    "reject_stage_unknown_mode_is_refused_at_transport_admission",
    "commit_restore_stage_commits_b_then_restores_a",
    "apply_await_termination_stage_is_ended_by_its_supervisor",
    "baseline_waits_below_the_declared_epoch_then_proceeds",
    "baseline_failures_exit_three_without_submitting",
    "outcome_before_termination_exits_five",
    "unterminated_peer_exits_six_at_its_deadline",
    "refused_arguments_exit_two_before_connecting",
    "outcome_with_the_wrong_topology_epoch_exits_four",
    "scripted_source_without_reuse_carries_commit_restore",
    "reused_publication_qid_exits_four",
];
const SESSION_TEST: &str = "live_session::reload::tests::desktop_launch_reload::output_file_recovery::native_session::native_proof_peer_uses_session_supervision_and_owner_settlement";

pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let [phase, output] = arguments else {
        return Err("expected prepare --output=/PRIVATE/NEW/DIR".into());
    };
    if phase != "prepare" {
        return Err("native proof preparation only; physical execution is external".into());
    }
    let output = PathBuf::from(
        output
            .strip_prefix("--output=")
            .ok_or("expected --output=")?,
    );
    if !output.is_absolute() || output.starts_with(repo) {
        return Err("output must be absolute and outside the source checkout".into());
    }
    let source = identity(repo)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&output)
        .map_err(|error| format!("create new preparation directory: {error}"))?;
    // Resolve before binding: an existing symlink in a parent cannot direct
    // build output back into the read-only source checkout.
    let output = output.canonicalize().map_err(|error| error.to_string())?;
    if output.starts_with(repo.canonicalize().map_err(|error| error.to_string())?) {
        return Err("resolved output is inside the source checkout".into());
    }
    let result = prepare(repo, &output, &source);
    write(
        &output,
        "preparation-result.json",
        &json!({"schema":1,"passed":result.is_ok(),"error":result.as_ref().err(),
            "source":source,"native_acceptance":false}),
    )?;
    result?;
    Ok(vec![format!(
        "output native proof peer preparation: PASS; evidence {}; no native acceptance",
        output.display()
    )])
}

fn prepare(repo: &Path, output: &Path, source: &serde_json::Value) -> Result<(), String> {
    fs::create_dir_all(output.join("cargo-home/registry")).map_err(|e| e.to_string())?;
    let sandbox = sandbox(repo, output)?;
    let sdk_source = repo.join("vendor/c-desktop-sdk/source");
    let mut sdk = sandbox.command("make");
    sdk.arg("-C")
        .arg(&sdk_source)
        .args(["-j1", "CC=cc", "CFLAGS=-O2"])
        .arg(format!("BUILD={}", output.join("sdk").display()))
        .arg("all");
    process::run(&mut sdk, output, "sdk-build", Duration::from_secs(600))?;
    let mut cc = sandbox.command("cc");
    cc.args([
        "-std=c99",
        "-O2",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-pedantic",
        "-UNDEBUG",
    ])
    .arg("-I")
    .arg(sdk_source.join("src"))
    .arg(repo.join("crates/sophia-runtime/tests/support/output_files_native_proof_peer.c"))
    .arg("-L")
    .arg(output.join("sdk"))
    .args(["-lsophia-desktop", "-lsophia-9p"])
    .arg("-o")
    .arg(output.join("peer"));
    process::run(&mut cc, output, "peer-build", Duration::from_secs(120))?;
    let mut cargo = sandbox.command("cargo");
    cargo.args([
        "test",
        "--offline",
        "--locked",
        "-p",
        "sophia-runtime",
        "--release",
        "--test",
        "output_file_native_proof",
        "--no-run",
        "--message-format=json",
    ]);
    process::run(&mut cargo, output, "rust-build", Duration::from_secs(1800))?;
    let messages =
        fs::read_to_string(output.join("rust-build.stdout")).map_err(|e| e.to_string())?;
    let executable = artifact_for(&messages, "output_file_native_proof")?;
    fs::copy(executable, output.join("harness")).map_err(|e| e.to_string())?;
    let peer_digest = digest(&output.join("peer"))?;
    let harness_digest = digest(&output.join("harness"))?;
    let mut check = sandbox.command("env");
    check
        .arg(format!(
            "SOPHIA_OUTPUT_NATIVE_PROOF_PEER={}",
            output.join("peer").display()
        ))
        .arg(output.join("harness"))
        .args(["--include-ignored", "--test-threads=1", "--nocapture"]);
    process::run(&mut check, output, "real-export", Duration::from_secs(180))?;
    let log = fs::read_to_string(output.join("real-export.stdout")).map_err(|e| e.to_string())?;
    check_export_result(&log)?;
    let mut cargo = sandbox.command("cargo");
    cargo.args([
        "test",
        "--offline",
        "--locked",
        "-p",
        "sophia-session",
        "--release",
        "--features",
        "native-session",
        "--lib",
        "--no-run",
        "--message-format=json",
    ]);
    process::run(
        &mut cargo,
        output,
        "session-build",
        Duration::from_secs(1800),
    )?;
    let messages =
        fs::read_to_string(output.join("session-build.stdout")).map_err(|e| e.to_string())?;
    let executable = artifact_for(&messages, "sophia_session")?;
    fs::copy(executable, output.join("session-harness")).map_err(|e| e.to_string())?;
    let session_digest = digest(&output.join("session-harness"))?;
    let mut check = sandbox.command("env");
    check
        .arg(format!(
            "SOPHIA_OUTPUT_NATIVE_PROOF_PEER={}",
            output.join("peer").display()
        ))
        .arg(output.join("session-harness"))
        .args([
            "--ignored",
            "--exact",
            SESSION_TEST,
            "--test-threads=1",
            "--nocapture",
        ]);
    process::run(
        &mut check,
        output,
        "session-supervision",
        Duration::from_secs(120),
    )?;
    let log =
        fs::read_to_string(output.join("session-supervision.stdout")).map_err(|e| e.to_string())?;
    if !log
        .lines()
        .any(|line| line.starts_with("test result: ok. 1 passed; 0 failed; 0 ignored;"))
    {
        return Err("missing successful exact Session fixture result".into());
    }
    if identity(repo)? != *source
        || peer_digest != digest(&output.join("peer"))?
        || harness_digest != digest(&output.join("harness"))?
        || session_digest != digest(&output.join("session-harness"))?
    {
        return Err("source or artifacts changed during preparation".into());
    }
    write(
        output,
        "prepared.json",
        &json!({"schema":1,"source":source,
        "peer_sha256":peer_digest,"harness_sha256":harness_digest,
        "session_harness_sha256":session_digest,
        "rust_profile":"release","cargo_jobs":1,"nice":19,
        "c_flags":"-std=c99 -O2 -Wall -Wextra -Werror -pedantic -UNDEBUG",
        "export_tests":EXPORT_TESTS,"session_test":SESSION_TEST,"device_hidden":true,"session_sockets_hidden":true,
        "network_hidden":true,"native_acceptance":false,
        "limits":"runtime and Session owners use supplied physical observations; native loop and KMS are not exercised; read-only offline registry needs extracted locked crates"}),
    )
}

pub(crate) fn check_export_result(log: &str) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for line in log.lines() {
        if let Some(name) = line
            .strip_prefix("test ")
            .and_then(|line| line.strip_suffix(" ... ok"))
            && !names.insert(name)
        {
            return Err(format!("duplicate test result: {name}"));
        }
    }
    if names != EXPORT_TESTS.iter().copied().collect() {
        return Err(format!("real-export test set differs: {names:?}"));
    }
    let prefix = format!(
        "test result: ok. {} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;",
        EXPORT_TESTS.len()
    );
    if !log.lines().any(|line| line.starts_with(&prefix)) {
        return Err("missing complete real-export result".into());
    }
    Ok(())
}

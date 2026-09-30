//! Opt-in output transport qualification. Preparation and measurement are
//! separate invocations so no compiler competes with the measured worker.
pub(crate) mod process;

use process::Sandbox;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub fn run(repo: &Path, args: &[String]) -> Result<Vec<String>, String> {
    let [phase, option] = args else {
        return Err("expected prepare|measure --output=/PRIVATE/DIR".into());
    };
    if !["prepare", "measure"].contains(&phase.as_str()) {
        return Err("expected prepare or measure".into());
    }
    let path = option
        .strip_prefix("--output=")
        .ok_or("expected --output=/PRIVATE/DIR")?;
    let output = PathBuf::from(path);
    if !output.is_absolute() || output.starts_with(repo) {
        return Err("output must be absolute and outside the source checkout".into());
    }
    if phase == "prepare" {
        prepare(repo, &output)?;
    } else {
        measure(repo, &output)?;
    }
    Ok(vec![format!(
        "output transport {phase}: PASS; evidence {}",
        output.display()
    )])
}

fn capture(repo: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "sophia-perf-capture-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|e| e.to_string())?;
    let mut command = Command::new(program);
    command.current_dir(repo).args(args);
    let result = process::run(&mut command, &directory, "capture", Duration::from_secs(30))
        .and_then(|()| {
            fs::read_to_string(directory.join("capture.stdout"))
                .map(|s| s.trim().to_owned())
                .map_err(|e| e.to_string())
        });
    let stderr = fs::read_to_string(directory.join("capture.stderr")).unwrap_or_default();
    let _ = fs::remove_dir_all(&directory);
    result.map_err(|e| format!("{program} {args:?}: {e}; {stderr}"))
}

fn identity(repo: &Path) -> Result<Value, String> {
    if !capture(
        repo,
        "git",
        &["status", "--porcelain", "--untracked-files=all"],
    )?
    .is_empty()
    {
        return Err("qualification requires a clean committed source tree".into());
    }
    capture(repo, "git", &["verify-commit", "HEAD"])?;
    let sdk = crate::c_desktop_sdk::verify(&repo.join("vendor/c-desktop-sdk"), repo)?;
    Ok(
        json!({"sophia_commit":capture(repo,"git", &["rev-parse","HEAD"])?,
        "sdk_revision":sdk,"sdk_manifest_sha256":digest(&repo.join("vendor/c-desktop-sdk/manifest.json"))?}),
    )
}

fn sandbox(repo: &Path, output: &Path) -> Result<Sandbox, String> {
    let toolchain = PathBuf::from(capture(repo, "rustc", &["--print", "sysroot"])?);
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))
        .ok_or("missing Cargo home")?;
    let registry = cargo_home
        .join("registry")
        .canonicalize()
        .map_err(|e| format!("Cargo registry: {e}"))?;
    Ok(Sandbox {
        repo: repo.into(),
        output: output.into(),
        toolchain,
        registry,
    })
}

fn prepare(repo: &Path, output: &Path) -> Result<(), String> {
    let source = identity(repo)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(output)
        .map_err(|e| format!("create new preparation directory: {e}"))?;
    fs::create_dir_all(output.join("cargo-home/registry")).map_err(|e| e.to_string())?;
    let sandbox = sandbox(repo, output)?;
    let result = (|| {
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
            "-Wconversion",
            "-Wshadow",
        ])
        .arg("-I")
        .arg(sdk_source.join("src"))
        .arg(repo.join("crates/sophia-runtime/tests/support/output_files_perf_peer.c"))
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
            "output_file_performance",
            "--no-run",
            "--message-format=json",
        ]);
        process::run(&mut cargo, output, "rust-build", Duration::from_secs(1800))?;
        let messages =
            fs::read_to_string(output.join("rust-build.stdout")).map_err(|e| e.to_string())?;
        let executable = artifact(&messages)?;
        // Copy to a fixed private path. Measurement checks both digests and
        // does not ask Cargo to rebuild or choose a different executable.
        fs::copy(&executable, output.join("harness")).map_err(|e| e.to_string())?;
        let mut versions = sandbox.command("rustc");
        versions.arg("-vV");
        process::run(
            &mut versions,
            output,
            "rustc-version",
            Duration::from_secs(30),
        )?;
        let mut versions = sandbox.command("cc");
        versions.arg("--version");
        process::run(&mut versions, output, "cc-version", Duration::from_secs(30))?;
        if identity(repo)? != source {
            return Err("source changed during preparation".into());
        }
        write(
            output,
            "prepared.json",
            &json!({"schema":1,"source":source,
            "peer_sha256":digest(&output.join("peer"))?,"harness_sha256":digest(&output.join("harness"))?,
            "rust_profile":"release", "cargo_jobs":1, "nice":19,
            "c_flags":"-std=c99 -O2 -Wall -Wextra -Werror -pedantic -Wconversion -Wshadow",
            "rust_flags":"workspace release profile; RUSTFLAGS unset",
            "registry":"read-only offline registry; all locked crates must already be extracted; host Cargo configuration is not imported",
            "evidence_limit":"completed workloads are flushed; an outer timeout or OOM may lose the current workload and cannot pass",
            "device_hidden":true,"network_hidden":true,"session_sockets_hidden":true}),
        )?;
        Ok(())
    })();
    write(
        output,
        "preparation-result.json",
        &json!({"passed":result.is_ok(),"error":result.as_ref().err()}),
    )?;
    result
}

fn measure(repo: &Path, output: &Path) -> Result<(), String> {
    let prepared: Value =
        serde_json::from_slice(&fs::read(output.join("prepared.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let source = identity(repo)?;
    if prepared["schema"] != 1
        || prepared["source"] != source
        || prepared["peer_sha256"] != digest(&output.join("peer"))?
        || prepared["harness_sha256"] != digest(&output.join("harness"))?
    {
        return Err("prepared artifacts or source identity changed".into());
    }
    // create_new refuses to overwrite either a failed run or its outliers.
    let lock = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("measurement-started"))
        .map_err(|e| format!("measurement may run only once per preparation: {e}"))?;
    drop(lock);
    let sandbox = sandbox(repo, output)?;
    write(output, "machine.json", &machine(repo)?)?;
    let mut command = sandbox.command("env");
    command
        .arg(format!(
            "SOPHIA_OUTPUT_PERF_PEER={}",
            output.join("peer").display()
        ))
        .arg(format!(
            "SOPHIA_OUTPUT_PERF_EVIDENCE={}",
            output.join("samples.jsonl").display()
        ))
        .arg(output.join("harness"))
        .args([
            "--ignored",
            "--exact",
            "output_file_performance",
            "--test-threads=1",
            "--nocapture",
        ]);
    let mut result = process::run(
        &mut command,
        output,
        "measurement",
        Duration::from_secs(1800),
    );
    if result.is_ok() {
        result = (|| {
            if identity(repo)? != source {
                return Err("source changed during measurement".into());
            }
            if prepared["peer_sha256"] != digest(&output.join("peer"))?
                || prepared["harness_sha256"] != digest(&output.join("harness"))?
            {
                return Err("prepared artifact changed during measurement".into());
            }
            let data =
                fs::read_to_string(output.join("samples.jsonl")).map_err(|e| e.to_string())?;
            let final_record: Value =
                serde_json::from_str(data.lines().last().ok_or("missing measurement result")?)
                    .map_err(|e| e.to_string())?;
            if final_record["kind"] != "result" || final_record["passed"] != true {
                return Err("missing successful measurement result".into());
            }
            Ok(())
        })();
    }
    write(
        output,
        "measurement-result.json",
        &json!({"passed":result.is_ok(),"error":result.as_ref().err(),
        "source":source,"native_acceptance":false}),
    )?;
    result
}

pub(crate) fn artifact(messages: &str) -> Result<PathBuf, String> {
    let mut found = None;
    for line in messages.lines() {
        let message: Value = serde_json::from_str(line).map_err(|e| format!("Cargo JSON: {e}"))?;
        if message["reason"] == "compiler-artifact"
            && message["target"]["name"] == "output_file_performance"
            && message["profile"]["test"] == true
            && let Some(executable) = message["executable"].as_str()
            && found.replace(PathBuf::from(executable)).is_some()
        {
            return Err("multiple performance artifacts".into());
        }
    }
    found.ok_or_else(|| "Cargo did not produce the performance harness".into())
}

fn machine(repo: &Path) -> Result<Value, String> {
    let mut governors = Vec::new();
    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu/cpufreq") {
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            governors.push(json!({"policy":entry.file_name().to_string_lossy(),
                "governor":fs::read_to_string(entry.path().join("scaling_governor")).ok()}));
        }
    }
    Ok(json!({"kernel":capture(repo,"uname", &["-r"])?,
        "cpuinfo":fs::read_to_string("/proc/cpuinfo").map_err(|e|e.to_string())?,
        "governors":governors,"intel_no_turbo":fs::read_to_string("/sys/devices/system/cpu/intel_pstate/no_turbo").ok(),
        "boost":fs::read_to_string("/sys/devices/system/cpu/cpufreq/boost").ok()}))
}
fn digest(path: &Path) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)
    ))
}
fn write(output: &Path, name: &str, value: &Value) -> Result<(), String> {
    fs::write(
        output.join(name),
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

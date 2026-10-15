//! Paired diagnostic, deliberately separate from administrative acceptance.
use crate::output_file_performance::{artifact_for, digest, process, write};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

fn capture(repo: &Path, program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|e| e.to_string())?
        .trim()
        .into())
}
fn source(repo: &Path) -> Result<String, String> {
    if !capture(
        repo,
        "git",
        &["status", "--porcelain", "--untracked-files=all"],
    )?
    .is_empty()
    {
        return Err("diagnostic requires clean committed source".into());
    }
    capture(repo, "git", &["verify-commit", "HEAD"])?;
    capture(repo, "git", &["rev-parse", "HEAD"])
}
pub fn run(repo: &Path, args: &[String]) -> Result<Vec<String>, String> {
    let [phase, option] = args else {
        return Err("expected prepare|measure --output=/PRIVATE/DIR".into());
    };
    let output = PathBuf::from(
        option
            .strip_prefix("--output=")
            .ok_or("expected --output=")?,
    );
    if !output.is_absolute() || output.starts_with(repo) {
        return Err("output must be absolute and outside checkout".into());
    }
    let revision = source(repo)?;
    match phase.as_str() {
        "prepare" => {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&output)
                .map_err(|e| e.to_string())?;
            // The outer cargo-slot owns this pool entry for the entire invocation.
            let target =
                std::env::var_os("CARGO_TARGET_DIR").ok_or("run through cargo-slot sophia")?;
            let pool = PathBuf::from(std::env::var_os("HOME").ok_or("missing HOME")?)
                .join(".cache/cargo-slots");
            if !Path::new(&target).starts_with(pool) {
                return Err("requires cargo-slot target pool".into());
            }
            let mut cargo = Command::new("cargo");
            cargo.current_dir(repo).args([
                "test",
                "--offline",
                "--locked",
                "--release",
                "-p",
                "sophia-runtime",
                "--test",
                "admin_file_performance",
                "--no-run",
                "--message-format=json",
            ]);
            process::run(&mut cargo, &output, "build", Duration::from_secs(1800))?;
            let messages =
                fs::read_to_string(output.join("build.stdout")).map_err(|e| e.to_string())?;
            let executable = artifact_for(&messages, "admin_file_performance")?;
            if source(repo)? != revision {
                return Err("source changed during build".into());
            }
            write(
                &output,
                "prepared.json",
                &json!({"schema":1,"revision":revision,"binary":executable,
                "sha256":digest(&executable)?,"rustc":capture(repo,"rustc", &["-vV"])?,
                "target_dir":target,"qualification":false,
                "cargo_build_jobs":std::env::var("CARGO_BUILD_JOBS").ok(),
                "rustflags":std::env::var("RUSTFLAGS").ok(),
                "cargo_encoded_rustflags":std::env::var("CARGO_ENCODED_RUSTFLAGS").ok()}),
            )?;
        }
        "measure" | "qualify" => {
            let qualification = phase == "qualify";
            let prepared: Value = serde_json::from_slice(
                &fs::read(output.join("prepared.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let binary = PathBuf::from(prepared["binary"].as_str().ok_or("missing binary")?);
            if prepared["schema"] != 1
                || prepared["revision"] != revision
                || prepared["sha256"] != digest(&binary)?
            {
                return Err("prepared source or binary changed".into());
            }
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(output.join("measurement-started"))
                .map_err(|e| e.to_string())?;
            write(
                &output,
                "machine.json",
                &json!({"kernel":capture(repo,"uname", &["-a"])?,
                "cpuinfo":fs::read_to_string("/proc/cpuinfo").map_err(|e|e.to_string())?,
                "loadavg":fs::read_to_string("/proc/loadavg").map_err(|e|e.to_string())?,
                "nice":process::niceness()?,"qualification":qualification,"quiet_window":"not externally certified"}),
            )?;
            let mut command = Command::new("bwrap");
            command
                .args([
                    "--unshare-all",
                    "--die-with-parent",
                    "--clearenv",
                    "--ro-bind",
                    "/usr",
                    "/usr",
                    "--symlink",
                    "usr/lib",
                    "/lib",
                    "--symlink",
                    "usr/lib",
                    "/lib64",
                    "--ro-bind",
                    "/etc",
                    "/etc",
                    "--proc",
                    "/proc",
                    "--dev",
                    "/dev",
                    "--tmpfs",
                    "/tmp",
                    "--tmpfs",
                    "/run",
                    "--ro-bind",
                ])
                .arg(&binary)
                .arg(&binary)
                .arg("--bind")
                .arg(&output)
                .arg(&output)
                .args(["--setenv", "SOPHIA_ADMIN_PERF_EVIDENCE"])
                .arg(output.join("samples.jsonl"))
                .args(["--setenv", "SOPHIA_ADMIN_PERF_QUALIFY"])
                .arg(if qualification { "1" } else { "0" })
                .arg(&binary)
                .args([
                    "--ignored",
                    "--exact",
                    "admin_file_performance",
                    "--test-threads=1",
                    "--nocapture",
                ]);
            let result = process::run(
                &mut command,
                &output,
                "measurement",
                Duration::from_secs(600),
            )
            .and_then(|()| {
                if source(repo)? != revision || prepared["sha256"] != digest(&binary)? {
                    return Err("source or binary changed during measurement".into());
                }
                let text =
                    fs::read_to_string(output.join("samples.jsonl")).map_err(|e| e.to_string())?;
                let last: Value =
                    serde_json::from_str(text.lines().last().ok_or("missing final record")?)
                        .map_err(|e| e.to_string())?;
                if last["kind"] != "result" || last["completed"] != true {
                    return Err("incomplete diagnostic".into());
                }
                if last["qualification"] != qualification
                    || (qualification && last["budgets_pass"] != true)
                {
                    return Err("administrative qualification did not pass".into());
                }
                Ok(())
            });
            write(
                &output,
                "result.json",
                &json!({"completed":result.is_ok(),"error":result.as_ref().err(),"qualification":qualification,"revision":revision}),
            )?;
            result?;
        }
        _ => return Err("expected prepare, measure or qualify".into()),
    }
    Ok(vec![format!(
        "administrative transport {phase} complete: {}",
        output.display()
    )])
}

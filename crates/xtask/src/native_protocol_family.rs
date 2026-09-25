//! One device-hidden entry for retained role corpora and owning reducers.
use std::fs::{self, File};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;

struct Options {
    output: PathBuf,
    target: PathBuf,
    hagia: PathBuf,
    narthex: PathBuf,
    timeout: Duration,
}

pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let options = options(repo, arguments)?;
    for (root, marker) in [
        (&options.hagia, "hagia.nimble"),
        (&options.narthex, "src/narthex.nim"),
    ] {
        if !root.join(marker).is_file() {
            return Err(format!(
                "required independent checkout missing: {}",
                root.display()
            ));
        }
    }
    let identities = identities(repo, &options)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&options.output)
        .map_err(|e| {
            format!(
                "create new evidence directory {}: {e}",
                options.output.display()
            )
        })?;
    for name in ["tmp", "tmp/config", "tmp/runtime"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(options.output.join(name))
            .map_err(|e| format!("create isolated {name}: {e}"))?;
    }
    fs::create_dir_all(&options.target).map_err(|e| format!("create target: {e}"))?;
    let started = Instant::now();
    let mut phases = Vec::new();
    for (name, program, args) in stages() {
        let log = options.output.join(format!("{name}.log"));
        let remaining = options.timeout.saturating_sub(started.elapsed());
        let result = if remaining.is_zero() {
            Err("family deadline expired".into())
        } else {
            stage(repo, &options, program, &args, remaining, &log)
        };
        phases.push(
            json!({"name": name, "status": if result.is_ok() { "pass" } else { "fail" },
            "log": log, "error": result.as_ref().err()}),
        );
        write_report(
            &options.output,
            &json!({
                "schema": 1, "status": if result.is_ok() { "running" } else { "fail" },
                "identities": identities, "phases": phases, "device_hidden": true,
                "native_acceptance": false,
            }),
        )?;
        result?;
    }
    // Retain mixed-source results, but never attribute them to one candidate.
    let final_identity = crate::native_protocol_family::identities(repo, &options);
    let coherent = final_identity.as_ref().is_ok_and(|end| identities == *end);
    write_report(
        &options.output,
        &json!({
            "schema": 1, "status": if coherent { "pass" } else { "noresult" },
            "identities": identities, "final_identities": final_identity.as_ref().ok(),
            "identity_error": final_identity.as_ref().err(), "phases": phases,
            "device_hidden": true, "native_acceptance": false,
            "supplied_facts": ["presentation completions", "output topology", "input activations"],
            "stable_roles": ["sophia_wm_v1_r3"], "experimental_roles": ["sophia_shell_v1_r8", "sophia_output_v1_r1"],
            "output_independent_lifecycle": false,
        }),
    )?;
    if !coherent {
        return Err("a source checkout changed during family conformance; see report.json".into());
    }
    Ok(vec![format!(
        "native protocol family: PASS; native acceptance not claimed; {}",
        options.output.join("report.json").display()
    )])
}

fn options(repo: &Path, args: &[String]) -> Result<Options, String> {
    let mut values = std::collections::BTreeMap::new();
    for arg in args {
        let (key, value) = arg.split_once('=').ok_or("expected --name=value")?;
        if ![
            "--output",
            "--target-dir",
            "--hagia-root",
            "--narthex-root",
            "--timeout",
        ]
        .contains(&key)
            || value.is_empty()
            || values.insert(key, value).is_some()
        {
            return Err(format!("invalid or repeated family option {arg:?}"));
        }
    }
    let absolute = |path: &str| {
        let p = PathBuf::from(path);
        if p.is_absolute() { p } else { repo.join(p) }
    };
    let output = absolute(
        values
            .get("--output")
            .ok_or("native-protocol-family requires --output=/NEW/DIR")?,
    );
    let target = absolute(
        values
            .get("--target-dir")
            .ok_or("native-protocol-family requires --target-dir=/OWNED/TARGET")?,
    );
    let sibling = repo.parent().ok_or("repository has no parent")?;
    let hagia = values
        .get("--hagia-root")
        .map(|p| absolute(p))
        .unwrap_or_else(|| sibling.join("hagia"));
    let narthex = values
        .get("--narthex-root")
        .map(|p| absolute(p))
        .unwrap_or_else(|| sibling.join("narthex"));
    let seconds = values
        .get("--timeout")
        .unwrap_or(&"3600")
        .parse::<u64>()
        .map_err(|_| "invalid family timeout")?;
    if !(1..=7200).contains(&seconds) {
        return Err("family timeout must be 1..7200 seconds".into());
    }
    Ok(Options {
        output,
        target,
        hagia,
        narthex,
        timeout: Duration::from_secs(seconds),
    })
}

type Stage = (&'static str, &'static str, Vec<&'static str>);

fn stages() -> Vec<Stage> {
    vec![
        (
            "isolation",
            "sh",
            vec![
                "-c",
                "test ! -e /dev/dri && test ! -e /dev/input && test -z \"${DISPLAY-}\" && test -z \"${WAYLAND_DISPLAY-}\"",
            ],
        ),
        (
            "wm-independent",
            "bash",
            vec!["tools/check_policy_client_matrix.sh"],
        ),
        (
            "shell-independent",
            "sh",
            vec!["tools/check_shell_protocol.sh"],
        ),
        // All integration targets include later grant, fairness and retirement
        // cases that the historic list of a few test names silently omitted.
        (
            "protocol-runtime",
            "cargo",
            vec![
                "test",
                "--offline",
                "-q",
                "-p",
                "sophia-protocol",
                "-p",
                "sophia-runtime",
                "--tests",
            ],
        ),
        (
            "engine-owners",
            "cargo",
            vec!["test", "--offline", "-q", "-p", "sophia-engine", "--tests"],
        ),
        (
            "output-live-owner",
            "cargo",
            vec![
                "test",
                "--offline",
                "-q",
                "-p",
                "sophia-session",
                "--features",
                "native-session",
                "--test",
                "live_output_authority",
            ],
        ),
        (
            "output-client",
            "cargo",
            vec![
                "test",
                "--offline",
                "-q",
                "-p",
                "sophia-wm-demo",
                "--test",
                "output_v1",
            ],
        ),
        // Control shares the envelope, but is not a supervised desktop role.
        (
            "control-service",
            "sh",
            vec!["tools/check_control_protocol.sh"],
        ),
    ]
}

fn stage(
    repo: &Path,
    options: &Options,
    program: &str,
    args: &[&str],
    remaining: Duration,
    log: &Path,
) -> Result<(), String> {
    let stdout = File::create(log).map_err(|e| format!("create phase log: {e}"))?;
    let stderr = stdout.try_clone().map_err(|e| e.to_string())?;
    // Hide devices and installed-session sockets. Protected shell hosts still
    // launch their own nested sandbox and enforce the real admission boundary.
    let status = Command::new("timeout")
        .args([
            "--signal=TERM",
            "--kill-after=5s",
            &format!("{}s", remaining.as_secs().max(1)),
        ])
        .args([
            "bwrap",
            "--die-with-parent",
            "--unshare-pid",
            "--bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/run/user",
            "--bind",
        ])
        .arg(options.output.join("tmp"))
        .arg("/tmp")
        .args(["--", program])
        .args(args)
        .current_dir(repo)
        .env("CARGO_TARGET_DIR", &options.target)
        .env("SOPHIA_HAGIA_ROOT", &options.hagia)
        .env("SOPHIA_NARTHEX_ROOT", &options.narthex)
        .env("XDG_CONFIG_HOME", "/tmp/config")
        .env("XDG_RUNTIME_DIR", "/tmp/runtime")
        .env("TMPDIR", "/tmp")
        .env_remove("DISPLAY")
        .env_remove("XAUTHORITY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("SOPHIA_WM_SOCKET")
        .env_remove("SOPHIA_SHELL_SOCKET")
        .env_remove("SOPHIA_OUTPUT_SOCKET")
        .env_remove("SOPHIA_SHELL_CONFIG")
        .env_remove("SOPHIA_DESKTOP_PROFILE")
        .env_remove("SOPHIA_HAGIA_BIN")
        .env_remove("SOPHIA_CONTROL_SANDBOX_PROOF")
        .env_remove("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE")
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .status()
        .map_err(|e| format!("start family phase: {e}"))?;
    if status.success() {
        if program == "cargo" {
            require_tests_ran(&fs::read_to_string(log).map_err(|e| e.to_string())?)?;
        }
        Ok(())
    } else {
        Err(format!("family phase exited {status}; {}", log.display()))
    }
}

/// A filtered or feature-disabled target can exit zero without asking a test.
pub(crate) fn require_tests_ran(log: &str) -> Result<(), String> {
    let ran = log
        .lines()
        .filter_map(|line| line.strip_prefix("test result: ok. "))
        .filter_map(|line| line.split_once(" passed;"))
        .filter_map(|(count, _)| count.parse::<usize>().ok())
        .any(|count| count > 0);
    if ran {
        Ok(())
    } else {
        Err("Cargo phase ran no tests; check features and target selection".into())
    }
}

fn identities(repo: &Path, options: &Options) -> Result<serde_json::Value, String> {
    Ok(
        json!({"sophia": identity(repo)?, "hagia": identity(&options.hagia)?, "narthex": identity(&options.narthex)?}),
    )
}

fn identity(root: &Path) -> Result<serde_json::Value, String> {
    use sha2::{Digest, Sha256};
    let git = |args: &[&str]| -> Result<Vec<u8>, String> {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!("git identity failed for {}", root.display()));
        }
        Ok(output.stdout)
    };
    let commit = String::from_utf8(git(&["rev-parse", "HEAD"])?).map_err(|e| e.to_string())?;
    let dirty = git(&["status", "--porcelain"])?;
    let diff = git(&["diff", "HEAD", "--binary"])?;
    // Untracked contents are absent from git diff; do not leave them unbound.
    if dirty
        .split(|b| *b == b'\n')
        .any(|line| line.starts_with(b"??"))
    {
        return Err(format!(
            "commit or stage untracked files before the family gate: {}",
            root.display()
        ));
    }
    Ok(
        json!({"root": root, "commit": commit.trim(), "dirty": !dirty.is_empty(), "diff_sha256": format!("{:x}", Sha256::digest(diff))}),
    )
}

fn write_report(output: &Path, report: &serde_json::Value) -> Result<(), String> {
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("write family report: {e}"))
}

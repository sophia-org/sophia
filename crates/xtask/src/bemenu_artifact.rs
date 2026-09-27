//! Prepare an immutable `bemenu-sophia` artifact from one signed revision for
//! the live launcher gate (crates/sophia-runtime/tests/shell_bemenu_files.rs).
//!
//! Signer AUTHORIZATION happens only here: `git verify-commit` must pass and
//! report status G. The live gate later re-checks content-to-revision BINDING
//! (commit object hash, tree, binary SHA-256, SDK pin) and never consults a
//! keyring. The source checkout is only read: the build runs in a fresh
//! scratch tree extracted from `git archive` of the signed commit, and that
//! tree must hash to exactly the commit's tree before anything is compiled.
use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: prepare-bemenu-artifact <source-repo> <signed-commit> <new-output-dir>";
const GIT_TIMEOUT: Duration = Duration::from_secs(60);
const BUILD_TIMEOUT: Duration = Duration::from_secs(900);
const OUTPUT_CAP: u64 = 1 << 20;
const PIPE_GRACE: Duration = Duration::from_secs(2);
const GROUP_GRACE: Duration = Duration::from_secs(2);
const BUILD_LOG_CAP: u64 = 4 << 20;
const BINARY: &str = "bemenu-sophia";
const MANIFEST: &str = "bemenu-artifact.manifest";
const COMMIT_OBJECT: &str = "source.commit";

struct RemoveOnDrop(PathBuf);
impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn run(repo: &Path, args: &[String]) -> Result<Vec<String>, String> {
    let [source, commit, output] = args else {
        return Err(USAGE.into());
    };
    if !hex(commit, 40) {
        return Err(format!(
            "signed commit must be 40 lowercase hex: {commit:?}"
        ));
    }
    let source = std::fs::canonicalize(source).map_err(|e| format!("{source}: {e}"))?;
    let output = absolute(Path::new(output))?;
    if output.symlink_metadata().is_ok() {
        return Err(format!("output already exists: {}", output.display()));
    }
    let parent = output.parent().ok_or("output has no parent directory")?;
    if !parent.is_dir() {
        return Err(format!(
            "output parent is not a directory: {}",
            parent.display()
        ));
    }

    // Identity and signer authorization, read-only against the source repo.
    let resolved = text(git(
        &source,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{commit}^{{commit}}"),
        ],
    )?)?;
    if resolved.trim() != commit {
        return Err(format!("{commit} does not name that exact commit"));
    }
    git(&source, &["verify-commit", commit])
        .map_err(|e| format!("signature authorization failed for {commit}: {e}"))?;
    let status = text(git(&source, &["log", "-1", "--format=%G?%n%GF", commit])?)?;
    let mut status = status.lines();
    let (signature, signer) = (status.next().unwrap_or(""), status.next().unwrap_or(""));
    if signature != "G" || signer.is_empty() || !signer.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "commit {commit} is not a good signature: status={signature:?} signer={signer:?}"
        ));
    }
    let raw = git(&source, &["cat-file", "commit", commit])?;
    let tree = commit_tree(&raw)?;
    if !raw
        .split(|b| *b == b'\n')
        .any(|line| line.starts_with(b"gpgsig "))
    {
        return Err("signed commit object has no gpgsig header".into());
    }

    // Isolated build input: exactly the signed tree, nothing from the checkout.
    let scratch = std::env::temp_dir().join(format!(
        "sophia-bemenu-artifact-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    std::fs::create_dir(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    let _scratch = RemoveOnDrop(scratch.clone());
    set_mode(&scratch, 0o700)?;
    let archive = scratch.join("source.tar");
    let tree_dir = scratch.join("source");
    std::fs::create_dir(&tree_dir).map_err(|e| e.to_string())?;
    git(
        &source,
        &["archive", "--format=tar", "-o", path_str(&archive)?, commit],
    )?;
    bounded(
        Command::new("tar")
            .arg("-x")
            .arg("--no-same-owner")
            .arg("-f")
            .arg(&archive)
            .arg("-C")
            .arg(&tree_dir),
        GIT_TIMEOUT,
        "tar -x",
    )?;
    std::fs::remove_file(&archive).map_err(|e| e.to_string())?;
    let inventory = crate::git_tree::inventory(&tree_dir)?;
    crate::git_tree::verify_commit(&raw, commit, &inventory.tree)
        .map_err(|e| format!("archived Bemenu tree is not the signed commit {commit}: {e}"))?;

    // SDK pin: the Bemenu snapshot is the same audited SDK revision Sophia pins.
    let bemenu_sdk = tree_dir.join("vendor/sophia-desktop-sdk");
    let sdk = crate::c_desktop_sdk::verify(&bemenu_sdk, repo)
        .map_err(|e| format!("Bemenu SDK snapshot: {e}"))?;
    let own = crate::c_desktop_sdk::verify(&repo.join("vendor/c-desktop-sdk"), repo)
        .map_err(|e| format!("Sophia SDK snapshot: {e}"))?;
    if sdk != own {
        return Err(format!("SDK pin differs: Bemenu {sdk}, Sophia {own}"));
    }
    let sdk_manifest = read(&bemenu_sdk.join("manifest.json"))?;
    if sdk_manifest != read(&repo.join("vendor/c-desktop-sdk/manifest.json"))? {
        return Err("SDK manifests differ between Bemenu and Sophia".into());
    }

    // Low-priority, two-job build inside the scratch tree only.
    let log = scratch.join("build.log");
    let log_file = File::create(&log).map_err(|e| e.to_string())?;
    // CFLAGS/CPPFLAGS/LDFLAGS from the caller would replace the recipe's
    // `?=` warning set, so they are removed; EXTRA_WARNINGS makes it fatal.
    let child = Command::new("nice")
        .args(["-n", "19", "make", "-j2", "EXTRA_WARNINGS=-Werror"])
        .arg(format!("GIT_SHA1={commit}"))
        .arg(BINARY)
        .current_dir(&tree_dir)
        .process_group(0)
        .env_remove("MAKEFLAGS")
        .env_remove("MFLAGS")
        .env_remove("MAKELEVEL")
        .env_remove("CFLAGS")
        .env_remove("CPPFLAGS")
        .env_remove("LDFLAGS")
        .env_remove("EXTRA_WARNINGS")
        .env("GIT_DIR", scratch.join("no-git"))
        .stdin(Stdio::null())
        .stdout(log_file.try_clone().map_err(|e| e.to_string())?)
        .stderr(log_file)
        .spawn()
        .map_err(|e| format!("make: {e}"))?;
    let built = wait_logged(child, &log, BUILD_TIMEOUT);
    if let Err(error) = built {
        return Err(format!("{error}\n{}", tail(&log)));
    }
    let binary = tree_dir.join(BINARY);
    if !std::fs::symlink_metadata(&binary).is_ok_and(|m| m.is_file()) {
        return Err(format!("build produced no regular {BINARY}"));
    }

    // Immutable output: created last, removed again if any step fails.
    std::fs::create_dir(&output).map_err(|e| format!("{}: {e}", output.display()))?;
    let written = write_output(&output, &binary, &raw, |binary_sha256| {
        [
            "schema=1".to_owned(),
            format!("binary={BINARY}"),
            format!("binary_sha256={binary_sha256}"),
            format!("source_commit={commit}"),
            format!("source_tree={tree}"),
            "signature_status=G".to_owned(),
            format!("signer_fingerprint={signer}"),
            format!("sdk_revision={sdk}"),
            format!("sdk_manifest_sha256={:x}", Sha256::digest(&sdk_manifest)),
        ]
        .join("\n")
            + "\n"
    });
    match written {
        Ok(binary_sha256) => Ok(vec![format!(
            "bemenu_artifact status=prepared commit={commit} binary_sha256={binary_sha256} signer={signer} sdk={sdk} dir={}",
            output.display()
        )]),
        Err(error) => {
            let _ = set_mode(&output, 0o700);
            let _ = std::fs::remove_dir_all(&output);
            Err(error)
        }
    }
}

fn write_output(
    output: &Path,
    binary: &Path,
    raw: &[u8],
    manifest: impl FnOnce(&str) -> String,
) -> Result<String, String> {
    let copy = output.join(BINARY);
    std::fs::copy(binary, &copy).map_err(|e| format!("copy {BINARY}: {e}"))?;
    let digest = format!("{:x}", Sha256::digest(read(&copy)?));
    std::fs::write(output.join(COMMIT_OBJECT), raw).map_err(|e| e.to_string())?;
    std::fs::write(output.join(MANIFEST), manifest(&digest)).map_err(|e| e.to_string())?;
    set_mode(&copy, 0o555)?;
    set_mode(&output.join(COMMIT_OBJECT), 0o444)?;
    set_mode(&output.join(MANIFEST), 0o444)?;
    set_mode(output, 0o555)?;
    Ok(digest)
}

fn commit_tree(raw: &[u8]) -> Result<String, String> {
    let first = raw.split(|b| *b == b'\n').next().unwrap_or_default();
    let tree = std::str::from_utf8(first)
        .ok()
        .and_then(|line| line.strip_prefix("tree "))
        .filter(|tree| hex(tree, 40))
        .ok_or("commit object has no leading tree line")?;
    Ok(tree.to_owned())
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    bounded(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["-c", "core.fsmonitor=false"])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0"),
        GIT_TIMEOUT,
        &format!("git {}", args.first().copied().unwrap_or("")),
    )
}

/// Run in a private process group to completion within `limit`, capturing at
/// most OUTPUT_CAP per stream. Output collection is bounded too: a descendant
/// still holding a pipe after the direct child exits ends the run and its group.
pub(crate) fn bounded(
    command: &mut Command,
    limit: Duration,
    what: &str,
) -> Result<Vec<u8>, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{what}: {e}"))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = match pipe {
                Some(pipe) => pipe.take(OUTPUT_CAP + 1).read_to_end(&mut bytes),
                None => Ok(0),
            };
            let _ = sender.send(result.map(|_| bytes));
        });
        receiver
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let status = match wait(&mut child, limit) {
        Ok(status) => status,
        Err(error) => {
            stop_group(&mut child);
            return Err(format!("{what}: {error}"));
        }
    };
    let deadline = Instant::now() + PIPE_GRACE;
    let collect = |receiver: &mpsc::Receiver<std::io::Result<Vec<u8>>>| {
        receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
    };
    let (Ok(stdout), Ok(stderr)) = (collect(&stdout), collect(&stderr)) else {
        stop_group(&mut child);
        return Err(format!("{what}: a descendant kept its output open"));
    };
    let (Ok(stdout), Ok(stderr)) = (stdout, stderr) else {
        stop_group(&mut child);
        return Err(format!("{what}: could not read command output"));
    };
    if stdout.len() as u64 > OUTPUT_CAP || stderr.len() as u64 > OUTPUT_CAP {
        stop_group(&mut child);
        return Err(format!("{what}: output exceeds {OUTPUT_CAP} bytes"));
    }
    if !status.success() {
        stop_group(&mut child);
        return Err(format!(
            "{what}: {status}: {}",
            String::from_utf8_lossy(&stderr)
        ));
    }
    Ok(stdout)
}

fn wait(child: &mut Child, limit: Duration) -> Result<std::process::ExitStatus, String> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(format!("exceeded {limit:?}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// SIGTERM the child's private group, allow GROUP_GRACE, then SIGKILL the
/// group and reap the direct child. Only this group is signalled: a daemon
/// that left it (for example with setsid) is never searched for or touched.
fn stop_group(child: &mut Child) {
    let Some(group) = i32::try_from(child.id()).ok().and_then(Pid::from_raw) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    let _ = kill_process_group(group, Signal::TERM);
    let deadline = Instant::now() + GROUP_GRACE;
    loop {
        // Reap the leader so a zombie does not keep the group observable.
        let _ = child.try_wait();
        if test_kill_process_group(group).is_err() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = kill_process_group(group, Signal::KILL);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.wait();
}

fn wait_logged(mut child: Child, log: &Path, limit: Duration) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            stop_group(&mut child);
            return Err(format!("make {BINARY}: {status}"));
        }
        let size = std::fs::metadata(log).map(|m| m.len()).unwrap_or(0);
        if Instant::now() >= deadline || size > BUILD_LOG_CAP {
            stop_group(&mut child);
            return Err(format!(
                "make {BINARY} stopped (limit {limit:?}, log {size} bytes)"
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn tail(log: &Path) -> String {
    let bytes = std::fs::read(log).unwrap_or_default();
    String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(4096)..]).into_owned()
}

fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path))
    }
}

fn path_str(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("non-UTF-8 path {}", path.display()))
}

fn text(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

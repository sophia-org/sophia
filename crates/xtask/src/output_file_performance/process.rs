//! Private namespace and bounded execution for build and measurement phases.
//! Children run at the caller's priority; builds use the caller's
//! `CARGO_BUILD_JOBS` or every available CPU, and both are recorded.
use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Sandbox {
    pub repo: PathBuf,
    pub output: PathBuf,
    pub toolchain: PathBuf,
    pub registry: PathBuf,
    /// Cargo and make parallelism inside the namespace.
    pub jobs: usize,
}

impl Sandbox {
    pub fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
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
                "usr/bin",
                "/bin",
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
            ])
            .arg("--ro-bind")
            .arg(&self.repo)
            .arg(&self.repo)
            .arg("--ro-bind")
            .arg(&self.toolchain)
            .arg(&self.toolchain)
            .arg("--bind")
            .arg(&self.output)
            .arg(&self.output)
            .arg("--ro-bind")
            .arg(&self.registry)
            .arg(self.output.join("cargo-home/registry"))
            .args(["--setenv", "PATH"])
            .arg(format!(
                "{}:/usr/bin:/bin",
                self.toolchain.join("bin").display()
            ))
            .args([
                "--setenv",
                "HOME",
                "/tmp",
                "--setenv",
                "TMPDIR",
                "/tmp",
                "--setenv",
                "LC_ALL",
                "C",
                "--setenv",
                "CARGO_NET_OFFLINE",
                "true",
                "--setenv",
                "RUST_TEST_THREADS",
                "1",
            ])
            .args(["--setenv", "CARGO_BUILD_JOBS"])
            .arg(self.jobs.to_string())
            .args(["--setenv", "CARGO_HOME"])
            .arg(self.output.join("cargo-home"))
            .args(["--setenv", "CARGO_TARGET_DIR"])
            .arg(self.output.join("target"))
            .arg("--chdir")
            .arg(&self.repo)
            .arg(program);
        command
    }
}

/// Build parallelism: an explicit `CARGO_BUILD_JOBS` must be a canonical
/// positive integer; without one, every available CPU.
pub fn jobs(explicit: Option<&std::ffi::OsStr>, available: usize) -> Result<usize, String> {
    let Some(value) = explicit else {
        return Ok(available.max(1));
    };
    value
        .to_str()
        .filter(|v| !v.starts_with('0') && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("CARGO_BUILD_JOBS must be a positive integer, not {value:?}"))
}

/// This process's nice value, which every sandboxed child inherits.
pub fn niceness() -> Result<i32, String> {
    let stat =
        fs::read_to_string("/proc/self/stat").map_err(|e| format!("/proc/self/stat: {e}"))?;
    nice_field(&stat).ok_or_else(|| format!("unreadable /proc/self/stat: {stat:?}"))
}

/// Field 19 of proc_pid_stat(5). The command name may contain spaces or
/// parentheses, so fields are counted from the last `)`, which ends field 2.
pub fn nice_field(stat: &str) -> Option<i32> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_ascii_whitespace().nth(16)?.parse().ok()
}

struct OwnedChild(Child, bool);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.1 {
            // Signal only while the direct child remains unreaped. bwrap owns
            // a PID namespace, so its termination also removes descendants.
            if let Some(pid) = rustix::process::Pid::from_raw(self.0.id() as i32) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub fn run(
    command: &mut Command,
    output: &Path,
    name: &str,
    timeout: Duration,
) -> Result<(), String> {
    let stdout_path = output.join(format!("{name}.stdout"));
    let stderr_path = output.join(format!("{name}.stderr"));
    let stdout = File::create(&stdout_path).map_err(|e| e.to_string())?;
    let stderr = File::create(&stderr_path).map_err(|e| e.to_string())?;
    command
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .process_group(0);
    let mut child = OwnedChild(
        command.spawn().map_err(|e| format!("start {name}: {e}"))?,
        false,
    );
    let deadline = Instant::now() + timeout;
    loop {
        for path in [&stdout_path, &stderr_path] {
            if fs::metadata(path).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
                return Err(format!("{name} log limit exceeded"));
            }
        }
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            child.1 = true;
            return if status.success() {
                Ok(())
            } else {
                Err(format!("{name} exited {status}; see {}", output.display()))
            };
        }
        if Instant::now() >= deadline {
            return Err(format!("{name} deadline expired"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

//! Private namespace and bounded execution for build and measurement phases.
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
                "CARGO_BUILD_JOBS",
                "1",
                "--setenv",
                "CARGO_NET_OFFLINE",
                "true",
                "--setenv",
                "RUST_TEST_THREADS",
                "1",
            ])
            .args(["--setenv", "CARGO_HOME"])
            .arg(self.output.join("cargo-home"))
            .args(["--setenv", "CARGO_TARGET_DIR"])
            .arg(self.output.join("target"))
            .arg("--chdir")
            .arg(&self.repo)
            .args(["/usr/bin/nice", "-n", "19"])
            .arg(program);
        command
    }
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

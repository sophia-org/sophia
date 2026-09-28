//! Bounded subprocesses shared by generic WM and shell SDK fixtures. Observe with WNOWAIT and
//! kill the private group before reaping its leader. The compiler and fixture
//! are trusted not to leave their process group; no arbitrary client is run.
#![cfg(test)]
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions};
use std::fs::{self, File};
use std::os::unix::{fs::DirBuilderExt, process::CommandExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
const OUTPUT_CAP: u64 = 4 * 1024 * 1024;

pub(super) struct Scratch(pub(super) PathBuf);
impl Scratch {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-c-wm-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) struct Process {
    child: Option<Child>,
    stdout: PathBuf,
    stderr: PathBuf,
}
impl Process {
    pub(super) fn spawn(command: &mut Command, root: &Path, label: &str) -> Self {
        let stdout = root.join(format!("{label}.stdout"));
        let stderr = root.join(format!("{label}.stderr"));
        let child = command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root)
            .env("TMPDIR", root)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::from(File::create(&stdout).unwrap()))
            .stderr(Stdio::from(File::create(&stderr).unwrap()))
            .process_group(0)
            .spawn()
            .unwrap();
        Self {
            child: Some(child),
            stdout,
            stderr,
        }
    }
    pub(super) fn check_output(&self) {
        for path in [&self.stdout, &self.stderr] {
            assert!(
                fs::metadata(path).unwrap().len() <= OUTPUT_CAP,
                "C WM fixture output overflow: {}",
                path.display()
            );
        }
    }
    pub(super) fn exited(&self) -> bool {
        self.check_output();
        let child = self.child.as_ref().unwrap();
        let pid = Pid::from_raw(child.id() as i32).unwrap();
        rustix::process::waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )
        .unwrap()
        .is_some()
    }
    pub(super) fn diagnostic(&self) -> String {
        self.check_output();
        fs::read_to_string(&self.stderr).unwrap()
    }
    fn stop(&mut self) -> Option<std::process::ExitStatus> {
        self.child.take().map(|mut child| {
            let pid = Pid::from_raw(child.id() as i32).unwrap();
            // The unreaped leader pins this group number through the last
            // signal, including normal exit with a surviving compiler child.
            let _ = rustix::process::kill_process_group(pid, Signal::KILL);
            child.wait().unwrap()
        })
    }
    pub(super) fn finish(mut self, timeout: Duration) -> String {
        let until = Instant::now() + timeout;
        while !self.exited() {
            assert!(
                Instant::now() < until,
                "C WM fixture timeout: {}",
                self.diagnostic()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let status = self.stop().unwrap();
        self.check_output();
        assert!(
            status.success(),
            "C WM fixture failed: {status}: {}",
            self.diagnostic()
        );
        assert_eq!(self.diagnostic(), "", "strict compiler/peer diagnostics");
        fs::read_to_string(&self.stdout).unwrap()
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

pub(super) fn compile(root: &Path, modules: &[&str], source: &str) -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let sdk = repo.join("vendor/c-desktop-sdk/source/src");
    let binary = root.join("sdk-peer");
    let mut command = Command::new("/usr/bin/nice");
    command
        .args([
            "-n",
            "19",
            "/usr/bin/cc",
            "-std=c99",
            "-O2",
            "-g",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-UNDEBUG",
            "-I",
        ])
        .arg(&sdk);
    for dir in modules {
        let mut files = fs::read_dir(sdk.join(dir))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "c"))
            .collect::<Vec<_>>();
        assert!(!files.is_empty(), "missing SDK module {dir}");
        files.sort();
        command.args(files);
    }
    command
        .arg(
            repo.join("crates/sophia-session/tests/support")
                .join(source),
        )
        .arg("-o")
        .arg(&binary);
    assert_eq!(
        Process::spawn(&mut command, root, "compile").finish(Duration::from_secs(90)),
        ""
    );
    binary
}

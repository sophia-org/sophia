//! Typed orchestration over the established namespace/descriptor launcher.
//! The small compatibility adapter keeps its existing kernel validation; all
//! host selection, waits and evidence decisions stay with the Rust caller.
use std::fs::File;
use std::os::fd::{AsRawFd, BorrowedFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

// A nested namespace launcher can leave its short-lived namespace monitor as
// an adopted child. One launch owns this private process's descendant scope;
// concurrent unrelated process owners are deliberately not supported here.
static LAUNCH_OWNER: Mutex<()> = Mutex::new(());

pub struct Mount {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub writable: bool,
}

pub struct Launch {
    pub source: PathBuf,
    pub directory: PathBuf,
    pub command: Vec<String>,
    pub mounts: Vec<Mount>,
    pub timeout: Duration,
}

impl Launch {
    /// The caller keeps every delegated descriptor alive through spawn. No
    /// unrelated descriptor is added to the launcher's pass-fd inventory.
    pub fn spawn(self, delegated: &[BorrowedFd<'_>]) -> Result<Child, String> {
        let owner = LAUNCH_OWNER
            .try_lock()
            .map_err(|_| "private launch already owned")?;
        if !children()?.is_empty() {
            return Err("private launch requires an empty child inventory".into());
        }
        rustix::process::set_child_subreaper(Some(rustix::process::getpid()))
            .map_err(|e| e.to_string())?;
        if self.timeout.is_zero()
            || self.timeout > Duration::from_secs(1800)
            || self
                .command
                .first()
                .is_none_or(|name| !Path::new(name).is_absolute())
        {
            return Err(
                "isolated launch requires an absolute executable and bounded timeout".into(),
            );
        }
        std::fs::create_dir(&self.directory).map_err(|e| e.to_string())?;
        let plan = self.directory.join("launch.json");
        let log = self.directory.join("child.log");
        let bwrap = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|directory| directory.join("bwrap"))
            .find(|path| path.is_file())
            .ok_or("bubblewrap unavailable; no ambient fallback")?;
        let mounts = self
            .mounts
            .iter()
            .map(|mount| {
                serde_json::json!({
                    "source":mount.source,"destination":mount.destination,"writable":mount.writable
                })
            })
            .collect::<Vec<_>>();
        let record = serde_json::json!({
            "command":self.command,"mounts":mounts,"timeout":self.timeout.as_secs_f64(),
            "delegated_fds":delegated.iter().map(AsRawFd::as_raw_fd).collect::<Vec<_>>(),
            "bwrap":bwrap,"isolation_directory":self.source.join("tools/probes/x11_conformance")
        });
        std::fs::write(
            &plan,
            serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let output = File::create(&log).map_err(|e| e.to_string())?;
        let mut command = Command::new("/usr/bin/python3");
        command
            .arg("-B")
            .arg(
                self.source
                    .join("tools/probes/m3_acceptance/containment.py"),
            )
            .arg(plan)
            .env_clear()
            .envs(super::ENVIRONMENT)
            .stdin(Stdio::null())
            .stdout(output.try_clone().map_err(|e| e.to_string())?)
            .stderr(output)
            .process_group(0);
        let mut inherited = Inherited::default();
        for fd in delegated {
            if fd.as_raw_fd() < 3 {
                return Err("delegated descriptors must be above stderr".into());
            }
            let flags = rustix::io::fcntl_getfd(fd).map_err(|e| e.to_string())?;
            inherited.0.push((*fd, flags));
            rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::empty()).map_err(|e| e.to_string())?;
        }
        let process = command.spawn().map_err(|e| e.to_string())?;
        drop(inherited);
        Ok(Child {
            process,
            deadline: Instant::now() + self.timeout + Duration::from_secs(3),
            waited: false,
            root_reaped: false,
            log,
            _owner: owner,
        })
    }
}

#[derive(Default)]
struct Inherited<'a>(Vec<(BorrowedFd<'a>, rustix::io::FdFlags)>);

impl Drop for Inherited<'_> {
    fn drop(&mut self) {
        for (fd, flags) in &self.0 {
            let _ = rustix::io::fcntl_setfd(fd, *flags);
        }
    }
}

/// A launcher is always waited, including assertion/error unwinds. A timeout
/// is a failure even when killing and collecting its process group succeeds.
pub struct Child {
    process: std::process::Child,
    deadline: Instant,
    waited: bool,
    root_reaped: bool,
    log: PathBuf,
    _owner: MutexGuard<'static, ()>,
}

impl Child {
    pub fn log(&self) -> &Path {
        &self.log
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, String> {
        let result = self.process.try_wait().map_err(|e| e.to_string())?;
        self.root_reaped |= result.is_some();
        if result.is_some() && !self.waited {
            collect_monitors(false)?;
            self.waited = true;
        }
        Ok(result)
    }

    pub fn wait(&mut self) -> Result<ExitStatus, String> {
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= self.deadline {
                self.kill_and_wait();
                return Err("contained child deadline elapsed".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn kill_and_wait(&mut self) {
        if !self.root_reaped {
            if let Some(pid) = rustix::process::Pid::from_raw(self.process.id() as i32) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
            let _ = self.process.kill();
            self.root_reaped = self.process.wait().is_ok();
        }
        self.waited = self.root_reaped && collect_monitors(true).is_ok();
    }
}

fn children() -> Result<std::collections::BTreeSet<u32>, String> {
    let mut children = std::collections::BTreeSet::new();
    for task in std::fs::read_dir("/proc/self/task").map_err(|e| e.to_string())? {
        let task = task.map_err(|e| e.to_string())?;
        let listed =
            std::fs::read_to_string(task.path().join("children")).map_err(|e| e.to_string())?;
        for pid in listed.split_whitespace() {
            children.insert(pid.parse::<u32>().map_err(|e| e.to_string())?);
        }
    }
    Ok(children)
}

fn collect_monitors(kill: bool) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let current = children()?;
        if current.is_empty() {
            return Ok(());
        }
        for raw in current {
            let pid = rustix::process::Pid::from_raw(raw as i32).ok_or("invalid monitor pid")?;
            if kill {
                let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
            }
            rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::NOHANG)
                .map_err(|e| e.to_string())?;
        }
        if Instant::now() >= deadline {
            return Err("private namespace monitor did not finish; launch is not collected".into());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if !self.waited {
            self.kill_and_wait();
        }
    }
}

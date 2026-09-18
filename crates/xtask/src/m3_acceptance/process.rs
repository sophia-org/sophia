//! Bounded child ownership. An orphan is collected but never counted as success.
use super::types::{Collection, Execution};
use rustix::process::{Pid, Signal, WaitOptions};
use std::collections::BTreeSet;
use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub(super) fn arm_subreaper() -> Result<(), String> {
    rustix::process::set_child_subreaper(Some(rustix::process::getpid())).map_err(|e| e.to_string())
}

fn children() -> Result<BTreeSet<u32>, String> {
    let mut found = BTreeSet::new();
    for task in std::fs::read_dir("/proc/self/task").map_err(|e| e.to_string())? {
        let path = task.map_err(|e| e.to_string())?.path().join("children");
        if let Ok(text) = std::fs::read_to_string(path) {
            for pid in text.split_whitespace() {
                found.insert(pid.parse().map_err(|e| format!("invalid child pid: {e}"))?);
            }
        }
    }
    Ok(found)
}

fn descendants() -> Collection {
    let mut result = Collection {
        root_waited: true,
        descendants_found: 0,
        descendants_reaped: 0,
        remaining: Vec::new(),
        error: None,
    };
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let current = match children() {
            Ok(current) => current,
            Err(error) => {
                result.error = Some(error);
                break;
            }
        };
        if current.is_empty() {
            break;
        }
        seen.extend(&current);
        for raw in &current {
            if let Some(pid) = Pid::from_raw(*raw as i32) {
                let _ = rustix::process::kill_process(pid, Signal::KILL);
                if matches!(
                    rustix::process::waitpid(Some(pid), WaitOptions::NOHANG),
                    Ok(Some(_))
                ) {
                    result.descendants_reaped += 1;
                }
            }
        }
        if Instant::now() >= deadline {
            result.remaining = children().unwrap_or(current).into_iter().collect();
            result.error = Some("descendant collection deadline elapsed".into());
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    result.descendants_found = seen.len();
    result
}

struct OwnedChild {
    child: Child,
    waited: bool,
}

impl OwnedChild {
    fn kill(&mut self) {
        if let Some(pid) = Pid::from_raw(self.child.id() as i32) {
            let _ = rustix::process::kill_process_group(pid, Signal::KILL);
        }
        let _ = self.child.kill();
    }

    fn until(&mut self, deadline: Instant) -> Result<Option<std::process::ExitStatus>, String> {
        loop {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())? {
                self.waited = true;
                return Ok(Some(status));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.waited {
            self.kill();
            let _ = self.until(Instant::now() + Duration::from_secs(3));
            let _ = descendants();
        }
    }
}

pub(super) fn run(
    command: &mut Command,
    log: &Path,
    timeout: Duration,
) -> Result<Execution, String> {
    if timeout.is_zero() || timeout > Duration::from_secs(1800) {
        return Err("process timeout must be in (0, 1800] seconds".into());
    }
    if !children()?.is_empty() {
        return Err("uncollected children exist before launch".into());
    }
    let text = format!("{command:?}");
    let output = File::create(log).map_err(|e| e.to_string())?;
    command
        .stdin(Stdio::null())
        .stdout(output.try_clone().map_err(|e| e.to_string())?)
        .stderr(output)
        .process_group(0);
    let began = Instant::now();
    let mut owner = OwnedChild {
        child: command.spawn().map_err(|e| e.to_string())?,
        waited: false,
    };
    let mut status = owner.until(began + timeout)?;
    let timed_out = status.is_none();
    if timed_out {
        owner.kill();
        status = owner.until(Instant::now() + Duration::from_secs(3))?;
    }
    let mut collection = descendants();
    collection.root_waited = owner.waited;
    Ok(Execution {
        command: text,
        returncode: status.and_then(|status| status.code()),
        timed_out,
        elapsed_millis: began.elapsed().as_millis(),
        collection,
    })
}

pub(super) fn capture(command: &mut Command, log: &Path) -> Result<String, String> {
    let result = run(command, log, Duration::from_secs(120))?;
    if !result.clean() {
        return Err(format!(
            "command failed or was not collected: {}",
            result.command
        ));
    }
    std::fs::read_to_string(log)
        .map(|text| text.trim().to_owned())
        .map_err(|e| e.to_string())
}

pub(super) fn private_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/home/test")
        .env("LANG", "C.UTF-8")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("PWD", "/work");
    command
}

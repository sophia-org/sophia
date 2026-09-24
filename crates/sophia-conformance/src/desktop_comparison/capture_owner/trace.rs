//! Narrow privileged adapter for kernel DRM completion timing.

use super::{READY_TIMEOUT, write_new};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn probe_tracefs(helper: &Path) -> Result<PathBuf, String> {
    let output = Command::new("sudo")
        .arg("--")
        .arg(helper)
        .arg("--probe")
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("could not run privileged tracefs probe: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "privileged tracefs probe failed: {}",
            output.status
        ));
    }
    parse_tracefs_probe(&String::from_utf8_lossy(&output.stdout))
}

fn parse_tracefs_probe(source: &str) -> Result<PathBuf, String> {
    const PREFIX: &str = "desktop_comparison_tracefs_probe schema=1 status=ready tracefs=";
    const SUFFIX: &str = " event=drm_vblank_event_delivered";
    let record = source.trim();
    let root = record
        .strip_prefix(PREFIX)
        .and_then(|value| value.strip_suffix(SUFFIX))
        .ok_or("privileged tracefs probe returned an invalid record")?;
    match root {
        "/sys/kernel/tracing" | "/sys/kernel/debug/tracing" => Ok(PathBuf::from(root)),
        _ => Err("privileged tracefs probe returned an unexpected root".to_owned()),
    }
}

pub(super) struct TraceOwner {
    child: Child,
    ready: PathBuf,
    start: PathBuf,
    stop: PathBuf,
    finished: bool,
}

impl TraceOwner {
    pub(super) fn start(helper: &Path, attempt: &Path, output: &Path) -> Result<Self, String> {
        let ready = attempt.join("trace.ready");
        let start = attempt.join("trace.start");
        let stop = attempt.join("trace.stop");
        let child = Command::new("sudo")
            .arg("--")
            .arg(helper)
            .arg(output)
            .arg(&ready)
            .arg(&start)
            .arg(&stop)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| {
                format!("could not start privileged kernel timing adapter: {error}")
            })?;
        let mut owner = Self {
            child,
            ready,
            start,
            stop,
            finished: false,
        };
        owner.wait_ready()?;
        Ok(owner)
    }

    fn wait_ready(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.ready.is_file() {
                return Ok(());
            }
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|error| format!("could not poll kernel timing adapter: {error}"))?
            {
                return Err(format!(
                    "kernel timing adapter exited before ready: {status}"
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err("kernel timing adapter did not become ready within 30 seconds".to_owned())
    }

    pub(super) fn begin(&self) -> Result<(), String> {
        write_new(&self.start, b"start\n")
    }

    pub(super) fn finish(&mut self) -> Result<(), String> {
        write_new(&self.stop, b"stop\n")?;
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|error| format!("could not poll kernel timing adapter: {error}"))?
            {
                self.finished = true;
                if !status.success() {
                    return Err(format!("kernel timing adapter failed: {status}"));
                }
                for marker in [&self.ready, &self.start, &self.stop] {
                    let _ = fs::remove_file(marker);
                }
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err("kernel timing adapter did not stop within 30 seconds".to_owned())
    }
}

impl Drop for TraceOwner {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = fs::write(&self.stop, b"stop\n");
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                self.finished = true;
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

mod tests;

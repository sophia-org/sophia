//! Running the PAM helper for one attempt: execute it with a clean
//! environment, hand it the request, and wait for one reply under a
//! deadline. A helper that overruns, exits early or answers malformed bytes
//! never yields an accepted verdict. The reply and the helper's exit are both
//! awaited under the one deadline, so no helper can hold a worker longer.

use crate::pam_wire::{FLAG_DISALLOW_NULL, FLAG_SETCRED, HelperReply, HelperRequest, REPLY_LEN};
use crate::proto::{PamRequest, PamVerdict};
use std::io::Write;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long a wait for the helper sleeps between checks of cancellation.
const WAIT_SLICE: Duration = Duration::from_millis(50);

#[derive(Clone, Debug)]
pub struct PamHelper {
    /// The helper binary, by the canonical path the agent checked at
    /// start-up: a helper the user could replace is refused.
    pub path: PathBuf,
    /// A private PAM configuration directory; set only by tests.
    pub confdir: Option<PathBuf>,
    /// The longest an attempt may take, PAM's failure delay included.
    pub deadline: Duration,
}

impl PamHelper {
    /// Runs one attempt. `cancelled` is set when the conversation that asked
    /// goes away; the helper is then killed and the verdict discarded.
    pub fn verify(&self, request: &PamRequest, cancelled: &AtomicBool) -> PamVerdict {
        let frame = match (HelperRequest {
            flags: FLAG_DISALLOW_NULL | FLAG_SETCRED,
            service: request.service.clone(),
            user: request.user.clone(),
            secret: request.secret.clone(),
        })
        .encode()
        {
            Ok(frame) => frame,
            Err(_) => return PamVerdict::HelperFailed,
        };
        let mut command = Command::new(&self.path);
        if let Some(confdir) = &self.confdir {
            command.arg("--confdir").arg(confdir);
        }
        let Ok(mut child) = command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        else {
            return PamVerdict::HelperFailed;
        };
        let written = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(frame.as_slice()).is_ok());
        drop(frame);
        if !written {
            return finish(child, PamVerdict::HelperFailed);
        }
        let started = Instant::now();
        let reply = match self.await_reply(&child, started, cancelled) {
            Ok(reply) => reply,
            Err(verdict) => return finish(child, verdict),
        };
        match self.await_exit(&child, started, cancelled) {
            Ok(()) => {}
            Err(verdict) => return finish(child, verdict),
        }
        // The helper has exited, so this wait returns at once.
        let exited = child.wait().is_ok_and(|status| status.success());
        if exited {
            reply.verdict
        } else {
            PamVerdict::HelperFailed
        }
    }

    /// Reads the reply as it arrives, however the helper splits it.
    fn await_reply(
        &self,
        child: &Child,
        started: Instant,
        cancelled: &AtomicBool,
    ) -> Result<HelperReply, PamVerdict> {
        let Some(stdout) = child.stdout.as_ref() else {
            return Err(PamVerdict::HelperFailed);
        };
        rustix::io::ioctl_fionbio(stdout, true).map_err(|_| PamVerdict::HelperFailed)?;
        let mut reply = [0; REPLY_LEN];
        let mut filled = 0;
        while filled < REPLY_LEN {
            self.await_readable(stdout.as_fd(), started, cancelled)?;
            match rustix::io::read(stdout, &mut reply[filled..]) {
                // The helper closed its end before a whole reply.
                Ok(0) => return Err(PamVerdict::HelperFailed),
                Ok(count) => filled += count,
                Err(rustix::io::Errno::INTR | rustix::io::Errno::AGAIN) => {}
                Err(_) => return Err(PamVerdict::HelperFailed),
            }
        }
        HelperReply::decode(&reply).map_err(|_| PamVerdict::HelperFailed)
    }

    /// Waits for the helper to exit, through a pidfd so the wait can be
    /// bounded and cancelled. The helper is not reaped here.
    fn await_exit(
        &self,
        child: &Child,
        started: Instant,
        cancelled: &AtomicBool,
    ) -> Result<(), PamVerdict> {
        let pid = i32::try_from(child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .ok_or(PamVerdict::HelperFailed)?;
        let pidfd = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty())
            .map_err(|_| PamVerdict::HelperFailed)?;
        self.await_readable(pidfd.as_fd(), started, cancelled)
    }

    /// Waits until `fd` is readable (or hung up), checking cancellation every
    /// slice, until the attempt's deadline.
    fn await_readable(
        &self,
        fd: std::os::fd::BorrowedFd<'_>,
        started: Instant,
        cancelled: &AtomicBool,
    ) -> Result<(), PamVerdict> {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(PamVerdict::HelperFailed);
            }
            let Some(left) = self.deadline.checked_sub(started.elapsed()) else {
                return Err(PamVerdict::TimedOut);
            };
            let slice = left.min(WAIT_SLICE);
            let timeout = rustix::event::Timespec {
                tv_sec: i64::try_from(slice.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(slice.subsec_nanos()),
            };
            let mut fds = [rustix::event::PollFd::new(
                &fd,
                rustix::event::PollFlags::IN,
            )];
            match rustix::event::poll(&mut fds, Some(&timeout)) {
                Ok(0) | Err(rustix::io::Errno::INTR) => continue,
                Ok(_) => return Ok(()),
                Err(_) => return Err(PamVerdict::HelperFailed),
            }
        }
    }
}

/// Kills and reaps a helper that will give no verdict.
fn finish(mut child: Child, verdict: PamVerdict) -> PamVerdict {
    let _ = child.kill();
    let _ = child.wait();
    verdict
}

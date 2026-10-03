//! Running the PAM helper for one attempt: execute it with a clean
//! environment, hand it the request, and wait for one reply under a
//! deadline. A helper that overruns, exits early or answers malformed bytes
//! never yields an accepted verdict.

use crate::pam_wire::{FLAG_DISALLOW_NULL, FLAG_SETCRED, HelperReply, HelperRequest, REPLY_LEN};
use crate::proto::{PamRequest, PamVerdict};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long a wait for the reply sleeps between checks of cancellation.
const WAIT_SLICE: Duration = Duration::from_millis(50);

#[derive(Clone, Debug)]
pub struct PamHelper {
    /// The helper binary. The agent refuses at start-up a helper the user
    /// could replace.
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
        match self.await_reply(&mut child, cancelled) {
            Ok(reply) => {
                let exited = child.wait().is_ok_and(|status| status.success());
                if exited {
                    reply.verdict
                } else {
                    PamVerdict::HelperFailed
                }
            }
            Err(verdict) => finish(child, verdict),
        }
    }

    fn await_reply(
        &self,
        child: &mut Child,
        cancelled: &AtomicBool,
    ) -> Result<HelperReply, PamVerdict> {
        let Some(stdout) = child.stdout.as_mut() else {
            return Err(PamVerdict::HelperFailed);
        };
        let started = Instant::now();
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
                &*stdout,
                rustix::event::PollFlags::IN,
            )];
            match rustix::event::poll(&mut fds, Some(&timeout)) {
                Ok(0) | Err(rustix::io::Errno::INTR) => continue,
                Ok(_) => break,
                Err(_) => return Err(PamVerdict::HelperFailed),
            }
        }
        // The reply is one small frame written at once; a helper that sends
        // part of it and stalls is still bounded by its exit or our kill.
        let mut reply = [0; REPLY_LEN];
        stdout
            .read_exact(&mut reply)
            .map_err(|_| PamVerdict::HelperFailed)?;
        HelperReply::decode(&reply).map_err(|_| PamVerdict::HelperFailed)
    }
}

/// Kills and reaps a helper that will give no verdict.
fn finish(mut child: Child, verdict: PamVerdict) -> PamVerdict {
    let _ = child.kill();
    let _ = child.wait();
    verdict
}

//! The session lock's authenticator: the `sophia-factotum` agent, started by
//! Session and spoken to over a private socket pair.
//!
//! Each attempt runs on a worker thread, because a PAM verdict can take
//! seconds (the failure delay alone is two). The owner loop hands the worker
//! an attempt and polls for its verdict; the worker rings the owner's wake
//! when one arrives.

use super::SessionFactotum;
use crate::session_lock::{SessionUnlockAttempt, SessionUnlockVerdict};
use crate::session_lock_input::{SessionUnlockAuthenticator, SessionUnlockUnavailable};
use sophia_factotum::client::{LoginVerdict, UnlockClient};
use sophia_factotum::secret::SecretBytes;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};
use std::time::{Duration, Instant};

/// Version negotiation and attach.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// One login, PAM's failure delay and the agent's own deadline included.
const LOGIN: Duration = Duration::from_secs(40);

type Verdict = (SessionUnlockAttempt, SessionUnlockVerdict);

pub(super) struct FactotumAuthenticator {
    agent: Child,
    attempts: SyncSender<(SessionUnlockAttempt, SecretBytes)>,
    verdicts: Receiver<Verdict>,
}

impl FactotumAuthenticator {
    fn start(factotum: &SessionFactotum, wake: sophia_wake::Notifier) -> std::io::Result<Self> {
        let (session_end, agent_end) = UnixStream::pair()?;
        // The agent's standard input is its end of the pair, so it adopts the
        // socket without any unsafe descriptor handling.
        let agent = Command::new(&factotum.agent)
            .arg("--owner")
            .arg(&factotum.user)
            .arg("--pam-helper")
            .arg(&factotum.pam_helper)
            .arg("--pam-service")
            .arg(&factotum.pam_service)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::from(OwnedFd::from(agent_end)))
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        let (attempts, pending) = sync_channel::<(SessionUnlockAttempt, SecretBytes)>(1);
        let (verdicts_sender, verdicts) = channel::<Verdict>();
        let user = factotum.user.clone();
        let service = factotum.pam_service.clone();
        std::thread::Builder::new()
            .name("sophia-unlock".into())
            .spawn(move || {
                let mut client = UnlockClient::over(session_end, HANDSHAKE).ok();
                while let Ok((attempt, secret)) = pending.recv() {
                    let verdict = match client.as_mut().map(|client| {
                        client.login(&service, &user, secret.as_slice(), Instant::now() + LOGIN)
                    }) {
                        Some(Ok(LoginVerdict::Accepted)) => SessionUnlockVerdict::Accepted,
                        Some(Ok(LoginVerdict::Refused(_))) => SessionUnlockVerdict::Rejected,
                        // A broken connection decides nothing, now or later.
                        Some(Err(_)) => {
                            client = None;
                            SessionUnlockVerdict::Unavailable
                        }
                        None => SessionUnlockVerdict::Unavailable,
                    };
                    drop(secret);
                    if verdicts_sender.send((attempt, verdict)).is_err() {
                        return;
                    }
                    wake.notify();
                }
            })?;
        Ok(Self {
            agent,
            attempts,
            verdicts,
        })
    }
}

impl SessionUnlockAuthenticator for FactotumAuthenticator {
    fn begin(
        &mut self,
        attempt: SessionUnlockAttempt,
        secret: &str,
    ) -> Result<(), SessionUnlockUnavailable> {
        self.attempts
            .try_send((attempt, SecretBytes::from_slice(secret.as_bytes())))
            .map_err(|_| SessionUnlockUnavailable)
    }

    fn poll(&mut self) -> Option<Verdict> {
        self.verdicts.try_recv().ok()
    }
}

impl Drop for FactotumAuthenticator {
    fn drop(&mut self) {
        let _ = self.agent.kill();
        let _ = self.agent.wait();
    }
}

/// The session's authenticator, if one is configured and starts. Without
/// one a lock is refused, so a failure here is reported and never fatal.
pub(super) fn start_session_authenticator(
    factotum: Option<&SessionFactotum>,
    wake: sophia_wake::Notifier,
) -> Option<Box<dyn SessionUnlockAuthenticator>> {
    match FactotumAuthenticator::start(factotum?, wake) {
        Ok(authenticator) => Some(Box::new(authenticator)),
        Err(error) => {
            crate::session_eprintln!(
                "sophia_live_session_lock schema=1 status=authenticator_unavailable error={error}"
            );
            None
        }
    }
}

//! The session lock's authenticator: the `sophia-factotum` agent, started by
//! Session and spoken to over a private socket pair, and replaced by the
//! supervisor whenever it fails.
//!
//! The supervisor's thread advances the shared client without waiting for a
//! PAM verdict. The owner loop hands it an attempt and polls for its verdict;
//! the thread rings the owner's wake when one arrives.

use super::SessionFactotum;
use crate::session_lock_input::SessionUnlockAuthenticator;
use crate::session_unlock_supervisor::{
    SupervisedAuthenticator, UnlockAgent, UnlockAgentLauncher, UnlockSupervision,
};
use sophia_factotum::client::{LoginVerdict, SessionClient};
use sophia_factotum::launch::queue::{self, ConnectionOwner, Ingress};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Version negotiation and attach.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// One login, PAM's failure delay and the agent's own deadline included.
const LOGIN: Duration = Duration::from_secs(40);

struct FactotumLauncher(SessionFactotum);

struct FactotumAgent {
    process: Child,
    owner: ConnectionOwner,
    ingress: Ingress,
    service: String,
    user: String,
    local_refusal: bool,
}

impl UnlockAgentLauncher for FactotumLauncher {
    type Agent = FactotumAgent;

    fn launch(&mut self) -> Result<FactotumAgent, String> {
        let factotum = &self.0;
        let (session_end, agent_end) = UnixStream::pair().map_err(|error| error.to_string())?;
        // The agent's standard input is its end of the pair, so it adopts the
        // socket without any unsafe descriptor handling.
        let mut process = Command::new(&factotum.agent)
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
            .spawn()
            .map_err(|error| error.to_string())?;
        // An agent that refuses its helper or its peer exits before it
        // answers; it is never reported available.
        match SessionClient::over(session_end, HANDSHAKE) {
            Ok(client) => {
                let (ingress, owner) = queue::channel();
                Ok(FactotumAgent {
                    process,
                    owner: owner.connect(client),
                    ingress,
                    service: factotum.pam_service.clone(),
                    user: factotum.user.clone(),
                    local_refusal: false,
                })
            }
            Err(error) => {
                let _ = process.kill();
                let _ = process.wait();
                Err(format!("handshake: {error:?}"))
            }
        }
    }
}

impl UnlockAgent for FactotumAgent {
    fn launch_ingress(&self) -> Option<Ingress> {
        Some(self.ingress.clone())
    }
    fn begin_login(&mut self, secret: &[u8]) -> Result<(), String> {
        let now = Instant::now();
        match self.owner.client_mut().begin_login(
            &self.service,
            &self.user,
            secret,
            now,
            now + LOGIN,
        ) {
            Ok(()) => Ok(()),
            // A local protocol-bound refusal is a rejected login, not an
            // unavailable agent. The input editor can hold more than PAM's
            // 512-byte response limit.
            Err(sophia_factotum::client::ClientError::InvalidInput) => {
                self.local_refusal = true;
                Ok(())
            }
            Err(error) => Err(format!("{error:?}")),
        }
    }

    fn poll_login(&mut self) -> Result<Option<bool>, String> {
        let now = Instant::now();
        self.owner.poll(now).map_err(|error| format!("{error:?}"))?;
        if std::mem::take(&mut self.local_refusal) {
            return Ok(Some(false));
        }
        self.owner
            .client_mut()
            .take_login(now)
            .map(|verdict| verdict.map(|v| v == LoginVerdict::Accepted))
            .map_err(|error| format!("{error:?}"))
    }

    fn gone(&mut self) -> bool {
        !matches!(self.process.try_wait(), Ok(None))
    }
}

impl Drop for FactotumAgent {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// The session's authenticator, if one is configured. Without one a lock is
/// refused, so a failure here is reported and never fatal.
pub(super) fn start_session_authenticator(
    factotum: Option<&SessionFactotum>,
    wake: sophia_wake::Notifier,
) -> Option<Box<dyn SessionUnlockAuthenticator>> {
    match SupervisedAuthenticator::start(
        FactotumLauncher(factotum?.clone()),
        UnlockSupervision::SESSION,
        move || wake.notify(),
    ) {
        Ok(authenticator) => Some(Box::new(authenticator)),
        Err(error) => {
            crate::session_eprintln!(
                "sophia_live_session_lock schema=1 status=authenticator_unavailable error={error}"
            );
            None
        }
    }
}

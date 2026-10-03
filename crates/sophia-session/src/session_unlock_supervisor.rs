//! Keeps the session lock's authenticator able to decide (t293).
//!
//! The agent is a separate process that can fail to start, refuse its
//! helper, die or stop answering. A lock must never outlive every way to
//! open it, so the supervisor runs the agent on its own thread: it starts it,
//! reports it available only once it has answered the handshake, replaces it
//! after any failure with a bounded backoff, and answers every attempt made
//! while no agent is up as undecided. Session refuses to lock while nothing
//! is available, and a lock already in force stays until an agent decides.

use crate::session_lock::{SessionUnlockAttempt, SessionUnlockVerdict};
use crate::session_lock_input::{SessionUnlockAuthenticator, SessionUnlockUnavailable};
use sophia_factotum::secret::SecretBytes;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, channel, sync_channel};
use std::time::{Duration, Instant};

type Verdict = (SessionUnlockAttempt, SessionUnlockVerdict);

/// One running agent that has answered its handshake.
pub trait UnlockAgent: Send {
    /// One login. `Ok(true)` accepts and `Ok(false)` refuses. An `Err`
    /// decides nothing and retires this agent.
    fn login(&mut self, secret: &[u8]) -> Result<bool, String>;

    /// Whether the agent has gone while idle.
    fn gone(&mut self) -> bool;
}

/// Starts agents. A launch returns only once the agent has answered its
/// handshake, so a launched agent can take an attempt at once.
pub trait UnlockAgentLauncher: Send + 'static {
    type Agent: UnlockAgent;

    fn launch(&mut self) -> Result<Self::Agent, String>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnlockSupervision {
    /// The wait before the first replacement; it doubles per failure in a
    /// row up to `longest_restart`, and resets once an agent decides.
    pub first_restart: Duration,
    pub longest_restart: Duration,
    /// How often an idle agent is checked for having gone.
    pub liveness: Duration,
}

impl UnlockSupervision {
    pub const SESSION: Self = Self {
        first_restart: Duration::from_secs(1),
        longest_restart: Duration::from_secs(60),
        liveness: Duration::from_secs(1),
    };
}

pub struct SupervisedAuthenticator {
    attempts: SyncSender<(SessionUnlockAttempt, SecretBytes)>,
    verdicts: Receiver<Verdict>,
    available: Arc<AtomicBool>,
}

impl SupervisedAuthenticator {
    /// Starts supervising on a thread of its own. `wake` rings the owner
    /// loop when a verdict is ready.
    pub fn start<L: UnlockAgentLauncher>(
        launcher: L,
        supervision: UnlockSupervision,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let (attempts, pending) = sync_channel(1);
        let (verdicts_sender, verdicts) = channel();
        let available = Arc::new(AtomicBool::new(false));
        let supervisor = Supervisor {
            launcher,
            supervision,
            pending,
            verdicts: verdicts_sender,
            available: Arc::clone(&available),
            wake: Box::new(wake),
        };
        std::thread::Builder::new()
            .name("sophia-unlock".into())
            .spawn(move || supervisor.run())?;
        Ok(Self {
            attempts,
            verdicts,
            available,
        })
    }
}

impl SessionUnlockAuthenticator for SupervisedAuthenticator {
    fn available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

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

struct Supervisor<L> {
    launcher: L,
    supervision: UnlockSupervision,
    pending: Receiver<(SessionUnlockAttempt, SecretBytes)>,
    verdicts: std::sync::mpsc::Sender<Verdict>,
    available: Arc<AtomicBool>,
    wake: Box<dyn Fn() + Send>,
}

/// Why serving stopped.
enum Stop {
    /// Session dropped the authenticator.
    Closed,
    /// The agent failed; replace it.
    Failed,
}

impl<L: UnlockAgentLauncher> Supervisor<L> {
    fn run(mut self) {
        let mut restart = self.supervision.first_restart;
        loop {
            match self.launcher.launch() {
                Ok(mut agent) => {
                    self.available.store(true, Ordering::Release);
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=authenticator_ready"
                    );
                    let stop = self.serve(&mut agent, &mut restart);
                    self.available.store(false, Ordering::Release);
                    // Dropping the agent ends its process.
                    drop(agent);
                    if let Stop::Closed = stop {
                        return;
                    }
                }
                Err(error) => {
                    crate::session_eprintln!(
                        "sophia_live_session_lock schema=1 status=authenticator_unavailable error={error}"
                    );
                }
            }
            if !self.refuse_until(Instant::now() + restart) {
                return;
            }
            restart = restart
                .saturating_mul(2)
                .min(self.supervision.longest_restart);
        }
    }

    fn serve(&mut self, agent: &mut L::Agent, restart: &mut Duration) -> Stop {
        loop {
            match self.pending.recv_timeout(self.supervision.liveness) {
                Ok((attempt, secret)) => {
                    let decided = agent.login(secret.as_slice());
                    drop(secret);
                    let verdict = match &decided {
                        Ok(true) => SessionUnlockVerdict::Accepted,
                        Ok(false) => SessionUnlockVerdict::Rejected,
                        Err(_) => SessionUnlockVerdict::Unavailable,
                    };
                    if !self.answer(attempt, verdict) {
                        return Stop::Closed;
                    }
                    match decided {
                        Ok(_) => *restart = self.supervision.first_restart,
                        Err(error) => {
                            // A broken connection decides nothing, now or
                            // later: this agent is replaced.
                            crate::session_eprintln!(
                                "sophia_live_session_lock schema=1 status=authenticator_failed error={error}"
                            );
                            return Stop::Failed;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if agent.gone() {
                        crate::session_eprintln!(
                            "sophia_live_session_lock schema=1 status=authenticator_failed error=agent_exited"
                        );
                        return Stop::Failed;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return Stop::Closed,
            }
        }
    }

    /// Answers every attempt as undecided until `deadline`. `false` once
    /// Session has dropped the authenticator.
    fn refuse_until(&mut self, deadline: Instant) -> bool {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match self.pending.recv_timeout(left) {
                Ok((attempt, secret)) => {
                    drop(secret);
                    if !self.answer(attempt, SessionUnlockVerdict::Unavailable) {
                        return false;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return true,
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
    }

    fn answer(&self, attempt: SessionUnlockAttempt, verdict: SessionUnlockVerdict) -> bool {
        if self.verdicts.send((attempt, verdict)).is_err() {
            return false;
        }
        (self.wake)();
        true
    }
}

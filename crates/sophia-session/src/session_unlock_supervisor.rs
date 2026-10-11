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
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::{Duration, Instant};

mod launch_service;
pub use launch_service::{LaunchService, LaunchServiceConnection};

type Verdict = (SessionUnlockAttempt, SessionUnlockVerdict);

/// One running agent that has answered its handshake.
pub trait UnlockAgent: Send {
    /// Trusted launch work shares this agent's transport and progress owner.
    /// Scripted or PAM-only agents need not expose a launch service.
    fn launch_ingress(&self) -> Option<sophia_factotum::launch::queue::Ingress> {
        None
    }
    /// Queue one login without waiting. An error decides nothing and retires
    /// this agent. The supervisor admits only one active login at a time.
    fn begin_login(&mut self, secret: &[u8]) -> Result<(), String>;

    /// Bounded nonblocking progress, also called while idle so other work on
    /// the same connection can progress. An error retires this agent.
    fn poll_login(&mut self) -> Result<Option<bool>, String>;

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
    launch_service: LaunchService,
    attempts: SyncSender<(SessionUnlockAttempt, SecretBytes)>,
    verdicts: Receiver<Verdict>,
    available: Arc<AtomicBool>,
    /// Includes queued, active and undelivered attempts. Slots are released
    /// only when Session consumes their correlated verdict.
    outstanding: usize,
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
        let (verdicts_sender, verdicts) = sync_channel(2);
        let available = Arc::new(AtomicBool::new(false));
        let launch_service = LaunchService::default();
        let supervisor = Supervisor {
            launcher,
            supervision,
            pending,
            verdicts: verdicts_sender,
            available: Arc::clone(&available),
            wake: Box::new(wake),
            launch_service: launch_service.clone(),
        };
        std::thread::Builder::new()
            .name("sophia-unlock".into())
            .spawn(move || supervisor.run())?;
        Ok(Self {
            launch_service,
            attempts,
            verdicts,
            available,
            outstanding: 0,
        })
    }

    /// Session keeps this separate from the PAM interface. A replacement
    /// publishes a fresh generation; old books and tickets remain closed.
    pub fn launch_service(&self) -> LaunchService {
        self.launch_service.clone()
    }
}

impl Drop for SupervisedAuthenticator {
    fn drop(&mut self) {
        self.launch_service.close();
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
        if self.outstanding >= 2
            || secret.len() > crate::session_lock_input::SESSION_LOCK_SECRET_CAPACITY
        {
            return Err(SessionUnlockUnavailable);
        }
        self.attempts
            .try_send((attempt, SecretBytes::from_slice(secret.as_bytes())))
            .map_err(|_| SessionUnlockUnavailable)?;
        self.outstanding += 1;
        Ok(())
    }

    fn poll(&mut self) -> Option<Verdict> {
        let verdict = self.verdicts.try_recv().ok()?;
        self.outstanding = self
            .outstanding
            .checked_sub(1)
            .expect("verdict owns an attempt slot");
        Some(verdict)
    }
}

struct Supervisor<L> {
    launch_service: LaunchService,
    launcher: L,
    supervision: UnlockSupervision,
    pending: Receiver<(SessionUnlockAttempt, SecretBytes)>,
    verdicts: SyncSender<Verdict>,
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
                    if !self.launch_service.publish(agent.launch_ingress()) {
                        return;
                    }
                    self.available.store(true, Ordering::Release);
                    crate::session_println!(
                        "sophia_live_session_lock schema=1 status=authenticator_ready"
                    );
                    let stop = self.serve(&mut agent, &mut restart);
                    self.launch_service.withdraw();
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
        let mut pending = None;
        // This is a supervision-thread readiness cadence, not an Engine timer
        // or a latency budget. It also bounds how long owner drop waits while
        // PAM has an unanswered read. Process liveness retains its own cadence.
        let turn = self
            .supervision
            .liveness
            .min(Duration::from_millis(20))
            .max(Duration::from_millis(1));
        let mut liveness = Instant::now() + self.supervision.liveness;
        loop {
            match agent.poll_login() {
                Ok(Some(accepted)) => {
                    let Some(attempt) = pending.take() else {
                        return Stop::Failed;
                    };
                    let verdict = if accepted {
                        SessionUnlockVerdict::Accepted
                    } else {
                        SessionUnlockVerdict::Rejected
                    };
                    if !self.answer(attempt, verdict) {
                        return Stop::Closed;
                    }
                    *restart = self.supervision.first_restart;
                }
                Ok(None) => {}
                Err(error) => {
                    if let Some(attempt) = pending.take()
                        && !self.answer(attempt, SessionUnlockVerdict::Unavailable)
                    {
                        return Stop::Closed;
                    }
                    crate::session_eprintln!(
                        "sophia_live_session_lock schema=1 status=authenticator_failed error={error}"
                    );
                    return Stop::Failed;
                }
            }
            if Instant::now() >= liveness {
                if agent.gone() {
                    if let Some(attempt) = pending.take()
                        && !self.answer(attempt, SessionUnlockVerdict::Unavailable)
                    {
                        return Stop::Closed;
                    }
                    crate::session_eprintln!(
                        "sophia_live_session_lock schema=1 status=authenticator_failed error=agent_exited"
                    );
                    return Stop::Failed;
                }
                liveness = Instant::now() + self.supervision.liveness;
            }
            match self.pending.recv_timeout(turn) {
                Ok((attempt, secret)) => {
                    if pending.is_some() {
                        drop(secret);
                        if !self.answer(attempt, SessionUnlockVerdict::Unavailable) {
                            return Stop::Closed;
                        }
                        continue;
                    }
                    let begun = agent.begin_login(secret.as_slice());
                    drop(secret);
                    if let Err(error) = begun {
                        if !self.answer(attempt, SessionUnlockVerdict::Unavailable) {
                            return Stop::Closed;
                        }
                        crate::session_eprintln!(
                            "sophia_live_session_lock schema=1 status=authenticator_failed error={error}"
                        );
                        return Stop::Failed;
                    }
                    pending = Some(attempt);
                }
                Err(RecvTimeoutError::Timeout) => {}
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
        // Every admitted attempt reserves a result slot through consumption.
        // Never block the shared connection owner on a slow Session consumer.
        if self.verdicts.try_send((attempt, verdict)).is_err() {
            return false;
        }
        (self.wake)();
        true
    }
}

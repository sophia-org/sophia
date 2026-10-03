//! Session's half of the session lock (t034): which lock is in force, which
//! authentication attempt may end it, and when input and pixels may return.
//!
//! This is a reducer over facts the owner loop gathers; it holds no secret
//! and runs no authentication. A lock is identified by its epoch and an
//! attempt by its epoch and serial, so a verdict can end only the lock and
//! the attempt it was issued for. Nothing here can unlock without such a
//! verdict, and no request from a client reaches it except to lock.

use core::num::NonZeroU64;
use sophia_engine::SessionLockEpoch;

/// One authentication attempt within one lock. Serials are minted once per
/// session and never reused, so an attempt of an earlier lock cannot alias
/// one of a later lock even before the epoch is compared.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct SessionUnlockAttempt {
    pub epoch: SessionLockEpoch,
    pub serial: NonZeroU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLockPhase {
    Unlocked,
    /// The cover is installed and input revoked; the session is not yet
    /// reported locked. That waits for every head to retire the cover and
    /// for the X frontend to apply the security epoch. Input already belongs
    /// to the lock, so an attempt may run: a head that never retires the
    /// cover cannot keep the user from unlocking.
    Locking {
        epoch: SessionLockEpoch,
        attempt: Option<SessionUnlockAttempt>,
    },
    /// Proven locked. At most one attempt is in flight.
    Locked {
        epoch: SessionLockEpoch,
        attempt: Option<SessionUnlockAttempt>,
    },
    /// The verdict accepted. The cover and the lock's hold on input stay
    /// until the X frontend applies the epoch that ends the lock, so nothing
    /// typed while locked reaches an application and nothing is shown before
    /// input can return.
    Unlocking {
        epoch: SessionLockEpoch,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLockStart {
    /// A new lock: install its cover and revoke input now. A lock requested
    /// while an attempt was in flight is a new lock too, so that attempt's
    /// verdict is void.
    Started(SessionLockEpoch),
    /// The session is already covered by this lock and nothing is being
    /// verified; nothing changes.
    AlreadyLocked(SessionLockEpoch),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLockError {
    /// Every epoch has been used. The request is refused rather than reuse
    /// one, since a reused epoch would let an old verdict end a new lock.
    EpochExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionUnlockVerdict {
    Accepted,
    Rejected,
    /// The authenticator could not decide: it failed, timed out or was
    /// replaced. The lock stays; the user may try again.
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionVerdictOutcome {
    /// The current attempt of the current lock was accepted: clear the
    /// cover and advance the input epoch.
    Unlocking(SessionLockEpoch),
    /// The current attempt failed; the lock stays and may be tried again.
    Failed(SessionUnlockAttempt, SessionUnlockVerdict),
    /// The verdict names a lock or an attempt that is no longer current.
    /// It is discarded and changes nothing.
    Stale,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionLockState {
    phase: SessionLockPhase,
    last_epoch: Option<SessionLockEpoch>,
    last_serial: u64,
}

impl Default for SessionLockState {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionLockState {
    pub const fn new() -> Self {
        Self {
            phase: SessionLockPhase::Unlocked,
            last_epoch: None,
            last_serial: 0,
        }
    }

    pub const fn phase(&self) -> SessionLockPhase {
        self.phase
    }

    /// Whether physical input belongs to the lock rather than to the
    /// desktop: in every phase but `Unlocked`.
    pub const fn holds_input(&self) -> bool {
        !matches!(self.phase, SessionLockPhase::Unlocked)
    }

    /// The lock whose cover Engine must draw, if any: until the unlock has
    /// been applied.
    pub const fn cover_epoch(&self) -> Option<SessionLockEpoch> {
        match self.phase {
            SessionLockPhase::Locking { epoch, .. }
            | SessionLockPhase::Locked { epoch, .. }
            | SessionLockPhase::Unlocking { epoch } => Some(epoch),
            SessionLockPhase::Unlocked => None,
        }
    }

    /// Locks the session. A lock in force with nothing being verified is
    /// kept as it is. Otherwise a new lock begins, whose new epoch voids
    /// anything issued for the old one: an unlock still waiting for its
    /// input epoch, or an attempt still being verified.
    pub fn lock(&mut self) -> Result<SessionLockStart, SessionLockError> {
        match self.phase {
            SessionLockPhase::Locking {
                epoch,
                attempt: None,
            }
            | SessionLockPhase::Locked {
                epoch,
                attempt: None,
            } => Ok(SessionLockStart::AlreadyLocked(epoch)),
            SessionLockPhase::Unlocked
            | SessionLockPhase::Locking { .. }
            | SessionLockPhase::Locked { .. }
            | SessionLockPhase::Unlocking { .. } => {
                let epoch = match self.last_epoch {
                    None => SessionLockEpoch::FIRST,
                    Some(last) => last.next().ok_or(SessionLockError::EpochExhausted)?,
                };
                self.last_epoch = Some(epoch);
                self.phase = SessionLockPhase::Locking {
                    epoch,
                    attempt: None,
                };
                Ok(SessionLockStart::Started(epoch))
            }
        }
    }

    /// Completes locking once every head has retired this lock's cover and
    /// the X frontend has applied the security epoch. `true` on the
    /// transition, which is when the lock may be reported.
    pub fn observe_covered(
        &mut self,
        presented: Option<SessionLockEpoch>,
        frontend_applied: bool,
    ) -> bool {
        match self.phase {
            SessionLockPhase::Locking { epoch, attempt }
                if presented == Some(epoch) && frontend_applied =>
            {
                self.phase = SessionLockPhase::Locked { epoch, attempt };
                true
            }
            _ => false,
        }
    }

    /// Opens an attempt for a submitted secret: while the lock holds input
    /// and no attempt is in flight. `None` otherwise, and the submission is
    /// not authenticated.
    pub fn begin_attempt(&mut self) -> Option<SessionUnlockAttempt> {
        let (SessionLockPhase::Locking {
            epoch,
            attempt: None,
        }
        | SessionLockPhase::Locked {
            epoch,
            attempt: None,
        }) = self.phase
        else {
            return None;
        };
        let serial = NonZeroU64::new(self.last_serial.checked_add(1)?)?;
        self.last_serial = serial.get();
        let attempt = SessionUnlockAttempt { epoch, serial };
        self.set_attempt(Some(attempt));
        Some(attempt)
    }

    fn set_attempt(&mut self, next: Option<SessionUnlockAttempt>) {
        if let SessionLockPhase::Locking { attempt, .. }
        | SessionLockPhase::Locked { attempt, .. } = &mut self.phase
        {
            *attempt = next;
        }
    }

    /// Settles a verdict. Only the current attempt of the current lock can
    /// move the state; anything else is `Stale` and changes nothing.
    pub fn settle(
        &mut self,
        attempt: SessionUnlockAttempt,
        verdict: SessionUnlockVerdict,
    ) -> SessionVerdictOutcome {
        match self.phase {
            SessionLockPhase::Locking {
                epoch,
                attempt: Some(current),
            }
            | SessionLockPhase::Locked {
                epoch,
                attempt: Some(current),
            } if current == attempt && attempt.epoch == epoch => match verdict {
                SessionUnlockVerdict::Accepted => {
                    self.phase = SessionLockPhase::Unlocking { epoch };
                    SessionVerdictOutcome::Unlocking(epoch)
                }
                SessionUnlockVerdict::Rejected | SessionUnlockVerdict::Unavailable => {
                    self.set_attempt(None);
                    SessionVerdictOutcome::Failed(attempt, verdict)
                }
            },
            _ => SessionVerdictOutcome::Stale,
        }
    }

    /// Completes the unlock once the X frontend has applied the epoch that
    /// ends it and the cover has been withdrawn. `true` on the transition,
    /// when input may return.
    pub fn observe_unlocked(&mut self, frontend_applied: bool) -> bool {
        match self.phase {
            SessionLockPhase::Unlocking { .. } if frontend_applied => {
                self.phase = SessionLockPhase::Unlocked;
                true
            }
            _ => false,
        }
    }
}

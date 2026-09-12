//! The private coordinator that owns a control-epoch transition.
//!
//! A bare compare-and-swap on a shared counter cannot express this. It makes
//! the new epoch visible before anything has been cleared, leaving a window in
//! which routing accepts work stamped with an epoch nothing has applied yet.
//! Every private epoch writer goes through here instead, so that window has an
//! owner and a name.
//!
//! Two numbers, deliberately spelled out rather than collapsed into one
//! "epoch". `requested_control_epoch` is what Session asked for.
//! `applied_control_epoch` is what has actually been cleared everywhere. They
//! differ for the whole length of a transition, and work is validated against
//! the applied one.

use std::fmt;

use std::sync::atomic::{AtomicU64, Ordering};

use sophia_input_authority::{AuthorityInstance, IssuerHandle, RegistrationError};

/// Distinguishes coordinators, so their tokens cannot be confused.
static COORDINATOR_INCARNATIONS: AtomicU64 = AtomicU64::new(1);

/// Why a coordinator refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlEpochRefusal {
    /// A transition is under way. The caller waits; it does not proceed on the
    /// last applied values, because those describe a world already revoked.
    TransitionPending,
    /// The work names an epoch that is not the applied one.
    EpochMismatch { stamped: u64, applied: u64 },
    /// The work names a publication that is not the committed one.
    PublicationMismatch { stamped: u64, committed: u64 },
    /// A requested epoch did not move forward.
    EpochWentBackwards { requested: u64, current: u64 },
    /// A security-control transition must advance the epoch; only a
    /// publication-only transition may keep it.
    EpochUnchanged { epoch: u64 },
    /// Nothing was requested, so there is nothing to apply or reopen.
    NoTransition,
    /// An installation report names a transition that is not the pending one.
    WrongTransition,
    /// Transition identities are exhausted; none may be reused.
    TransitionIdentitiesExhausted,
    /// Coordinator incarnations are exhausted; none may be reused.
    CoordinatorIncarnationsExhausted,
    /// The state this kind of transition requires was not installed.
    NotInstalled { kind: TransitionKind },
    /// The common authority refused the transition itself.
    Authority(RegistrationError),
}

impl fmt::Display for ControlEpochRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransitionPending => {
                write!(formatter, "a control-epoch transition has not published")
            }
            Self::EpochMismatch { stamped, applied } => write!(
                formatter,
                "work stamped with control epoch {stamped} against applied {applied}"
            ),
            Self::PublicationMismatch { stamped, committed } => write!(
                formatter,
                "work stamped with publication {stamped} against committed {committed}"
            ),
            Self::EpochWentBackwards { requested, current } => write!(
                formatter,
                "requested control epoch {requested} does not follow {current}"
            ),
            Self::EpochUnchanged { epoch } => write!(
                formatter,
                "a security-control transition cannot keep control epoch {epoch}"
            ),
            Self::NoTransition => write!(formatter, "no control transition is in flight"),
            Self::WrongTransition => write!(
                formatter,
                "the installation report belongs to another transition"
            ),
            Self::TransitionIdentitiesExhausted => {
                write!(formatter, "control transition identities are exhausted")
            }
            Self::CoordinatorIncarnationsExhausted => {
                write!(formatter, "control coordinator incarnations are exhausted")
            }
            Self::NotInstalled { kind } => {
                write!(
                    formatter,
                    "a {kind:?} transition has not installed its state"
                )
            }
            Self::Authority(error) => write!(formatter, "{error:?}"),
        }
    }
}

/// What kind of change a transition carries.
///
/// The distinction matters because a routine focus change must not revoke
/// grabs. Collapsing the two would make every publication gratuitously
/// destroy active grabs, and would let a publication reopen on proof that
/// belongs to a security change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    /// A new security control epoch. Active grabs, pointer state and frozen
    /// input are all revoked, and the new snapshot is installed.
    SecurityControl,
    /// A focus publication only. Grabs survive; the snapshot is installed.
    Publication,
}

/// What a transition has installed, as declared by the writer that did it.
///
/// These are declarations, not observations. The coordinator records what the
/// production writer reports after applying state under the ranked guards; it
/// cannot see those populations itself, and a test that sets these booleans
/// proves the coordinator's sequencing rather than that anything was cleared.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransitionInstallation {
    pub x_grabs_cleared: bool,
    pub pointer_state_cleared: bool,
    pub frozen_input_cleared: bool,
    pub snapshot_installed: bool,
}

impl TransitionInstallation {
    /// Everything a security-control transition must report.
    pub fn security_control() -> Self {
        Self {
            x_grabs_cleared: true,
            pointer_state_cleared: true,
            frozen_input_cleared: true,
            snapshot_installed: true,
        }
    }

    /// What a publication-only transition must report.
    pub fn publication() -> Self {
        Self {
            snapshot_installed: true,
            ..Self::default()
        }
    }

    fn satisfies(self, kind: TransitionKind) -> bool {
        match kind {
            // Every transition installs a snapshot; only a security change
            // additionally revokes what the old epoch authorised.
            TransitionKind::SecurityControl => {
                self.snapshot_installed
                    && self.x_grabs_cleared
                    && self.pointer_state_cleared
                    && self.frozen_input_cleared
            }
            TransitionKind::Publication => self.snapshot_installed,
        }
    }
}

/// One transition's identity, stamped onto work and carried unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlStamp {
    pub control_epoch: u64,
    pub publication: u64,
}

/// Names one transition, so its installation can only be reported against it.
///
/// Without this a report prepared for one transition could be filed against
/// whatever happens to be pending when it arrives, which is the same class of
/// mistake as restamping queued work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionToken {
    /// Which coordinator issued it.
    ///
    /// Without this the first token of every coordinator is the same value,
    /// so a report meant for one could be filed against another's pending
    /// transition and be accepted as its exact installation identity.
    coordinator: u64,
    transition: u64,
}

/// A transition that has been requested and not yet reopened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingTransition {
    /// Identifies this transition and no other.
    ///
    /// Application is tracked against this rather than against epoch equality,
    /// because a publication-only transition keeps its epoch, so equality is
    /// already true the moment it is requested and would wave it through.
    id: u64,
    kind: TransitionKind,
    control_epoch: u64,
    publication: u64,
}

/// Owns the request, the application and the reopening of a control epoch.
#[derive(Debug)]
pub struct ControlEpochCoordinator {
    requested_control_epoch: u64,
    applied_control_epoch: u64,
    committed_publication: u64,
    pending: Option<PendingTransition>,
    applied_transition: Option<u64>,
    incarnation: u64,
    next_transition: u64,
}

impl ControlEpochCoordinator {
    /// Read the coordinator's starting point from the authority it will drive.
    ///
    /// Taking no arguments would still allow a coordinator reset to zero to be
    /// attached to an authority that had already advanced, and every later
    /// transition would then be read as a far larger jump than intended: a
    /// publication-only change could arrive as a security change and revoke
    /// grants that should have survived. Deriving under the guard closes that
    /// directly rather than relying on the caller to have nothing to say.
    ///
    /// An authority mid-transition has no published revision to derive from,
    /// and refuses rather than offering the last good one.
    pub fn derive(
        authority: &AuthorityInstance,
        issuer: &IssuerHandle,
    ) -> Result<Self, ControlEpochRefusal> {
        let revision = authority
            .published_revision(issuer)
            .map_err(ControlEpochRefusal::Authority)?;
        Ok(Self {
            requested_control_epoch: revision.control_epoch,
            applied_control_epoch: revision.control_epoch,
            committed_publication: revision.publication,
            pending: None,
            applied_transition: None,
            incarnation: allocate_incarnation()?,
            next_transition: 1,
        })
    }

    pub fn requested_control_epoch(&self) -> u64 {
        self.requested_control_epoch
    }

    pub fn applied_control_epoch(&self) -> u64 {
        self.applied_control_epoch
    }

    pub fn committed_publication(&self) -> u64 {
        self.committed_publication
    }

    /// Whether routing is open.
    pub fn is_open(&self) -> bool {
        self.pending.is_none()
    }

    /// The kind of transition in flight, if any.
    pub fn pending_kind(&self) -> Option<TransitionKind> {
        self.pending.map(|pending| pending.kind)
    }

    /// The stamp work should carry, or a refusal.
    ///
    /// Work is stamped once, here, and that exact pair travels with it through
    /// queueing and thaw. Restamping on the way out would let work admitted
    /// under one transition execute as though it belonged to another.
    pub fn stamp(&self) -> Result<ControlStamp, ControlEpochRefusal> {
        if !self.is_open() {
            return Err(ControlEpochRefusal::TransitionPending);
        }
        Ok(ControlStamp {
            control_epoch: self.applied_control_epoch,
            publication: self.committed_publication,
        })
    }

    /// Close routing and bind the requested epoch and publication.
    ///
    /// The order is the whole point: the common authority is told first, so
    /// routing is already unavailable before the requested change is anywhere
    /// an observer could act on it.
    pub fn request(
        &mut self,
        authority: &mut AuthorityInstance,
        issuer: &IssuerHandle,
        kind: TransitionKind,
        control_epoch: u64,
        publication: u64,
    ) -> Result<TransitionToken, ControlEpochRefusal> {
        // No supersession. A second request while one is in flight would leave
        // an installation report in the air with nothing to match it against,
        // and the first transition's clearing half-done.
        if self.pending.is_some() {
            return Err(ControlEpochRefusal::TransitionPending);
        }
        if control_epoch < self.applied_control_epoch {
            return Err(ControlEpochRefusal::EpochWentBackwards {
                requested: control_epoch,
                current: self.applied_control_epoch,
            });
        }
        match kind {
            // A security change is defined by advancing the epoch. One that
            // kept it would revoke grabs while leaving work stamped with the
            // old epoch still admissible.
            TransitionKind::SecurityControl if control_epoch == self.applied_control_epoch => {
                return Err(ControlEpochRefusal::EpochUnchanged {
                    epoch: control_epoch,
                });
            }
            // A publication-only change must not move the epoch, or it would
            // be a security change that skipped revocation.
            TransitionKind::Publication if control_epoch != self.applied_control_epoch => {
                return Err(ControlEpochRefusal::EpochWentBackwards {
                    requested: control_epoch,
                    current: self.applied_control_epoch,
                });
            }
            _ => {}
        }
        // Checked before the authority is told, so a transition is never
        // opened that cannot be named.
        let id = self.next_transition;
        let next = next_transition_identity(id)?;
        authority
            .begin_transition(issuer, publication, control_epoch)
            .map_err(ControlEpochRefusal::Authority)?;
        self.next_transition = next;
        self.requested_control_epoch = control_epoch;
        self.pending = Some(PendingTransition {
            id,
            kind,
            control_epoch,
            publication,
        });
        Ok(TransitionToken {
            coordinator: self.incarnation,
            transition: id,
        })
    }

    /// Record what the transition installed.
    ///
    /// The applied epoch takes the exact requested value. It is never
    /// incremented independently: a requested epoch may jump, and a counter
    /// advancing by one would name a transition that never happened.
    pub fn apply(
        &mut self,
        token: TransitionToken,
        installed: TransitionInstallation,
    ) -> Result<(), ControlEpochRefusal> {
        let pending = self.pending.ok_or(ControlEpochRefusal::NoTransition)?;
        let expected = TransitionToken {
            coordinator: self.incarnation,
            transition: pending.id,
        };
        if token != expected {
            return Err(ControlEpochRefusal::WrongTransition);
        }
        if !installed.satisfies(pending.kind) {
            return Err(ControlEpochRefusal::NotInstalled { kind: pending.kind });
        }
        self.applied_control_epoch = pending.control_epoch;
        self.applied_transition = Some(pending.id);
        Ok(())
    }

    /// Reopen routing once Session commits the publication this transition
    /// was requested under.
    ///
    /// A publication that does not match leaves the coordinator closed. There
    /// is no fallback to the last good value, because the state this
    /// coordinator guards was already changed for a different one.
    pub fn reopen(
        &mut self,
        authority: &mut AuthorityInstance,
        issuer: &IssuerHandle,
        publication: u64,
    ) -> Result<(), ControlEpochRefusal> {
        let pending = self.pending.ok_or(ControlEpochRefusal::NoTransition)?;
        if publication != pending.publication {
            return Err(ControlEpochRefusal::PublicationMismatch {
                stamped: publication,
                committed: pending.publication,
            });
        }
        // Against this transition's own identity, not against epoch equality,
        // which a publication-only transition satisfies before it has done
        // anything at all.
        if self.applied_transition != Some(pending.id) {
            return Err(ControlEpochRefusal::NotInstalled { kind: pending.kind });
        }
        authority
            .publish(issuer, publication, pending.control_epoch)
            .map_err(ControlEpochRefusal::Authority)?;
        self.committed_publication = publication;
        self.pending = None;
        Ok(())
    }

    /// Whether work carrying this stamp may execute now.
    pub fn admits(&self, stamp: ControlStamp) -> Result<(), ControlEpochRefusal> {
        if !self.is_open() {
            return Err(ControlEpochRefusal::TransitionPending);
        }
        if stamp.control_epoch != self.applied_control_epoch {
            return Err(ControlEpochRefusal::EpochMismatch {
                stamped: stamp.control_epoch,
                applied: self.applied_control_epoch,
            });
        }
        if stamp.publication != self.committed_publication {
            return Err(ControlEpochRefusal::PublicationMismatch {
                stamped: stamp.publication,
                committed: self.committed_publication,
            });
        }
        Ok(())
    }
}

/// Take the next coordinator incarnation, or refuse.
///
/// Wrapping here would eventually hand a new coordinator an incarnation an
/// older one still stamps its tokens with, which is exactly the confusion the
/// incarnation exists to prevent.
fn allocate_incarnation() -> Result<u64, ControlEpochRefusal> {
    COORDINATOR_INCARNATIONS
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| ControlEpochRefusal::CoordinatorIncarnationsExhausted)
}

/// The identity a transition after this one would carry.
///
/// A rule rather than a line inside `request`, so exhaustion can be shown
/// without a constructor that puts a coordinator into a state it could never
/// reach on its own. Saturating here would hand out an identity that an
/// earlier installation still answers to.
pub fn next_transition_identity(current: u64) -> Result<u64, ControlEpochRefusal> {
    current
        .checked_add(1)
        .ok_or(ControlEpochRefusal::TransitionIdentitiesExhausted)
}

/// A coordinator shared between the thread that drives transitions and the
/// threads that stamp and admit work against them.
///
/// Enqueue and delivery run on different threads from the Session loop that
/// requests a transition, so the coordinator they consult has to be the same
/// one, not a copy that could describe a different moment.
#[derive(Clone, Debug)]
pub struct ControlEpochGate {
    coordinator: std::sync::Arc<std::sync::Mutex<ControlEpochCoordinator>>,
}

impl ControlEpochGate {
    pub fn new(coordinator: ControlEpochCoordinator) -> Self {
        Self {
            coordinator: std::sync::Arc::new(std::sync::Mutex::new(coordinator)),
        }
    }

    /// The stamp new work should carry, or a refusal.
    ///
    /// A poisoned guard refuses rather than reporting a value: the coordinator
    /// behind it is of unknown currency, and admitting work against an unknown
    /// transition state is the failure this type exists to prevent.
    pub fn stamp(&self) -> Result<ControlStamp, ControlEpochRefusal> {
        self.coordinator
            .lock()
            .map_err(|_| ControlEpochRefusal::TransitionPending)?
            .stamp()
    }

    /// Whether work carrying this stamp may be delivered now.
    pub fn admits(&self, stamp: ControlStamp) -> Result<(), ControlEpochRefusal> {
        self.coordinator
            .lock()
            .map_err(|_| ControlEpochRefusal::TransitionPending)?
            .admits(stamp)
    }

    /// Drive a transition. Used by whoever owns the authority, not by the
    /// threads that merely stamp and admit.
    pub fn with<R>(
        &self,
        act: impl FnOnce(&mut ControlEpochCoordinator) -> R,
    ) -> Result<R, ControlEpochRefusal> {
        let mut coordinator = self
            .coordinator
            .lock()
            .map_err(|_| ControlEpochRefusal::TransitionPending)?;
        Ok(act(&mut coordinator))
    }
}

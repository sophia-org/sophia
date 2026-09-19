//! The outer owner of one private input service's lifetime.
//!
//! RESERVED BEFORE ANYTHING IS STARTED. Custody of a service that ended with
//! work still owed has to have a home, and that home cannot be the value the
//! stop happens to return: a caller that drops the outcome would drop the
//! custody with it, and a caller that never calls stop at all leaves the
//! controller's drop with nowhere to put anything. The slot is allocated
//! before the service exists, so placing custody into it later cannot fail for
//! want of room -- the one moment at which failing would be worst.
//!
//! INDEPENDENT OF THE HANDLES. This is not the controller and not a
//! submission. A controller is dropped when a caller has finished driving the
//! service; a submission is handed to an adapter. Neither is a lifetime, and
//! tying custody to either made the end of a service depend on which of them
//! happened to outlive the other. This outlives both, by the caller's own
//! scope.
//!
//! ONE SLOT, ONE SERVICE AT A TIME. The slot is claimed before the service is
//! spawned, not after it ends. Claiming afterwards let one lifetime start any
//! number of services against a single empty slot, and the first of them to
//! end with work owed would take the only home there was.
//!
//! NO CYCLE. The slot holds the runtime strongly, because that is what custody
//! means. The runtime reaches the slot weakly, because it only ever needs to
//! put itself there and must not keep it alive to do so.
//!
//! NOTHING HERE SETTLES ANYTHING, AND NOTHING HERE LETS GO. Placing a runtime
//! records that it ended with work outstanding. There is deliberately no call
//! that takes the retained runtime back out: taking the only handle to
//! unresolved work and dropping it is a disposal however it is described, and
//! a caller that could reach for it would have a way to make outstanding
//! obligations disappear without resolving one of them.

use std::sync::{Arc, Mutex};

use super::handle::{PrivateInputHandle, PrivateInputRefusal};
use super::service::PrivateInputRuntime;

/// What a lifetime's closing slot holds.
enum PrivateInputSlotState {
    /// Nothing has been started under this lifetime.
    Vacant,
    /// A service is running under it. The slot is spoken for.
    Claimed,
    /// A service ended with work owed, and this is that service.
    Retained(Arc<PrivateInputRuntime>),
}

/// The reserved home for one service's unresolved runtime.
pub(super) struct PrivateInputClosingSlot {
    state: Mutex<PrivateInputSlotState>,
    /// Whether a thread ever panicked holding the slot.
    poisoned: std::sync::atomic::AtomicBool,
}

impl PrivateInputClosingSlot {
    /// Put the runtime in the reserved slot.
    ///
    /// RECOVERS A POISONED SLOT RATHER THAN FAILING INTO IT. A lock this could
    /// not take would leave the only home for this runtime unreachable at
    /// exactly the moment custody needed one. The poisoning is a fact about a
    /// thread that panicked while holding the lock, not a reason to lose the
    /// work; the state behind it is a single enum that is always one of its
    /// own variants, so taking it back is safe. That the slot was poisoned is
    /// preserved and reported.
    pub(super) fn place(&self, runtime: Arc<PrivateInputRuntime>) -> bool {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                self.poisoned
                    .store(true, std::sync::atomic::Ordering::Release);
                poisoned.into_inner()
            }
        };
        // ALREADY OCCUPIED MEANS SOMETHING ENDED TWICE. The first one stands;
        // overwriting would discard the custody of whichever ended first. The
        // caller still holds its own reference either way, so refusing here
        // loses nothing.
        if matches!(*state, PrivateInputSlotState::Retained(_)) {
            return false;
        }
        *state = PrivateInputSlotState::Retained(runtime);
        true
    }

    /// Give the claim back, for a service that ended owing nothing.
    ///
    /// NOT A DISPOSAL. There is nothing retained to dispose of: this is the
    /// claim a start took, returned by a service that finished clean, so the
    /// lifetime can be used again. A slot holding a retained runtime is left
    /// exactly as it is.
    pub(super) fn release_claim(&self) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                self.poisoned
                    .store(true, std::sync::atomic::Ordering::Release);
                poisoned.into_inner()
            }
        };
        if matches!(*state, PrivateInputSlotState::Claimed) {
            *state = PrivateInputSlotState::Vacant;
        }
    }
}

/// Owns one private input service's lifetime, from before it starts.
pub struct PrivateInputLifetimeOwner {
    closing: Arc<PrivateInputClosingSlot>,
}

impl PrivateInputLifetimeOwner {
    /// Reserve a lifetime, with its closing slot, before any service exists.
    pub fn reserved() -> Self {
        Self {
            closing: Arc::new(PrivateInputClosingSlot {
                state: Mutex::new(PrivateInputSlotState::Vacant),
                poisoned: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    /// Stand a service up under this lifetime.
    ///
    /// THE SLOT IS CLAIMED BEFORE THE SERVICE IS SPAWNED. A claim taken
    /// afterwards would let one lifetime start any number of services against
    /// a single empty slot, and the first to end with work owed would take the
    /// only home there was. A lifetime that is already running a service, or
    /// already holding one that ended with work owed, refuses rather than
    /// admitting a second; so does one whose slot cannot be read, because a
    /// service admitted under an unreadable lifetime is one whose custody has
    /// nowhere to go.
    ///
    /// The issuer, the authority instance, the durable store and the service
    /// owner are established here and never leave. What comes back can name
    /// connections, issue submissions, drain receipts and stop; it cannot
    /// reach any of those, and it is not what keeps the lifetime.
    pub fn start(
        &self,
        config: super::PrivateInputConfig,
    ) -> Result<PrivateInputHandle, PrivateInputRefusal> {
        self.start_with_faults(config, super::faults::PrivateInputFaults::default())
    }

    /// Stand a service up with test-only faults armed against it.
    ///
    /// cfg(test) ONLY. There is no release path that reaches a fault, and the
    /// carrier is an empty struct outside test builds.
    #[cfg(test)]
    pub(super) fn start_with_faults(
        &self,
        config: super::PrivateInputConfig,
        faults: super::faults::PrivateInputFaults,
    ) -> Result<PrivateInputHandle, PrivateInputRefusal> {
        self.start_checked(config, faults)
    }

    #[cfg(not(test))]
    fn start_with_faults(
        &self,
        config: super::PrivateInputConfig,
        faults: super::faults::PrivateInputFaults,
    ) -> Result<PrivateInputHandle, PrivateInputRefusal> {
        self.start_checked(config, faults)
    }

    fn start_checked(
        &self,
        config: super::PrivateInputConfig,
        faults: super::faults::PrivateInputFaults,
    ) -> Result<PrivateInputHandle, PrivateInputRefusal> {
        {
            let mut state = self
                .closing
                .state
                .lock()
                .map_err(|_| PrivateInputRefusal::LifetimeUnreadable)?;
            match *state {
                PrivateInputSlotState::Vacant => *state = PrivateInputSlotState::Claimed,
                PrivateInputSlotState::Claimed => {
                    return Err(PrivateInputRefusal::LifetimeInUse);
                }
                PrivateInputSlotState::Retained(_) => {
                    return Err(PrivateInputRefusal::LifetimeRetainsUnresolved);
                }
            }
        }
        match PrivateInputRuntime::start(config, Arc::downgrade(&self.closing), faults) {
            Ok(runtime) => Ok(PrivateInputHandle {
                runtime: Arc::new(runtime),
            }),
            Err(refusal) => {
                // Nothing was started, so the claim goes back rather than
                // making this lifetime unusable for a configuration that was
                // never served.
                self.closing.release_claim();
                Err(refusal)
            }
        }
    }

    /// Whether the closing slot holds a runtime that ended with work owed.
    ///
    /// `false` IS NOT "NOTHING WAS OWED" UNLESS THE SERVICE HAS ENDED. A
    /// service still running has placed nothing, which is not a statement
    /// about its obligations. Recovers a poisoned slot rather than refusing to
    /// answer; [`Self::slot_poisoned`] is how a reader learns that happened.
    pub fn retains_unresolved(&self) -> bool {
        let state = match self.closing.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                self.closing
                    .poisoned
                    .store(true, std::sync::atomic::Ordering::Release);
                poisoned.into_inner()
            }
        };
        matches!(*state, PrivateInputSlotState::Retained(_))
    }

    /// Whether this lifetime is running a service right now.
    pub fn in_use(&self) -> bool {
        let state = match self.closing.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                self.closing
                    .poisoned
                    .store(true, std::sync::atomic::Ordering::Release);
                poisoned.into_inner()
            }
        };
        matches!(*state, PrivateInputSlotState::Claimed)
    }

    /// What the retained runtime still owes, if one is held.
    ///
    /// READS THE CUSTODY WITHOUT HANDING IT OVER. A caller needs to be able to
    /// see that something is still owed and how much, which is exactly the
    /// question this slot exists to answer; it does not need the runtime
    /// itself, and giving it out would be the disposal escape this type
    /// deliberately lacks. `None` means nothing is retained; `Some(None)`
    /// means a runtime is retained and its own record could not be read.
    pub fn unresolved_receipts(&self) -> Option<Option<usize>> {
        let state = match self.closing.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                self.closing
                    .poisoned
                    .store(true, std::sync::atomic::Ordering::Release);
                poisoned.into_inner()
            }
        };
        match &*state {
            PrivateInputSlotState::Retained(runtime) => {
                Some(runtime.retained_receipts.lock().map(|held| held.len()).ok())
            }
            _ => None,
        }
    }

    /// Whether a thread ever panicked holding this slot.
    ///
    /// KEPT AS ITS OWN FACT. The readings above recover from poisoning so that
    /// custody is never lost to it, which would otherwise make the poisoning
    /// invisible.
    pub fn slot_poisoned(&self) -> bool {
        // ASKS THE LOCK ITSELF, NOT ONLY THE FLAG. The flag is set by the
        // accessors that recover from poisoning, so a caller that asked this
        // first -- before anything had recovered -- would have been told no
        // while the slot was already poisoned.
        self.closing.state.is_poisoned()
            || self
                .closing
                .poisoned
                .load(std::sync::atomic::Ordering::Acquire)
    }
}

impl core::fmt::Debug for PrivateInputLifetimeOwner {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("PrivateInputLifetimeOwner")
            .field("in_use", &self.in_use())
            .field("retains_unresolved", &self.retains_unresolved())
            .field("slot_poisoned", &self.slot_poisoned())
            .field("unresolved_receipts", &self.unresolved_receipts())
            .finish()
    }
}

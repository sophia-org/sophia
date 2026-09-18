use super::*;
use crate::application_catalog::ApplicationCatalogEntry;
use sophia_protocol::{CatalogActivation, ContentGrant, NativeLauncherActivation};

/// Exact launch origin; a persistent catalog click never acquires a transient
/// opening or keyboard lease. Presentation authorization precedes queue entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogLaunchCause {
    Transient(NativeLauncherActivation),
    Persistent(CatalogActivation),
}

impl CatalogLaunchCause {
    pub const fn grant(&self) -> ContentGrant {
        match self {
            Self::Transient(value) => value.event.binding.grant,
            Self::Persistent(value) => value.action.grant,
        }
    }

    fn slot(&self) -> u64 {
        match self {
            Self::Transient(value) => u64::from(value.slot),
            Self::Persistent(value) => value.action.action_id,
        }
    }
}

/// The actual catalog queue payload. Worker verification retains this same
/// entry and exact origin; a client transaction is never the Session serial.
#[derive(Clone, Debug)]
pub struct NativeCatalogLaunch {
    pub transaction: TransactionId,
    pub cause: CatalogLaunchCause,
    pub entry: Arc<ApplicationCatalogEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCatalogLaunchRefusal {
    Stale,
    Unauthorized,
    Capacity,
    Exhausted,
}

#[derive(Clone, Debug)]
pub(super) struct CatalogDispatch {
    pub transaction: TransactionId,
    pub native: Option<CatalogLaunchCause>,
}

impl SessionLaunchQueue {
    /// Match the managed child against the current admission without treating
    /// numeric transactions from different catalog owners as interchangeable.
    pub fn matches_child_launch(
        &self,
        transaction: TransactionId,
        catalog: bool,
        native: Option<&NativeCatalogLaunch>,
    ) -> bool {
        if self
            .admission
            .is_none_or(|a| a.intent.transaction != transaction)
            || self.catalog_admission(transaction) != catalog
        {
            return false;
        }
        match (self.admitted_native.as_ref(), native) {
            (None, None) => true,
            (Some(_), Some(origin)) => self.native_catalog_admission(origin),
            _ => false,
        }
    }

    pub fn enqueue_native_catalog(
        &mut self,
        activation: NativeLauncherActivation,
        entry: Arc<ApplicationCatalogEntry>,
        application: SessionApplicationId,
        active: usize,
    ) -> Result<TransactionId, NativeCatalogLaunchRefusal> {
        self.enqueue_catalog_cause(
            CatalogLaunchCause::Transient(activation),
            entry,
            application,
            active,
        )
    }

    /// Queue an already-authorized persistent event under the same bounded
    /// execution owner as a transient menu. This validates shape, not Presented.
    pub fn enqueue_persistent_catalog(
        &mut self,
        activation: CatalogActivation,
        entry: Arc<ApplicationCatalogEntry>,
        application: SessionApplicationId,
        active: usize,
    ) -> Result<TransactionId, NativeCatalogLaunchRefusal> {
        sophia_protocol::encode_shell_catalog_action_frame(
            TransactionId::from_raw(1),
            &sophia_protocol::ShellCatalogActionRecord::Activate(activation.clone()),
        )
        .map_err(|_| NativeCatalogLaunchRefusal::Unauthorized)?;
        self.enqueue_catalog_cause(
            CatalogLaunchCause::Persistent(activation),
            entry,
            application,
            active,
        )
    }

    fn enqueue_catalog_cause(
        &mut self,
        cause: CatalogLaunchCause,
        entry: Arc<ApplicationCatalogEntry>,
        application: SessionApplicationId,
        active: usize,
    ) -> Result<TransactionId, NativeCatalogLaunchRefusal> {
        if u64::from(entry.descriptor.slot) != cause.slot()
            || !entry.descriptor.available
            || entry.command.is_none()
        {
            return Err(NativeCatalogLaunchRefusal::Unauthorized);
        }
        if self
            .admitted_native
            .as_ref()
            .is_some_and(|v| v.cause == cause)
            || self
                .pending
                .iter()
                .any(|v| v.native.as_ref().is_some_and(|v| v.cause == cause))
        {
            return Err(NativeCatalogLaunchRefusal::Stale);
        }
        let mut serial = self.next_native_transaction.max(1);
        // At most sixteen pending launches and one current admission can name
        // a serial. Keep catalog/WM/native numeric collisions out of this mint.
        loop {
            self.next_native_transaction = serial
                .checked_add(1)
                .ok_or(NativeCatalogLaunchRefusal::Exhausted)?;
            if !self
                .pending
                .iter()
                .any(|v| v.intent.transaction.raw() == serial)
                && self
                    .admission
                    .is_none_or(|v| v.intent.transaction.raw() != serial)
            {
                break;
            }
            serial = self.next_native_transaction;
        }
        let transaction = TransactionId::from_raw(serial);
        let payload = Arc::new(NativeCatalogLaunch {
            transaction,
            cause,
            entry,
        });
        let intent = SessionLaunchIntent {
            transaction,
            application,
            placement_classification: None,
        };
        if !matches!(
            self.enqueue_catalog(intent, active),
            SessionLaunchQueueOutcome::Queued { .. }
        ) {
            return Err(NativeCatalogLaunchRefusal::Capacity);
        }
        self.pending
            .back_mut()
            .expect("accepted queue owner")
            .native = Some(payload);
        Ok(transaction)
    }

    /// Rechecked immediately before execution; join/verification success alone
    /// cannot authorize a replaced grant or a different queued command.
    pub fn native_catalog_admission(&self, launch: &NativeCatalogLaunch) -> bool {
        self.catalog_admission(launch.transaction)
            && self.admitted_native.as_ref().is_some_and(|current| {
                current.transaction == launch.transaction
                    && current.cause == launch.cause
                    && Arc::ptr_eq(&current.entry, &launch.entry)
            })
    }

    /// Consume the one execution attempt immediately before the Session's spawn
    /// call. Verification alone cannot grant authority. Once attempted, grant
    /// revocation must not erase an application's first-window attribution;
    /// spawn failure is settled separately with exact cancellation.
    pub fn begin_native_catalog_execution(
        &mut self,
        launch: &NativeCatalogLaunch,
        current_grant: ContentGrant,
        verified: &crate::application_catalog::ApplicationLaunchCommand,
    ) -> bool {
        if self.native_execution_attempted
            || !self.native_dispatch_taken
            || !self.native_catalog_admission(launch)
            || current_grant != launch.cause.grant()
            || launch.entry.command.as_ref() != Some(verified)
        {
            return false;
        }
        self.native_execution_attempted = true;
        self.catalog_dispatch = None;
        true
    }

    /// Observe the exact ready owner without transferring the dispatch. The
    /// component scheduler uses it to borrow the matching live connection.
    pub fn native_catalog_dispatch_grant(&self) -> Option<ContentGrant> {
        let dispatch = self.catalog_dispatch.as_ref()?;
        let activation = dispatch.native.as_ref()?;
        let current = self.admitted_native.as_ref()?;
        if current.transaction != dispatch.transaction
            || &current.cause != activation
            || !self.native_catalog_admission(current)
        {
            return None;
        }
        Some(current.cause.grant())
    }

    pub fn take_native_catalog_dispatch(&mut self) -> Option<Arc<NativeCatalogLaunch>> {
        self.native_catalog_dispatch_grant()?;
        let current = self.admitted_native.as_ref()?;
        let result = Arc::clone(current);
        self.catalog_dispatch = None;
        self.native_dispatch_taken = true;
        Some(result)
    }

    /// Revocation only, not ordinary launcher dismissal after queue admission.
    /// The returned worker payload may stay alive, but loses execution authority.
    pub fn revoke_native_catalog_grant(&mut self, grant: ContentGrant) -> usize {
        let before = self.pending.len();
        self.pending.retain(|launch| {
            launch
                .native
                .as_ref()
                .is_none_or(|v| v.cause.grant() != grant)
        });
        let mut removed = before - self.pending.len();
        if !self.native_execution_attempted
            && self
                .admitted_native
                .as_ref()
                .is_some_and(|v| v.cause.grant() == grant)
        {
            self.take_admission();
            removed += 1;
        }
        removed
    }

    pub fn reject_native_before_execution(&mut self, launch: &NativeCatalogLaunch) {
        if !self.native_execution_attempted {
            self.cancel_native_catalog(launch);
        }
    }

    pub fn cancel_native_catalog(&mut self, launch: &NativeCatalogLaunch) {
        self.pending.retain(|v| {
            v.native.as_ref().is_none_or(|v| {
                v.transaction != launch.transaction
                    || v.cause != launch.cause
                    || !Arc::ptr_eq(&v.entry, &launch.entry)
            })
        });
        if self.native_catalog_admission(launch) {
            self.take_admission();
        }
    }
}

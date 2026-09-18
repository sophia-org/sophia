use super::*;
use crate::application_catalog::ApplicationCatalogEntry;
use sophia_protocol::{ContentGrant, NativeLauncherActivation};

/// The actual catalog queue payload. Worker verification retains this same
/// entry and exact origin; a client transaction is never the Session serial.
#[derive(Clone, Debug)]
pub struct NativeCatalogLaunch {
    pub transaction: TransactionId,
    pub activation: NativeLauncherActivation,
    pub entry: Arc<ApplicationCatalogEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeCatalogLaunchRefusal {
    Stale,
    Unauthorized,
    Capacity,
    Exhausted,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CatalogDispatch {
    pub transaction: TransactionId,
    pub native: Option<NativeLauncherActivation>,
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
        if entry.descriptor.slot != activation.slot
            || !entry.descriptor.available
            || entry.command.is_none()
        {
            return Err(NativeCatalogLaunchRefusal::Unauthorized);
        }
        if self
            .admitted_native
            .as_ref()
            .is_some_and(|v| v.activation == activation)
            || self.pending.iter().any(|v| {
                v.native
                    .as_ref()
                    .is_some_and(|v| v.activation == activation)
            })
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
            activation,
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
                    && current.activation == launch.activation
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
            || current_grant != launch.activation.event.binding.grant
            || launch.entry.command.as_ref() != Some(verified)
        {
            return false;
        }
        self.native_execution_attempted = true;
        self.catalog_dispatch = None;
        true
    }

    pub fn take_native_catalog_dispatch(&mut self) -> Option<Arc<NativeCatalogLaunch>> {
        let dispatch = self.catalog_dispatch?;
        let activation = dispatch.native?;
        let current = self.admitted_native.as_ref()?;
        if current.transaction != dispatch.transaction
            || current.activation != activation
            || !self.native_catalog_admission(current)
        {
            return None;
        }
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
                .is_none_or(|v| v.activation.event.binding.grant != grant)
        });
        let mut removed = before - self.pending.len();
        if !self.native_execution_attempted
            && self
                .admitted_native
                .as_ref()
                .is_some_and(|v| v.activation.event.binding.grant == grant)
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
                    || v.activation != launch.activation
                    || !Arc::ptr_eq(&v.entry, &launch.entry)
            })
        });
        if self.native_catalog_admission(launch) {
            self.take_admission();
        }
    }
}

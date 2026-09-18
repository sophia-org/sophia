use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use sophia_protocol::{SessionApplicationId, SurfaceId, TransactionId};

mod native_catalog;
use native_catalog::CatalogDispatch;
pub use native_catalog::{NativeCatalogLaunch, NativeCatalogLaunchRefusal};

pub const SESSION_ACTION_APPLICATION_CAPACITY: usize = 16;
pub const SESSION_ACTION_SURFACE_CAPACITY: usize = 16;

/// Session-owned executable data, retained independently of later configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionLaunchCommand {
    pub id: String,
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub placement_classification: Option<u64>,
}

#[derive(Debug)]
struct QueuedLaunch {
    intent: SessionLaunchIntent,
    catalog: bool,
    native: Option<Arc<NativeCatalogLaunch>>,
    command: Option<Arc<SessionLaunchCommand>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLaunchIntent {
    pub transaction: TransactionId,
    pub application: SessionApplicationId,
    pub placement_classification: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLaunchSurfaceObservation {
    pub intent: SessionLaunchIntent,
    pub surface: SurfaceId,
    /// Present only for the first surface observed from a classified launch.
    pub placement_classification: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLaunchAdmission {
    pub intent: SessionLaunchIntent,
    observed_surfaces: [Option<SurfaceId>; SESSION_ACTION_SURFACE_CAPACITY],
    observed_surface_count: usize,
    placement_classification_consumed: bool,
}

impl SessionLaunchAdmission {
    pub fn observed_surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.observed_surfaces.iter().flatten().copied()
    }

    pub const fn has_observed_surface(&self) -> bool {
        self.observed_surface_count != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLaunchQueueOutcome {
    Queued { depth: usize },
    RejectedCapacity,
}

#[derive(Debug, Default)]
pub struct SessionLaunchQueue {
    pending: VecDeque<QueuedLaunch>,
    admitted_command: Option<Arc<SessionLaunchCommand>>,
    admission_from_catalog: bool,
    catalog_dispatch: Option<CatalogDispatch>,
    admitted_native: Option<Arc<NativeCatalogLaunch>>,
    next_native_transaction: u64,
    native_execution_attempted: bool,
    native_dispatch_taken: bool,
    admission: Option<SessionLaunchAdmission>,
    peak_depth: usize,
    rejected: usize,
    timed_out: usize,
    withdrawn: usize,
}

impl SessionLaunchQueue {
    pub fn enqueue_catalog(
        &mut self,
        intent: SessionLaunchIntent,
        active: usize,
    ) -> SessionLaunchQueueOutcome {
        let result = self.enqueue(intent, active);
        if matches!(result, SessionLaunchQueueOutcome::Queued { .. }) {
            self.pending
                .back_mut()
                .expect("successful enqueue retains an entry")
                .catalog = true;
        }
        result
    }
    /// Transactions are scoped to their issuer. A WM transaction with the
    /// same number cannot consume or cancel a shell-authorized launch.
    pub fn catalog_admission(&self, transaction: TransactionId) -> bool {
        self.admission_from_catalog
            && self
                .admission
                .is_some_and(|a| a.intent.transaction == transaction)
    }
    pub fn dispatch_catalog(&mut self, transaction: TransactionId) -> bool {
        if !self.catalog_admission(transaction)
            || self.catalog_dispatch.is_some()
            || self.native_execution_attempted
            || self.native_dispatch_taken
        {
            return false;
        }
        self.catalog_dispatch = Some(CatalogDispatch {
            transaction,
            native: self.admitted_native.as_ref().map(|owner| owner.activation),
        });
        true
    }
    pub fn take_catalog_dispatch(&mut self) -> Option<TransactionId> {
        let dispatch = self.catalog_dispatch?;
        if dispatch.native.is_some() {
            return None;
        }
        self.catalog_dispatch = None;
        Some(dispatch.transaction)
    }
    pub fn cancel_catalog(&mut self, transaction: TransactionId) {
        self.pending.retain(|launch| {
            !launch.catalog || launch.native.is_some() || launch.intent.transaction != transaction
        });
        if self.catalog_dispatch.is_some_and(|dispatch| {
            dispatch.transaction == transaction && dispatch.native.is_none()
        }) {
            self.catalog_dispatch = None;
        }
        if self.admitted_native.is_none() && self.catalog_admission(transaction) {
            self.take_admission();
        }
    }

    pub fn enqueue(
        &mut self,
        intent: SessionLaunchIntent,
        active_applications: usize,
    ) -> SessionLaunchQueueOutcome {
        if active_applications.saturating_add(self.pending.len())
            >= SESSION_ACTION_APPLICATION_CAPACITY
        {
            self.rejected = self.rejected.saturating_add(1);
            return SessionLaunchQueueOutcome::RejectedCapacity;
        }
        self.pending.push_back(QueuedLaunch {
            intent,
            catalog: false,
            native: None,
            command: None,
        });
        self.peak_depth = self.peak_depth.max(self.pending.len());
        SessionLaunchQueueOutcome::Queued {
            depth: self.pending.len(),
        }
    }

    /// Capture an authorized command before it can wait behind another launch.
    pub fn enqueue_command(
        &mut self,
        mut intent: SessionLaunchIntent,
        command: Arc<SessionLaunchCommand>,
        active_applications: usize,
    ) -> SessionLaunchQueueOutcome {
        intent.placement_classification = command.placement_classification;
        let result = self.enqueue(intent, active_applications);
        if matches!(result, SessionLaunchQueueOutcome::Queued { .. }) {
            self.pending
                .back_mut()
                .expect("accepted launch is queued")
                .command = Some(command);
        }
        result
    }

    pub fn take_admitted_command(
        &mut self,
        transaction: TransactionId,
    ) -> Option<Arc<SessionLaunchCommand>> {
        if self.admission_from_catalog
            || self
                .admission
                .is_none_or(|entry| entry.intent.transaction != transaction)
        {
            return None;
        }
        self.admitted_command.take()
    }

    /// A process without placement metadata does not owe a first window.
    pub fn complete_spawn(&mut self, transaction: TransactionId) -> Option<SessionLaunchAdmission> {
        let admission = self.admission?;
        if self.admission_from_catalog
            || admission.intent.transaction != transaction
            || admission.intent.placement_classification.is_some()
        {
            return None;
        }
        self.take_admission()
    }

    pub fn complete_successful_exit(
        &mut self,
        transaction: TransactionId,
        catalog: bool,
    ) -> Option<SessionLaunchAdmission> {
        if self.admission_from_catalog != catalog
            || self
                .admission
                .is_none_or(|entry| entry.intent.transaction != transaction)
            || (catalog
                && self
                    .admission
                    .is_some_and(|entry| !entry.has_observed_surface()))
        {
            return None;
        }
        self.take_admission()
    }

    fn take_admission(&mut self) -> Option<SessionLaunchAdmission> {
        self.admitted_command = None;
        self.admitted_native = None;
        self.native_execution_attempted = false;
        self.native_dispatch_taken = false;
        self.catalog_dispatch = None;
        self.admission.take()
    }

    /// Admission is independent of application proof evidence. The owner only
    /// enqueues authorized, committed intents after authority activation.
    pub fn begin_next(&mut self, admission_pipeline_idle: bool) -> Option<SessionLaunchIntent> {
        if !admission_pipeline_idle || self.admission.is_some() {
            return None;
        }
        let queued = self.pending.pop_front()?;
        let intent = queued.intent;
        self.admission_from_catalog = queued.catalog;
        self.admitted_command = queued.command;
        self.admitted_native = queued.native;
        self.admission = Some(SessionLaunchAdmission {
            intent,
            observed_surfaces: [None; SESSION_ACTION_SURFACE_CAPACITY],
            observed_surface_count: 0,
            placement_classification_consumed: false,
        });
        Some(intent)
    }

    pub fn observe_surface(
        &mut self,
        surface: SurfaceId,
    ) -> Option<SessionLaunchSurfaceObservation> {
        let admission = self.admission.as_mut()?;
        if admission
            .observed_surfaces()
            .any(|candidate| candidate == surface)
            || admission.observed_surface_count >= SESSION_ACTION_SURFACE_CAPACITY
        {
            return None;
        }
        admission.observed_surfaces[admission.observed_surface_count] = Some(surface);
        admission.observed_surface_count += 1;
        let placement_classification = (!admission.placement_classification_consumed)
            .then_some(admission.intent.placement_classification)
            .flatten();
        admission.placement_classification_consumed |= placement_classification.is_some();
        Some(SessionLaunchSurfaceObservation {
            intent: admission.intent,
            surface,
            placement_classification,
        })
    }

    pub fn complete_if_stable(
        &mut self,
        admission_pipeline_idle: bool,
        stable_surface: Option<SurfaceId>,
    ) -> Option<SessionLaunchAdmission> {
        let admission = self.admission?;
        if !admission_pipeline_idle
            || !stable_surface
                .is_some_and(|surface| admission.observed_surfaces().any(|seen| seen == surface))
        {
            return None;
        }
        self.take_admission()
    }

    pub fn fail_current(&mut self) -> Option<SessionLaunchAdmission> {
        self.take_admission()
    }

    pub fn complete_observed_exit(&mut self) -> Option<SessionLaunchAdmission> {
        self.admission
            .is_some_and(|admission| admission.has_observed_surface())
            .then(|| self.take_admission())
            .flatten()
    }

    /// Abandons the outstanding launch when the coordinator gave up on a
    /// surface it was waiting for.
    ///
    /// A withdrawn surface is not a slow one: the admission it would have
    /// settled no longer exists, so waiting out the remaining budget only
    /// holds the queue shut behind a launch that can never complete.
    /// Counted apart from a timeout because it is a different outcome --
    /// the deadline was never reached.
    pub fn withdraw_current(&mut self, surfaces: &[SurfaceId]) -> Option<SessionLaunchAdmission> {
        let admission = self.admission?;
        if !admission
            .observed_surfaces()
            .any(|seen| surfaces.contains(&seen))
        {
            return None;
        }
        self.withdrawn = self.withdrawn.saturating_add(1);
        self.take_admission()
    }

    pub fn timeout_current(&mut self) -> Option<SessionLaunchAdmission> {
        let admission = self.take_admission()?;
        self.timed_out = self.timed_out.saturating_add(1);
        Some(admission)
    }

    pub fn cancel_pending(&mut self) -> usize {
        let cancelled = self.pending.len();
        self.pending.clear();
        self.catalog_dispatch = None;
        cancelled
    }

    pub fn admission(&self) -> Option<SessionLaunchAdmission> {
        self.admission
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn peak_depth(&self) -> usize {
        self.peak_depth
    }

    pub fn rejected(&self) -> usize {
        self.rejected
    }

    pub fn timed_out(&self) -> usize {
        self.timed_out
    }

    pub fn withdrawn(&self) -> usize {
        self.withdrawn
    }
}

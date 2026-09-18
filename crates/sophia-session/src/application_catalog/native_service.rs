//! Bounded catalog worker/execution ownership for one native component.
use super::*;
use crate::session_actions::{NativeCatalogLaunch, SessionLaunchQueue};
use sophia_runtime::ShellTransportConnection;
use std::sync::Arc;

const VERIFY_TIMEOUT_MSEC: u64 = 5_000;
struct PendingVerification {
    launch: Arc<NativeCatalogLaunch>,
    deadline: u64,
    submitted: bool,
    rejected: bool,
}

pub enum NativeCatalogServiceEvent {
    Idle,
    Catalog(u64, Result<ApplicationCatalog, String>),
    Started(NativeCatalogChild),
    Rejected,
    SpawnFailed(std::io::Error),
    Unavailable,
}

/// Retain this owner through shutdown until `poll_shutdown` succeeds. A worker
/// result and a launched child are separate owners; the caller must immediately
/// adopt Started's child+origin into Session process supervision.
pub struct NativeCatalogService {
    worker: ApplicationCatalogWorker,
    pending: Option<PendingVerification>,
    stopped: bool,
    last_visit: u64,
}
impl NativeCatalogService {
    pub fn start(
        config: sophia_config::ApplicationCatalogConfig,
        registered: Vec<RegisteredCatalogApplication>,
        environment: ApplicationCatalogEnvironment,
    ) -> std::io::Result<Self> {
        Ok(Self {
            worker: ApplicationCatalogWorker::start(config, registered, environment)?,
            pending: None,
            stopped: false,
            last_visit: 0,
        })
    }
    pub fn refresh(&mut self, generation: u64) -> bool {
        !self.stopped
            && self.pending.is_none()
            && generation != 0
            && self.worker.refresh(generation)
    }
    pub fn request_shutdown(&mut self, launches: &mut SessionLaunchQueue) {
        self.stopped = true;
        if let Some(pending) = &mut self.pending {
            launches.reject_native_before_execution(&pending.launch);
            pending.rejected = true;
        }
        self.worker.request_shutdown();
    }
    pub fn poll_shutdown(&mut self) -> Result<bool, &'static str> {
        if !self.stopped || self.pending.is_some() {
            return Ok(false);
        }
        self.worker.poll_shutdown()
    }

    /// Terminal owner visits drain at most one result without a process
    /// environment or connection. No shutdown result can execute an application.
    /// Keep this owner and its queue while the worker is still outstanding.
    pub fn drain_shutdown(
        &mut self,
        launches: &mut SessionLaunchQueue,
    ) -> Result<bool, &'static str> {
        self.request_shutdown(launches);
        if let Some(result) = self.worker.poll() {
            match result {
                ApplicationCatalogWorkerResult::NativeVerified(launch, _) => {
                    launches.reject_native_before_execution(&launch);
                    if self.pending.as_ref().is_none_or(|pending| {
                        !pending.submitted || !Arc::ptr_eq(&pending.launch, &launch)
                    }) {
                        return Err("shutdown catalog result has no exact owner");
                    }
                    self.pending = None;
                }
                ApplicationCatalogWorkerResult::Built(_, _) => {}
                ApplicationCatalogWorkerResult::Unavailable => {
                    // The worker join still reports a panic independently.
                    self.pending = None;
                }
                ApplicationCatalogWorkerResult::Verified(_, _) => {
                    return Err("legacy verification reached native catalog shutdown");
                }
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| !pending.submitted)
        {
            self.pending = None;
        }
        self.poll_shutdown()
    }

    /// One worker result and at most one dispatch submission per visit. Time is
    /// Session monotonic milliseconds; regression stops effects and drains work.
    pub fn service(
        &mut self,
        connection: Option<&ShellTransportConnection<'_>>,
        launches: &mut SessionLaunchQueue,
        environment: CatalogProcessEnvironment<'_>,
        now_msec: u64,
    ) -> NativeCatalogServiceEvent {
        if now_msec < self.last_visit {
            self.request_shutdown(launches);
        }
        self.last_visit = self.last_visit.max(now_msec);
        if let Some(pending) = &mut self.pending
            && (self.stopped
                || now_msec >= pending.deadline
                || connection
                    .is_none_or(|c| !execution::connection_permits_cause(c, &pending.launch.cause))
                || !launches.native_catalog_admission(&pending.launch))
        {
            launches.reject_native_before_execution(&pending.launch);
            pending.rejected = true;
        }
        if let Some(result) = self.worker.poll() {
            match result {
                ApplicationCatalogWorkerResult::NativeVerified(launch, verified) => {
                    let pending = self.pending.take();
                    let valid = pending.as_ref().is_some_and(|p| {
                        p.submitted && !p.rejected && Arc::ptr_eq(&p.launch, &launch)
                    });
                    if !valid || self.stopped || connection.is_none() {
                        launches.reject_native_before_execution(&launch);
                        return NativeCatalogServiceEvent::Rejected;
                    }
                    return match spawn_native_catalog(
                        connection.unwrap(),
                        launches,
                        launch,
                        verified,
                        environment,
                    ) {
                        Ok(child) => NativeCatalogServiceEvent::Started(child),
                        Err(NativeCatalogSpawnError::Refused) => {
                            NativeCatalogServiceEvent::Rejected
                        }
                        Err(NativeCatalogSpawnError::Spawn(error)) => {
                            NativeCatalogServiceEvent::SpawnFailed(error)
                        }
                    };
                }
                ApplicationCatalogWorkerResult::Built(generation, catalog) => {
                    return if self.stopped {
                        NativeCatalogServiceEvent::Rejected
                    } else {
                        NativeCatalogServiceEvent::Catalog(generation, catalog)
                    };
                }
                ApplicationCatalogWorkerResult::Unavailable if self.stopped => {
                    self.pending = None;
                    return NativeCatalogServiceEvent::Idle;
                }
                _ => {
                    self.request_shutdown(launches);
                    self.pending = None;
                    return NativeCatalogServiceEvent::Unavailable;
                }
            }
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.rejected && !p.submitted)
        {
            self.pending = None;
            return NativeCatalogServiceEvent::Rejected;
        }
        if self.stopped {
            return NativeCatalogServiceEvent::Idle;
        }
        if self.pending.is_none()
            && let Some(launch) = launches.take_native_catalog_dispatch()
        {
            let Some(deadline) = now_msec.checked_add(VERIFY_TIMEOUT_MSEC) else {
                launches.reject_native_before_execution(&launch);
                self.request_shutdown(launches);
                return NativeCatalogServiceEvent::Rejected;
            };
            self.pending = Some(PendingVerification {
                launch,
                deadline,
                submitted: false,
                rejected: false,
            });
        }
        if let Some(pending) = &mut self.pending
            && !pending.submitted
            && !pending.rejected
        {
            pending.submitted = self.worker.verify_native(Arc::clone(&pending.launch));
        }
        NativeCatalogServiceEvent::Idle
    }
}

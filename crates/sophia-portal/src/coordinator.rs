//! Protocol-neutral orchestration of the bounded upload handoff. This owner
//! contains no X identities, fids or client paths. Adapters retain those facts.
use crate::{
    execution_fence::PortalExecutionFence,
    file_client::PortalFileClient,
    file_service::SessionPortalFileService,
    files::{PortalFileAuthority, PortalFileExecution},
    handlers::PortalPayloadHandler,
};
use sophia_protocol::{
    NamespaceContext, NamespaceId, PortalBrokerResponseDecision, PortalGrant, PortalGrantState,
    PortalRequest, PortalTransfer, PortalTransferId, portal_files::PortalFileLimits,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub trait PortalPolicy: Send + Sync {
    /// Owner-derived facts only. Security changes must advance the shared
    /// execution fence before removing or replacing these facts, so a later
    /// re-admission cannot revive a lease from the earlier interval.
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext>;
    fn allow(&self, request: &PortalRequest) -> bool;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PortalTransferFailure {
    Pressure,
    Denied,
    Source,
    Transport,
    Recipient,
    Cancelled,
}

/// Collection may wait off the Session owner thread, bounded by `wait`.
pub trait PortalSource: Send + Sync {
    /// Read current authority facts without I/O. Generations never repeat
    /// within a registration; replacement/teardown advances the shared fence.
    fn generation(&self, request: &PortalRequest) -> Option<u64>;
    fn collect(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure>;
}

pub trait PortalRecipient: Send + Sync {
    /// Must revalidate retained protocol identity and never wait for network
    /// progress. The common owner holds the security fence around this effect.
    fn execute(&self, work: &PortalFileExecution, lease: PortalAccessLease) -> bool;
    /// Release retained protocol state and fail its waiter at most once.
    fn failed(&self, transfer: PortalTransferId, reason: PortalTransferFailure);
}

/// Recipient access survives successful installation, not its grant deadline
/// or any security/source transition. Only the common handoff can mint this
/// lease; a requester cannot turn a decoded grant into retained authority.
#[derive(Clone)]
pub struct PortalAccessLease {
    request: PortalRequest,
    grant: PortalGrant,
    revision: u64,
    fence: PortalExecutionFence,
    source: Arc<dyn PortalSource>,
    policy: Arc<dyn PortalPolicy>,
    stopped: Arc<AtomicBool>,
    accepted: Arc<AtomicBool>,
}
impl PortalAccessLease {
    pub fn grant(&self) -> &PortalGrant {
        &self.grant
    }
    /// The check and read share the lock/security barrier. `read` must not
    /// block, access a transport or call back into the same fence.
    pub fn with_access<T>(&self, read: impl FnOnce() -> T) -> Option<T> {
        if !self.accepted.load(Ordering::Acquire) {
            return None;
        }
        self.fence
            .execute(self.revision, self.grant.deadline_msec, || {
                self.current().then(read)
            })
            .flatten()
    }
    fn current(&self) -> bool {
        !self.stopped.load(Ordering::Acquire)
            && self.source.generation(&self.request) == Some(self.grant.source_generation)
            && self
                .policy
                .namespace(self.grant.source_namespace)
                .is_some_and(|n| n.capabilities.allows_publish(self.grant.kind.capability()))
            && self
                .policy
                .namespace(self.grant.target_namespace)
                .is_some_and(|n| n.capabilities.allows_request(self.grant.kind.capability()))
            && self.policy.allow(&self.request)
    }
}

pub struct PortalCollection {
    pub deadline: Instant,
    pub revision: u64,
    pub fence: PortalExecutionFence,
    stop: Arc<AtomicBool>,
}
impl PortalCollection {
    pub fn remaining(&self) -> Result<Duration, PortalTransferFailure> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero()
            || self.stop.load(Ordering::Acquire)
            || !self.fence.live()
            || self.fence.revision() != self.revision
        {
            Err(PortalTransferFailure::Cancelled)
        } else {
            Ok(remaining)
        }
    }
}

struct Authority {
    policy: Arc<dyn PortalPolicy>,
    source: Arc<dyn PortalSource>,
    handler: Arc<dyn PortalPayloadHandler>,
    fence: PortalExecutionFence,
}
impl PortalFileAuthority for Authority {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        self.policy.namespace(id)
    }
    fn source_generation(&self, request: &PortalRequest) -> Option<u64> {
        self.source.generation(request)
    }
    fn now_msec(&self) -> u64 {
        self.fence.now_msec()
    }
    fn live(&self) -> bool {
        self.fence.live()
    }
    fn revision(&self) -> u64 {
        self.fence.revision()
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        self.handler.supports(request.transfer.kind) && self.policy.allow(request)
    }
    fn payload_handler(&self) -> &dyn PortalPayloadHandler {
        &*self.handler
    }
}

struct Work {
    transfer: PortalTransfer,
    revision: u64,
    completion: SyncSender<Result<(), PortalTransferFailure>>,
}
pub struct PortalTransferCoordinator {
    sender: SyncSender<Work>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    recipient: Arc<dyn PortalRecipient>,
    fence: PortalExecutionFence,
}
impl PortalTransferCoordinator {
    /// Wiring is supplied by the trusted owner, never selected by a request.
    pub fn start(
        policy: Arc<dyn PortalPolicy>,
        source: Arc<dyn PortalSource>,
        recipient: Arc<dyn PortalRecipient>,
        handler: Arc<dyn PortalPayloadHandler>,
        fence: PortalExecutionFence,
        limits: PortalFileLimits,
    ) -> std::io::Result<Self> {
        let authority = Authority {
            policy: policy.clone(),
            source: source.clone(),
            handler,
            fence: fence.clone(),
        };
        let effect = recipient.clone();
        let guard = fence.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let lease_stop = stop.clone();
        let lease_source = source.clone();
        let service = SessionPortalFileService::spawn_with_stop(
            authority,
            limits,
            stop.clone(),
            move |work| {
                let accepted = Arc::new(AtomicBool::new(false));
                let lease = PortalAccessLease {
                    request: work.request.clone(),
                    grant: work.grant.clone(),
                    revision: work.authority_revision,
                    fence: guard.clone(),
                    source: lease_source.clone(),
                    policy: policy.clone(),
                    stopped: lease_stop.clone(),
                    accepted: accepted.clone(),
                };
                guard
                    .execute(work.authority_revision, work.grant.deadline_msec, || {
                        if !lease.current() {
                            return false;
                        }
                        let result = effect.execute(&work, lease);
                        accepted.store(result, Ordering::Release);
                        result
                    })
                    .unwrap_or(false)
            },
        )?;
        let (sender, requests) = mpsc::sync_channel::<Work>(1);
        let stopping = stop.clone();
        let worker_fence = fence.clone();
        let worker_recipient = recipient.clone();
        let thread = std::thread::Builder::new()
            .name("portal-transfer".into())
            .spawn(move || {
                while !stopping.load(Ordering::Acquire) {
                    let work = match requests.recv_timeout(Duration::from_millis(10)) {
                        Ok(work) => work,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    let transfer = work.transfer.transfer;
                    let result = (|| {
                        let wait = PortalCollection {
                            deadline: Instant::now() + Duration::from_secs(2),
                            revision: work.revision,
                            fence: worker_fence.clone(),
                            stop: stopping.clone(),
                        };
                        wait.remaining()?;
                        let request = PortalRequest {
                            transfer: work.transfer,
                            deadline_msec: worker_fence
                                .now_msec()
                                .checked_add(2000)
                                .ok_or(PortalTransferFailure::Cancelled)?,
                        };
                        let connection = service
                            .connect(
                                request.transfer.source_namespace,
                                request.transfer.target_namespace,
                                work.revision,
                            )
                            .map_err(|_| PortalTransferFailure::Transport)?;
                        let mut client = PortalFileClient::over(
                            connection.stream,
                            connection.epoch,
                            wait.remaining()?,
                        )
                        .map_err(|_| PortalTransferFailure::Transport)?;
                        let PortalBrokerResponseDecision::Allowed(grant) = client
                            .request(request.clone())
                            .map_err(|_| PortalTransferFailure::Transport)?
                        else {
                            return Err(PortalTransferFailure::Denied);
                        };
                        let payload = source.collect(&request, &grant, &wait)?;
                        wait.remaining()?;
                        if client
                            .upload(&payload)
                            .map_err(|_| PortalTransferFailure::Transport)?
                            != PortalGrantState::Completed
                        {
                            return Err(PortalTransferFailure::Recipient);
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        worker_recipient.failed(transfer, error);
                    }
                    let _ = work.completion.try_send(result);
                }
                while let Ok(work) = requests.try_recv() {
                    worker_recipient
                        .failed(work.transfer.transfer, PortalTransferFailure::Cancelled);
                    let _ = work
                        .completion
                        .try_send(Err(PortalTransferFailure::Cancelled));
                }
            })?;
        Ok(Self {
            sender,
            stop,
            thread: Some(thread),
            recipient,
            fence,
        })
    }
    /// Nonblocking admission. Completion means the requester received the
    /// terminal outcome through portal/files, not merely an executor callback.
    pub fn enqueue(
        &self,
        transfer: PortalTransfer,
    ) -> mpsc::Receiver<Result<(), PortalTransferFailure>> {
        let (completion, result) = mpsc::sync_channel(1);
        if let Err(error) = self.sender.try_send(Work {
            transfer,
            revision: self.fence.revision(),
            completion,
        }) {
            let work = match error {
                mpsc::TrySendError::Full(w) | mpsc::TrySendError::Disconnected(w) => w,
            };
            self.recipient
                .failed(work.transfer.transfer, PortalTransferFailure::Pressure);
            let _ = work
                .completion
                .try_send(Err(PortalTransferFailure::Pressure));
        }
        result
    }
}
impl Drop for PortalTransferCoordinator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

//! The in-process X requester uses the real portal files service. Policy sees
//! metadata; source collection and property effects stay with the X recipient.
use crate::{
    ClipboardSelectionExecutionError as Failure, ClipboardSelectionExecutionOutcome,
    ClipboardSourcePayload, XServerFrontendClipboardExecutor,
};
use sophia_portal::{
    execution_fence::PortalExecutionFence, file_client::PortalFileClient,
    file_service::SessionPortalFileService, files::PortalFileAuthority,
};
use sophia_protocol::{
    NamespaceContext, NamespaceId, PortalBrokerResponseDecision, PortalGrantState, PortalRequest,
    PortalTransfer, portal_files::PortalFileLimits,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::time::{Duration, Instant};

pub trait XClipboardPortalPolicy: Send + Sync {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext>;
    fn allow(&self, request: &PortalRequest) -> bool;
}
#[derive(Clone)]
pub struct XClipboardPortal {
    pub policy: Arc<dyn XClipboardPortalPolicy>,
    pub fence: PortalExecutionFence,
    pub broker_generation: u64,
}
struct Authority {
    settings: XClipboardPortal,
    executor: XServerFrontendClipboardExecutor,
}
impl PortalFileAuthority for Authority {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        self.settings.policy.namespace(id)
    }
    fn source_generation(&self, request: &PortalRequest) -> Option<u64> {
        self.executor.source_generation(request)
    }
    fn now_msec(&self) -> u64 {
        self.settings.fence.now_msec()
    }
    fn live(&self) -> bool {
        self.settings.fence.live()
    }
    fn revision(&self) -> u64 {
        self.settings.fence.revision()
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        self.settings.policy.allow(request)
    }
}
struct Work {
    transfer: PortalTransfer,
    revision: u64,
}
pub(crate) struct ClipboardFileCoordinator {
    sender: SyncSender<Work>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    executor: XServerFrontendClipboardExecutor,
    fence: PortalExecutionFence,
}
impl ClipboardFileCoordinator {
    pub(crate) fn start(
        settings: XClipboardPortal,
        executor: XServerFrontendClipboardExecutor,
        payloads: Receiver<ClipboardSourcePayload>,
    ) -> std::io::Result<Self> {
        let authority = Authority {
            settings: settings.clone(),
            executor: executor.clone(),
        };
        let effect = executor.clone();
        let guard = settings.clone();
        let service = SessionPortalFileService::spawn(
            authority,
            PortalFileLimits::defaults(settings.broker_generation),
            move |work| {
                guard
                    .fence
                    .execute(work.authority_revision, work.grant.deadline_msec, || {
                        let grant = &work.grant;
                        let permitted =
                            guard
                                .policy
                                .namespace(grant.source_namespace)
                                .is_some_and(|n| {
                                    n.capabilities.allows_publish(grant.kind.capability())
                                })
                                && guard.policy.namespace(grant.target_namespace).is_some_and(
                                    |n| n.capabilities.allows_request(grant.kind.capability()),
                                );
                        permitted
                            && matches!(
                                effect.execute_portal(grant, &work.payload),
                                Ok(ClipboardSelectionExecutionOutcome::Handoff(_))
                            )
                    })
                    .unwrap_or(false)
            },
        )?;
        let (sender, requests) = mpsc::sync_channel::<Work>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = executor.clone();
        let fence = settings.fence.clone();
        let thread = std::thread::Builder::new()
            .name("x-clipboard-files".into())
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let work = match requests.recv_timeout(Duration::from_millis(10)) {
                        Ok(work) => work,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => break,
                    };
                    let transfer = work.transfer.transfer;
                    let result = (|| -> Result<(), ()> {
                        let deadline = Instant::now() + Duration::from_secs(2);
                        let request = PortalRequest {
                            transfer: work.transfer,
                            deadline_msec: fence.now_msec().checked_add(2000).ok_or(())?,
                        };
                        let targets = request.transfer.mime_type.as_deref() == Some("TARGETS");
                        let connection = service
                            .connect(
                                request.transfer.source_namespace,
                                request.transfer.target_namespace,
                                work.revision,
                            )
                            .map_err(|_| ())?;
                        let mut client = PortalFileClient::over(
                            connection.stream,
                            connection.epoch,
                            deadline.saturating_duration_since(Instant::now()),
                        )
                        .map_err(|_| ())?;
                        let PortalBrokerResponseDecision::Allowed(grant) =
                            client.request(request).map_err(|_| ())?
                        else {
                            return Err(());
                        };
                        let payload = if targets {
                            Vec::new()
                        } else {
                            fence
                                .execute(work.revision, grant.deadline_msec, || {
                                    worker.request_source(&grant)
                                })
                                .ok_or(())?
                                .map_err(|_| ())?;
                            loop {
                                if stopped.load(Ordering::Acquire)
                                    || !fence.live()
                                    || fence.revision() != work.revision
                                {
                                    return Err(());
                                }
                                let remaining = deadline.saturating_duration_since(Instant::now());
                                if remaining.is_zero() {
                                    return Err(());
                                }
                                match payloads
                                    .recv_timeout(remaining.min(Duration::from_millis(10)))
                                {
                                    Ok(payload) if payload.transfer == transfer => {
                                        break payload.bytes;
                                    }
                                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                                    Err(_) => return Err(()),
                                }
                            }
                        };
                        if client.upload(&payload).map_err(|_| ())? != PortalGrantState::Completed {
                            return Err(());
                        }
                        Ok(())
                    })();
                    if result.is_err() {
                        let _ = worker.fail(transfer, Failure::Denied);
                    }
                }
                while let Ok(work) = requests.try_recv() {
                    let _ = worker.fail(work.transfer.transfer, Failure::Disconnected);
                }
            })?;
        Ok(Self {
            sender,
            stop,
            thread: Some(thread),
            executor,
            fence: settings.fence,
        })
    }
    pub(crate) fn enqueue(&self, transfer: PortalTransfer) {
        if let Err(error) = self.sender.try_send(Work {
            transfer,
            revision: self.fence.revision(),
        }) {
            let work = match error {
                mpsc::TrySendError::Full(w) | mpsc::TrySendError::Disconnected(w) => w,
            };
            let _ = self.executor.fail(work.transfer.transfer, Failure::Denied);
        }
    }
}
impl Drop for ClipboardFileCoordinator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.fence.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

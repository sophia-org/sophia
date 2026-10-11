//! X protocol adapters for the shared portal transfer owner.
use crate::{
    ClipboardSelectionExecutionError as Failure, ClipboardSelectionExecutionOutcome,
    ClipboardSourcePayload, XServerFrontendClipboardExecutor,
};
use sophia_portal::{
    coordinator::{
        PortalAccessLease, PortalCollection, PortalRecipient, PortalSource,
        PortalTransferCoordinator, PortalTransferFailure,
    },
    execution_fence::PortalExecutionFence,
    files::PortalFileExecution,
    handlers::ClipboardPayloadHandler,
};
use sophia_protocol::{
    PortalGrant, PortalRequest, PortalTransferId, portal_files::PortalFileLimits,
};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver},
    },
    time::Duration,
};

pub use sophia_portal::coordinator::PortalPolicy as XClipboardPortalPolicy;

#[derive(Clone)]
pub struct XClipboardPortal {
    pub policy: Arc<dyn XClipboardPortalPolicy>,
    pub fence: PortalExecutionFence,
    pub broker_generation: u64,
}

/// Authority-owned source adapter. An explicit selection can feed a recipient
/// in any protocol; no synthetic X recipient window is needed for that path.
pub struct XClipboardSource {
    pub(crate) executor: XServerFrontendClipboardExecutor,
    pub(crate) payloads: Mutex<Receiver<ClipboardSourcePayload>>,
    pub(crate) selection: Option<(sophia_protocol::NamespaceId, crate::XAtom)>,
}
impl PortalSource for XClipboardSource {
    fn generation(&self, request: &PortalRequest) -> Option<u64> {
        if let Some((namespace, selection)) = self.selection {
            if request.transfer.source_namespace != namespace
                || request.transfer.kind != sophia_protocol::PortalTransferKind::Clipboard
            {
                return None;
            }
            return self
                .executor
                .selection_source_generation(namespace, selection);
        }
        self.executor.source_generation(request)
    }
    fn collect(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        if request.transfer.mime_type.as_deref() == Some("TARGETS") {
            return Ok(Vec::new());
        }
        let result = (|| {
            wait.fence
                .execute(wait.revision, grant.deadline_msec, || {
                    if let Some((_, selection)) = self.selection {
                        self.executor.request_selection_source(
                            grant,
                            selection,
                            request.transfer.mime_type.as_deref().unwrap_or(""),
                        )
                    } else {
                        self.executor.request_source(grant)
                    }
                })
                .ok_or(PortalTransferFailure::Cancelled)?
                .map_err(|_| PortalTransferFailure::Source)?;
            let payloads = self
                .payloads
                .lock()
                .map_err(|_| PortalTransferFailure::Source)?;
            loop {
                let remaining = wait.remaining()?;
                match payloads.recv_timeout(remaining.min(Duration::from_millis(10))) {
                    Ok(payload) if payload.transfer == grant.transfer => return Ok(payload.bytes),
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(_) => return Err(PortalTransferFailure::Source),
                }
            }
        })();
        let _ = self.executor.discard_source(grant.transfer);
        result
    }
}
impl PortalRecipient for XServerFrontendClipboardExecutor {
    fn execute(&self, work: &PortalFileExecution, lease: PortalAccessLease) -> bool {
        matches!(
            self.execute_portal(work, &lease),
            Ok(ClipboardSelectionExecutionOutcome::Handoff(_))
        )
    }
    fn failed(&self, transfer: PortalTransferId, reason: PortalTransferFailure) {
        if std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some() {
            tracing::info!(
                "sophia_portal_transfer schema=1 status=failed stage={reason:?} content=redacted"
            );
        }
        let _ = self.fail(transfer, Failure::Denied);
    }
}

pub(crate) struct ClipboardFileCoordinator(PortalTransferCoordinator);
impl ClipboardFileCoordinator {
    pub(crate) fn start(
        settings: XClipboardPortal,
        executor: XServerFrontendClipboardExecutor,
        payloads: Receiver<ClipboardSourcePayload>,
    ) -> std::io::Result<Self> {
        PortalTransferCoordinator::start(
            settings.policy,
            Arc::new(XClipboardSource {
                executor: executor.clone(),
                payloads: Mutex::new(payloads),
                selection: None,
            }),
            Arc::new(executor),
            Arc::new(ClipboardPayloadHandler),
            settings.fence,
            PortalFileLimits::defaults(settings.broker_generation),
        )
        .map(Self)
    }
    pub(crate) fn enqueue(&self, transfer: sophia_protocol::PortalTransfer) {
        self.0.enqueue(transfer);
    }
}

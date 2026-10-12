//! Real common coordinator and files handoff; policy and source bytes supplied.
#![cfg(test)]
use sophia_portal::{
    coordinator::*, execution_fence::PortalExecutionFence, handlers::ClipboardPayloadHandler,
    recipient_object::PortalObjectRecipient,
};
use sophia_protocol::{portal_files::PortalFileLimits, *};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

pub struct Source(pub AtomicU64);
impl PortalSource for Source {
    fn generation(&self, _: &PortalRequest) -> Option<u64> {
        Some(self.0.load(Ordering::Acquire))
    }
    fn collect(
        &self,
        _: &PortalRequest,
        _: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        wait.remaining()?;
        Ok(b"granted".to_vec())
    }
}
struct Policy;
impl PortalPolicy for Policy {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        matches!(id.raw(), 1 | 2).then(|| {
            NamespaceContext::new(id, NamespaceProfile::Confined, NamespaceCapabilities::ALL)
                .unwrap()
        })
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        request.transfer.source_namespace.raw() == 1 && request.transfer.target_namespace.raw() == 2
    }
}
pub struct Fixture {
    pub objects: Arc<PortalObjectRecipient>,
    pub source: Arc<Source>,
    pub fence: PortalExecutionFence,
    pub coordinator: PortalTransferCoordinator,
    source_namespace: NamespaceId,
}
impl Fixture {
    pub fn new() -> Self {
        Self::with_policy(Arc::new(Policy), NamespaceId::from_raw(1))
    }
    pub fn with_policy(policy: Arc<dyn PortalPolicy>, source_namespace: NamespaceId) -> Self {
        let objects = Arc::new(PortalObjectRecipient::new(NamespaceId::from_raw(2)));
        let source = Arc::new(Source(AtomicU64::new(9)));
        let fence = PortalExecutionFence::default();
        let coordinator = PortalTransferCoordinator::start(
            policy,
            source.clone(),
            objects.clone(),
            Arc::new(ClipboardPayloadHandler),
            fence.clone(),
            PortalFileLimits::defaults(1),
        )
        .unwrap();
        Self {
            objects,
            source,
            fence,
            coordinator,
            source_namespace,
        }
    }
    pub fn transfer(&self, id: u64) {
        assert_eq!(
            self.coordinator
                .enqueue(PortalTransfer {
                    transfer: PortalTransferId::from_raw(id),
                    source_namespace: self.source_namespace,
                    target_namespace: NamespaceId::from_raw(2),
                    kind: PortalTransferKind::Clipboard,
                    mime_type: Some("UTF8_STRING".into()),
                    byte_size: 7,
                    decision: PortalDecision::Pending,
                    generation: self.source.0.load(Ordering::Acquire),
                })
                .recv_timeout(Duration::from_secs(3))
                .unwrap(),
            Ok(())
        );
    }
}

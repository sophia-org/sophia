//! Supplied pixels exercise the real CLI/authentication/portal handoff.
//! Native presentation is qualified separately with the render-node runner.
use sophia_portal::{
    capture_frame::CaptureRetentionBudget, capture_requests::CaptureRequestQueue, coordinator::*,
    execution_fence::PortalExecutionFence, handlers::OutputCapturePayloadHandler,
    output_capture_source::OutputCaptureSource, recipient_object::PortalObjectRecipient,
};
use sophia_protocol::{
    output_capture::*, output_capture_request::OutputCaptureState, portal_files::PortalFileLimits,
    *,
};
use std::{sync::Arc, time::Duration};
struct Policy;
impl PortalPolicy for Policy {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        let caps = match id.raw() {
            1 => NamespaceCapabilities::NONE.with_publish(NamespacePortalCapability::ScreenCapture),
            2 => NamespaceCapabilities::NONE.with_request(NamespacePortalCapability::ScreenCapture),
            _ => return None,
        };
        NamespaceContext::new(id, NamespaceProfile::Confined, caps)
    }
    fn allow(&self, request: &PortalRequest) -> bool {
        request.transfer.source_namespace.raw() == 1
            && request.transfer.target_namespace.raw() == 2
            && request.transfer.kind == PortalTransferKind::ScreenCapture
    }
}
pub struct Fixture {
    pub objects: Arc<PortalObjectRecipient>,
    queue: Arc<CaptureRequestQueue>,
    coordinator: PortalTransferCoordinator,
    fence: PortalExecutionFence,
}
impl Fixture {
    pub fn install_supplied_frame(&self) {
        self.queue_request();
        self.service();
    }
    pub fn queue_request(&self) {
        self.queue
            .submit(
                sophia_protocol::output_capture_request::OutputCaptureRequest {
                    serial: 1,
                    output: OutputId::from_raw(7),
                },
            )
            .unwrap();
    }
    pub fn new(width: u32, height: u32) -> Self {
        let fence = PortalExecutionFence::default();
        let queue = Arc::new(CaptureRequestQueue::new(fence.clone()));
        let objects = Arc::new(
            PortalObjectRecipient::new(NamespaceId::from_raw(2))
                .with_capture_requests(queue.clone()),
        );
        let source = Arc::new(OutputCaptureSource::new(OutputId::from_raw(7), 9).unwrap());
        let image = CaptureRetentionBudget::default()
            .reserve(OutputCaptureFrameInfo {
                output: OutputId::from_raw(7),
                output_generation: 9,
                presentation_generation: 11,
                width,
                height,
            })
            .unwrap()
            .complete(vec![17; width as usize * height as usize * 4].into())
            .unwrap();
        assert!(source.publish(Arc::new(image)));
        let coordinator = PortalTransferCoordinator::start(
            Arc::new(Policy),
            source,
            objects.clone(),
            Arc::new(OutputCapturePayloadHandler),
            fence.clone(),
            PortalFileLimits::defaults(1),
        )
        .unwrap();
        Self {
            objects,
            queue,
            coordinator,
            fence,
        }
    }
    pub fn service(&self) {
        if let Some(work) = self.queue.take() {
            assert_eq!(work.request().output, OutputId::from_raw(7));
            self.coordinator
                .enqueue_at_revision(
                    PortalTransfer {
                        transfer: PortalTransferId::from_raw(work.request().serial),
                        source_namespace: NamespaceId::from_raw(1),
                        target_namespace: NamespaceId::from_raw(2),
                        kind: PortalTransferKind::ScreenCapture,
                        mime_type: Some(OUTPUT_CAPTURE_MIME.into()),
                        byte_size: OUTPUT_CAPTURE_RECORD_LEN as u64,
                        decision: PortalDecision::Pending,
                        generation: 9,
                    },
                    work.execution_revision(),
                )
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .unwrap();
            assert!(self.queue.finish(work, OutputCaptureState::Ready));
        }
    }
    pub fn revoke_supplied_frame(&self) {
        self.fence.block();
        self.fence.unblock();
        assert!(self.objects.current().is_err());
    }
    pub fn lock_requests(&self) {
        self.fence.block();
    }
    pub fn cancel_request(&self, active: bool) -> bool {
        if self.queue.status().state != OutputCaptureState::Queued {
            return false;
        }
        let work = active.then(|| self.queue.take().unwrap());
        self.fence.block();
        self.fence.unblock();
        assert_eq!(self.queue.status().state, OutputCaptureState::Cancelled);
        assert!(self.queue.take().is_none());
        if let Some(work) = work {
            assert!(!self.queue.current(work));
            assert!(!self.queue.finish(work, OutputCaptureState::Ready));
        }
        assert!(self.objects.current().is_err());
        true
    }
}

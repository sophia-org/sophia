//! Real coordinator, lease and recipient export; supplied presentation records.
//! Renderer pixels and native completion identity have separate qualification.
use sophia_9p::{ConnectionId, Errno, OpenFlags, export::*};
use sophia_portal::{
    capture_frame::{CaptureRetentionBudget, PortalCaptureFrame},
    client_files::{PortalClientFiles, PortalClientNode as Node},
    coordinator::*,
    execution_fence::PortalExecutionFence,
    handlers::OutputCapturePayloadHandler,
    output_capture_source::OutputCaptureSource,
    recipient_object::PortalObjectRecipient,
};
use sophia_protocol::{output_capture::*, portal_files::PortalFileLimits, *};
use sophia_runtime::authenticated_files::{PreparedFileRoot, ProtectedFiles};
use std::{sync::Arc, time::Duration};

fn transfer(id: u64, generation: u64) -> PortalTransfer {
    PortalTransfer {
        transfer: PortalTransferId::from_raw(id),
        source_namespace: NamespaceId::from_raw(1),
        target_namespace: NamespaceId::from_raw(2),
        kind: PortalTransferKind::ScreenCapture,
        mime_type: Some(OUTPUT_CAPTURE_MIME.into()),
        byte_size: OUTPUT_CAPTURE_RECORD_LEN as u64,
        decision: PortalDecision::Pending,
        generation,
    }
}

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
fn image(output: u64, incarnation: u64, presentation: u64, byte: u8) -> Arc<PortalCaptureFrame> {
    budgeted_image(
        &CaptureRetentionBudget::default(),
        output,
        incarnation,
        presentation,
        byte,
    )
}
fn budgeted_image(
    budget: &CaptureRetentionBudget,
    output: u64,
    incarnation: u64,
    presentation: u64,
    byte: u8,
) -> Arc<PortalCaptureFrame> {
    Arc::new(
        budget
            .reserve(OutputCaptureFrameInfo {
                output: OutputId::from_raw(output),
                output_generation: incarnation,
                presentation_generation: presentation,
                width: 256,
                height: 256,
            })
            .unwrap()
            .complete(vec![byte; 256 * 256 * 4].into())
            .unwrap(),
    )
}
struct View {
    source: Arc<OutputCaptureSource>,
    objects: Arc<PortalObjectRecipient>,
    coordinator: PortalTransferCoordinator,
    files: PortalClientFiles,
}
impl View {
    fn new(output: u64, fence: PortalExecutionFence) -> Self {
        Self::with_budget(output, fence, &CaptureRetentionBudget::default())
    }
    fn with_budget(
        output: u64,
        fence: PortalExecutionFence,
        budget: &CaptureRetentionBudget,
    ) -> Self {
        let source = Arc::new(OutputCaptureSource::new(OutputId::from_raw(output), 9).unwrap());
        assert!(source.publish(budgeted_image(budget, output, 9, 1, 17)));
        let objects = Arc::new(PortalObjectRecipient::new(NamespaceId::from_raw(2)));
        let coordinator = PortalTransferCoordinator::start(
            Arc::new(Policy),
            source.clone(),
            objects.clone(),
            Arc::new(OutputCapturePayloadHandler),
            fence,
            PortalFileLimits::defaults(1),
        )
        .unwrap();
        let mut files =
            PortalClientFiles::new(NamespaceId::from_raw(2), objects.clone(), 7).unwrap();
        files.bind_connection(ConnectionId(1)).unwrap();
        files
            .prepare_authenticated_root(&AttachContext {
                connection: ConnectionId(1),
                peer: None,
                uname: b"observer",
                aname: b"portal-client",
                n_uname: u32::MAX,
            })
            .unwrap()
            .publish();
        Self {
            source,
            objects,
            coordinator,
            files,
        }
    }
    fn transfer(&self, id: u64, generation: u64) -> Result<(), PortalTransferFailure> {
        self.coordinator
            .enqueue(PortalTransfer {
                transfer: PortalTransferId::from_raw(id),
                source_namespace: NamespaceId::from_raw(1),
                target_namespace: NamespaceId::from_raw(2),
                kind: PortalTransferKind::ScreenCapture,
                mime_type: Some(OUTPUT_CAPTURE_MIME.into()),
                byte_size: OUTPUT_CAPTURE_RECORD_LEN as u64,
                decision: PortalDecision::Pending,
                generation,
            })
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
    }
    fn node(&mut self, name: &[u8]) -> Node {
        self.files
            .lookup(&Node::Root, WalkName::Child(name))
            .unwrap()
    }
    fn read(&mut self, node: &Node, count: u32) -> Result<ReadOutcome, Errno> {
        let mut handle = self.files.open(node, OpenFlags(0))?;
        self.files.read(node, &mut handle, 0, count)
    }
}

#[test]
fn shared_budget_follows_source_and_recipient_custody_across_replacement() {
    let bytes = 256 * 256 * 4;
    let budget = CaptureRetentionBudget::new(2 * bytes, 2).unwrap();
    let mut view = View::with_budget(7, PortalExecutionFence::default(), &budget);
    assert_eq!(view.transfer(1, 9), Ok(()));
    let old = view.node(b"frame");
    let next = budgeted_image(&budget, 7, 9, 2, 29);
    let info = next.info();
    assert!(view.source.publish(next));
    assert_eq!(budget.usage().unwrap().bytes, 2 * bytes);
    assert!(budget.reserve(info).is_none());
    assert_eq!(view.read(&old, 4), Ok(ReadOutcome::Ready(vec![17; 4])));

    // Replacing the recipient frees the old image even while its fid exists.
    assert_eq!(view.transfer(2, 9), Ok(()));
    assert_eq!(view.read(&old, 4), Err(Errno::ESTALE));
    assert_eq!(budget.usage().unwrap().bytes, bytes);
    let other = View::with_budget(8, PortalExecutionFence::default(), &budget);
    assert_eq!(budget.usage().unwrap().bytes, 2 * bytes);
    assert!(budget.reserve(info).is_none());
    view.source.withdraw();
    // Revoked pixels still occupy storage until recipient custody releases them.
    assert_eq!(budget.usage().unwrap().bytes, 2 * bytes);
    view.objects.revoke();
    assert_eq!(budget.usage().unwrap().bytes, bytes);
    drop(other);
    assert_eq!(budget.usage().unwrap().bytes, 0);
}

#[test]
fn large_pixels_and_record_share_one_grant_and_never_follow_later_frames() {
    let mut view = View::new(7, PortalExecutionFence::default());
    assert_eq!(view.transfer(1, 9), Ok(()));
    let record = view.node(b"frame-info");
    let pixels = view.node(b"frame");
    let original = image(7, 9, 1, 17);
    assert_eq!(
        view.read(&record, 64),
        Ok(ReadOutcome::Ready(
            original.info().encode().unwrap().to_vec()
        ))
    );
    assert_eq!(view.files.describe(&pixels, None).size, 256 * 256 * 4);
    assert_eq!(
        view.read(&pixels, u32::MAX),
        Ok(ReadOutcome::Ready(vec![17; 65536]))
    );
    let mut handle = view.files.open(&pixels, OpenFlags(0)).unwrap();
    assert_eq!(
        view.files.read(&pixels, &mut handle, 65536, 65536),
        Ok(ReadOutcome::Ready(vec![17; 65536]))
    );
    assert_eq!(
        view.files.read(&pixels, &mut handle, u64::MAX, 1),
        Ok(ReadOutcome::Ready(vec![]))
    );
    assert!(view.files.open(&pixels, OpenFlags(1)).is_err());
    assert!(view.source.publish(image(7, 9, 2, 29)));
    assert_eq!(view.read(&pixels, 4), Ok(ReadOutcome::Ready(vec![17; 4])));
    assert_eq!(view.transfer(2, 9), Ok(()));
    let replacement = view.node(b"frame");
    assert_ne!(pixels, replacement);
    assert_eq!(view.read(&pixels, 4), Err(Errno::ESTALE));
    assert_eq!(view.read(&record, 64), Err(Errno::ESTALE));
    assert_eq!(
        view.read(&replacement, 4),
        Ok(ReadOutcome::Ready(vec![29; 4]))
    );
}

#[test]
fn output_replacement_stales_both_files_without_revoking_another_output() {
    let fence = PortalExecutionFence::default();
    let mut first = View::new(7, fence.clone());
    let mut other = View::new(8, fence);
    assert_eq!(first.transfer(1, 9), Ok(()));
    assert_eq!(other.transfer(1, 9), Ok(()));
    let record = first.node(b"frame-info");
    let pixels = first.node(b"frame");
    let unrelated = other.node(b"frame");
    first.source.withdraw();
    assert!(!first.source.replace(9));
    assert!(!first.source.publish(image(7, 9, 2, 99)));
    assert!(first.source.replace(10));
    assert!(first.source.publish(image(7, 10, 1, 99)));
    assert_eq!(first.read(&pixels, 4), Err(Errno::ESTALE));
    assert_eq!(first.read(&record, 64), Err(Errno::ESTALE));
    assert_eq!(
        other.read(&unrelated, 4),
        Ok(ReadOutcome::Ready(vec![17; 4]))
    );
    assert_eq!(first.transfer(2, 10), Ok(()));
    let fresh = first.node(b"frame");
    assert_eq!(first.read(&fresh, 4), Ok(ReadOutcome::Ready(vec![99; 4])));
    assert_eq!(first.read(&pixels, 4), Err(Errno::ESTALE));
}

#[test]
fn lock_and_binding_revocation_stale_record_and_pixels_without_unlock_revival() {
    for binding in [false, true] {
        let fence = PortalExecutionFence::default();
        let mut view = View::new(7, fence.clone());
        assert_eq!(view.transfer(1, 9), Ok(()));
        let record = view.node(b"frame-info");
        let pixels = view.node(b"frame");
        if binding {
            view.objects.revoke();
        } else {
            fence.block();
        }
        assert_eq!(view.read(&record, 64), Err(Errno::ESTALE));
        assert_eq!(view.read(&pixels, 4), Err(Errno::ESTALE));
        if !binding {
            assert!(view.transfer(2, 9).is_err());
            fence.unblock();
            assert_eq!(view.read(&pixels, 4), Err(Errno::ESTALE));
        }
    }
}

#[test]
fn socket_delivery_rechecks_lock_binding_and_output_after_read() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    for revoke in 0..3 {
        let fence = PortalExecutionFence::default();
        let mut view = View::new(7, fence.clone());
        assert_eq!(view.transfer(1, 9), Ok(()));
        for name in [b"transfer".as_slice(), b"frame-info", b"frame"] {
            let node = view.node(name);
            assert!(view.files.guard_read_delivery(&node));
        }
        let node = view.node(b"frame");
        let ReadOutcome::Ready(bytes) = view.read(&node, 32).unwrap() else {
            panic!("ready image")
        };
        let (mut tx, mut rx) = UnixStream::pair().unwrap();
        tx.set_nonblocking(true).unwrap();
        let access = Access {
            connection: ConnectionId(1),
            epoch: Epoch(7),
            node: &node,
            operation: Operation::Read,
        };
        assert_eq!(
            view.files
                .with_delivery(&access, || tx.write(&bytes))
                .unwrap()
                .unwrap(),
            32
        );
        let mut first = [0; 32];
        rx.read_exact(&mut first).unwrap();
        assert_eq!(first, [17; 32]);
        match revoke {
            0 => {
                fence.block();
            }
            1 => view.objects.revoke(),
            _ => view.source.withdraw(),
        }
        assert_eq!(
            view.files
                .with_delivery(&access, || tx.write(&bytes))
                .unwrap_err(),
            Errno::ESTALE
        );
        drop(tx);
        assert_eq!(rx.read(&mut [0; 1]).unwrap(), 0);
    }
}

#[test]
fn withdrawal_waits_for_an_earlier_read_and_refuses_later_reads() {
    use std::sync::mpsc;
    let source = Arc::new(OutputCaptureSource::new(OutputId::from_raw(7), 9).unwrap());
    assert!(source.publish(image(7, 9, 1, 17)));
    let request = PortalRequest {
        transfer: transfer(1, 9),
        deadline_msec: 2000,
    };
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let reading = source.clone();
    let retained_request = request.clone();
    let reader = std::thread::spawn(move || {
        assert!(reading.with_generation(&retained_request, 9, &mut || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        }));
    });
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let withdrawing = source.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (ended_tx, ended_rx) = mpsc::channel();
    let withdrawer = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        withdrawing.withdraw();
        ended_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(ended_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    reader.join().unwrap();
    ended_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    withdrawer.join().unwrap();
    assert!(!source.with_generation(&request, 9, &mut || panic!("withdrawn pixels delivered")));
}

struct PausedSource {
    inner: OutputCaptureSource,
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}
impl PortalSource for PausedSource {
    fn generation(&self, request: &PortalRequest) -> Option<u64> {
        self.inner.generation(request)
    }
    fn collect(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        let record = self.inner.collect(request, grant, wait)?;
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        Ok(record)
    }
    fn capture_frame(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
    ) -> Option<Arc<PortalCaptureFrame>> {
        self.inner.capture_frame(request, grant)
    }
}

#[test]
fn lock_during_collection_cannot_install_an_inflight_image() {
    use std::sync::{Mutex, mpsc};
    let inner = OutputCaptureSource::new(OutputId::from_raw(7), 9).unwrap();
    assert!(inner.publish(image(7, 9, 1, 17)));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::channel();
    let source = Arc::new(PausedSource {
        inner,
        entered: entered_tx,
        release: Mutex::new(release_rx),
    });
    let objects = Arc::new(PortalObjectRecipient::new(NamespaceId::from_raw(2)));
    let fence = PortalExecutionFence::default();
    let coordinator = PortalTransferCoordinator::start(
        Arc::new(Policy),
        source,
        objects.clone(),
        Arc::new(OutputCapturePayloadHandler),
        fence.clone(),
        PortalFileLimits::defaults(1),
    )
    .unwrap();
    let pending = coordinator.enqueue(transfer(1, 9));
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    fence.block();
    release_tx.send(()).unwrap();
    assert!(
        pending
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .is_err()
    );
    assert_eq!(objects.current(), Err(Errno::ENOENT));
    fence.unblock();
    assert_eq!(objects.current(), Err(Errno::ENOENT));
}

#[test]
fn invalid_or_repeated_presentations_never_replace_current_pixels() {
    let mut view = View::new(7, PortalExecutionFence::default());
    assert!(!view.source.publish(image(8, 9, 2, 88)));
    assert!(!view.source.publish(image(7, 10, 2, 88)));
    assert!(!view.source.publish(image(7, 9, 1, 88)));
    assert_eq!(view.transfer(1, 9), Ok(()));
    let pixels = view.node(b"frame");
    assert_eq!(view.read(&pixels, 4), Ok(ReadOutcome::Ready(vec![17; 4])));
    let info = image(7, 9, 1, 17).info();
    assert!(
        CaptureRetentionBudget::default()
            .reserve(info)
            .unwrap()
            .complete(vec![0; 1].into())
            .is_none()
    );
}

struct WrongImageSource {
    inner: OutputCaptureSource,
    missing: bool,
}
impl PortalSource for WrongImageSource {
    fn generation(&self, request: &PortalRequest) -> Option<u64> {
        self.inner.generation(request)
    }
    fn collect(
        &self,
        request: &PortalRequest,
        grant: &PortalGrant,
        wait: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        self.inner.collect(request, grant, wait)
    }
    fn capture_frame(&self, _: &PortalRequest, _: &PortalGrant) -> Option<Arc<PortalCaptureFrame>> {
        (!self.missing).then(|| image(7, 9, 99, 99))
    }
}

#[test]
fn an_approved_record_without_its_exact_source_image_cannot_bind_pixels() {
    for missing in [true, false] {
        let inner = OutputCaptureSource::new(OutputId::from_raw(7), 9).unwrap();
        assert!(inner.publish(image(7, 9, 1, 17)));
        let source = Arc::new(WrongImageSource { inner, missing });
        let objects = Arc::new(PortalObjectRecipient::new(NamespaceId::from_raw(2)));
        let coordinator = PortalTransferCoordinator::start(
            Arc::new(Policy),
            source,
            objects.clone(),
            Arc::new(OutputCapturePayloadHandler),
            PortalExecutionFence::default(),
            PortalFileLimits::defaults(1),
        )
        .unwrap();
        assert!(
            coordinator
                .enqueue(transfer(1, 9))
                .recv_timeout(Duration::from_secs(3))
                .unwrap()
                .is_err()
        );
        assert_eq!(objects.current(), Err(Errno::ENOENT));
    }
}

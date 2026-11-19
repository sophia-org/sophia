#![cfg(any(
    test,
    all(feature = "test-support", feature = "experimental-combined-frame-job")
))]
//! The worker test core and its reply helpers, beside the worker so that the
//! combined accept stub can use them under test-support. Moved verbatim from
//! renderer_worker.rs, renderer_worker_lifecycle.rs and
//! renderer_worker_correlation.rs; no device.

use super::{LiveRendererWorkerOutputKey, WorkerCommand, WorkerOutcome, WorkerResult};
use sophia_protocol::Size;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::Duration;

pub(super) const SETTLE: Duration = Duration::from_secs(2);

pub(super) fn inventory_test_core(
    capacity: usize,
) -> (
    std::sync::Arc<super::NativeGbmRendererWorkerCore>,
    Receiver<WorkerCommand>,
    SyncSender<()>,
) {
    let control = std::sync::Arc::new(super::WorkerControl::default());
    let registry = std::sync::Arc::new(super::WorkerRegistry::default());
    let identity = super::DeviceIdentity {
        device: 1,
        inode: 901,
        special_device: 226 << 8,
    };
    let (commands, receive) = sync_channel(capacity);
    let (resume, gate) = sync_channel(1);
    let thread = registry
        .reserve(identity)
        .unwrap()
        .start(|| {
            std::thread::Builder::new().spawn(move || {
                let _ = gate.recv_timeout(SETTLE * 4);
            })
        })
        .unwrap();
    let core = std::sync::Arc::new(super::NativeGbmRendererWorkerCore {
        image_store: super::NEXT_IMAGE_STORE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        command_sender: super::wrap_commands(commands, &control),
        _thread: std::sync::Mutex::new(thread),
        control,
        inventory_replacement: std::sync::Mutex::new(None),
        #[cfg(feature = "experimental-shell-capture")]
        capture_install: std::sync::Mutex::new(None),
        #[cfg(feature = "experimental-shell-capture")]
        capture_prepare: std::sync::Mutex::new(None),
        release_enqueue_failures: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    });
    (core, receive, resume)
}

pub(super) fn correlated_result(
    correlation: super::LiveRendererFrameCorrelation,
    outcome: WorkerOutcome,
) -> WorkerResult {
    WorkerResult {
        output: LiveRendererWorkerOutputKey::from_raw(701),
        request_id: correlation.request.unwrap(),
        correlation,
        context_status: super::NativeGbmRenderedScanoutContextStatus::Ready,
        persistent_render_stats: super::LiveNativePersistentRenderStats::default(),
        gl_identity: super::NativeGlIdentityState::Unobserved,
        composition_nonzero_rgb_pixels: 0,
        outcome,
    }
}

pub(super) fn exported_outcome() -> WorkerOutcome {
    let mut slots = super::LiveRendererFrameSlotPool::new();
    let crate::LiveRendererFrameSlotAcquire::Acquired(slot_token) = slots.try_acquire() else {
        panic!("empty slot pool refused a lease");
    };
    WorkerOutcome::Exported {
        capture: None,
        descriptor: super::LiveRendererScanoutBufferDescriptor::new(
            Size {
                width: 16,
                height: 16,
            },
            64,
            sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
            1,
        ),
        lease_id: super::LiveRendererWorkerLeaseId(1),
        slot_token,
    }
}

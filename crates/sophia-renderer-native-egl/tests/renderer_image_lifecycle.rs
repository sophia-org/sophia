#![cfg(all(feature = "gbm-platform", target_os = "linux"))]

use std::{
    fmt,
    fs::OpenOptions,
    os::fd::AsFd,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use sophia_renderer_native_egl::{
    NativeCompositionFrame, NativeCompositionLayer, NativeCompositionRect,
    NativeCompositionSampling, NativeDmaBufPlane, NativeGbmRenderedScanoutContext,
    NativeMultiPlaneDmaBufFrame, NativeRendererImageCompositionLayer, NativeRendererImageId,
    native_dmabuf_cpu_write_access,
};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

struct LifecycleSubscriber {
    events: Arc<Mutex<Vec<String>>>,
    next_span: AtomicU64,
}

#[derive(Default)]
struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

impl Subscriber for LifecycleSubscriber {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(self.next_span.fetch_add(1, Ordering::Relaxed))
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut message = Message::default();
        event.record(&mut message);
        if message.0.contains("sophia_native_lifecycle ") {
            self.events.lock().unwrap().push(message.0);
        }
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE and SOPHIA_LIVE_SESSION_DIAGNOSTIC; creates no windows"]
fn retained_image_surfaces_are_released_before_their_display_terminates() {
    let path = std::env::var_os("SOPHIA_TEST_RENDER_NODE")
        .expect("set SOPHIA_TEST_RENDER_NODE to a DRM render node");
    assert!(
        std::env::var_os("SOPHIA_LIVE_SESSION_DIAGNOSTIC").is_some(),
        "enable existing native lifecycle diagnostics"
    );
    let device = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open render node");
    let allocator =
        gbm::Device::new(device.try_clone().unwrap()).expect("initialize GBM allocator");
    let mut source = allocator
        .create_buffer_object_with_modifiers2::<()>(
            2,
            1,
            gbm::Format::Argb8888,
            std::iter::once(gbm::Modifier::Linear),
            gbm::BufferObjectFlags::RENDERING,
        )
        .expect("allocate source image");
    assert_eq!(source.plane_count(), 1);
    let fd = source.fd_for_plane(0).expect("export source image");
    native_dmabuf_cpu_write_access(&fd, false).expect("begin source write");
    let written = source.map_mut(0, 0, 2, 1, |mapped| {
        mapped.buffer_mut()[..8].copy_from_slice(&[0x21, 0x43, 0x65, 0xff, 0xab, 0xcd, 0xef, 0xff]);
    });
    let ended = native_dmabuf_cpu_write_access(&fd, true);
    written.expect("write source image");
    ended.expect("end source write");

    let events = Arc::new(Mutex::new(Vec::new()));
    let subscriber = LifecycleSubscriber {
        events: events.clone(),
        next_span: AtomicU64::new(1),
    };
    tracing::subscriber::with_default(subscriber, || {
        let created = NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(device));
        let mut context = created
            .context
            .unwrap_or_else(|| panic!("create native context: {:?}", created.status));
        // This witness covers EGLSurface-backed fallback destruction.
        context.set_snapshot_reuse_enabled(false);
        let image_id = NativeRendererImageId::from_raw(71);
        assert!(
            context
                .capture_renderer_image(
                    image_id,
                    NativeMultiPlaneDmaBufFrame {
                        width: 2,
                        height: 1,
                        format: source.format() as u32,
                        modifier: u64::from(source.modifier()),
                        plane_count: 1,
                        planes: [
                            Some(NativeDmaBufPlane {
                                fd: fd.as_fd(),
                                offset: source.offset(0),
                                stride: source.stride_for_plane(0),
                            }),
                            None,
                            None,
                            None,
                        ],
                    },
                )
                .expect("capture retained image")
        );
        let layers = [NativeCompositionLayer::RendererImage(
            NativeRendererImageCompositionLayer {
                image_id,
                target: NativeCompositionRect {
                    x: 0,
                    y: 0,
                    width: 2,
                    height: 1,
                },
                clip: None,
                alpha: 1.0,
                sampling: NativeCompositionSampling::ExactNearest,
            },
        )];
        let rendered = context.export_composed_owned_scanout_buffer_with_modifiers(
            NativeCompositionFrame {
                width: 2,
                height: 1,
                layers: &layers,
                trace: None,
                repaint: None,
            },
            &[0],
        );
        let output = rendered
            .buffer
            .unwrap_or_else(|| panic!("compose retained image: {:?}", rendered.detail));
        let stats = context.persistent_render_stats();
        assert_eq!(stats.snapshot_live_entries, 1);
        assert!(
            stats.import_cache.live_entries > 0,
            "the image must have a live consumer import"
        );
        drop(output);
        // Only resources still owned by the context participate in this assertion.
        events.lock().unwrap().clear();
        drop(context);
    });

    let events = events.lock().unwrap();
    let position = |stage: &str| {
        events
            .iter()
            .position(|event| event.ends_with(&format!("stage={stage}")))
            .unwrap_or_else(|| panic!("missing lifecycle stage {stage}: {events:?}"))
    };
    let context_destroyed = position("egl_context_destroyed");
    let backing_released = position("front_buffer_released");
    let terminated = position("egl_display_terminated");
    assert!(
        context_destroyed < backing_released,
        "consumer context must retire before image backing: {events:?}"
    );
    assert!(
        backing_released < terminated,
        "image backing must retire before display: {events:?}"
    );
    let surface_destroys = events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.ends_with("stage=egl_surface_destroyed"))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert!(
        !surface_destroys.is_empty(),
        "real retained EGL surfaces must exist"
    );
    assert!(
        surface_destroys.iter().all(|index| *index < terminated),
        "no EGL surface may outlive its display: {events:?}"
    );
}

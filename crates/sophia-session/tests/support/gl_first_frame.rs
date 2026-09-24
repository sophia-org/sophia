use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    num::NonZeroUsize,
    os::fd::{AsFd, OwnedFd},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use sophia_protocol::{BufferHandle, DmaBufDescriptor, NamespaceId};
use sophia_renderer_native_egl::{
    NativeCompositionFrame, NativeCompositionLayer, NativeCompositionRect,
    NativeCompositionSampling, NativeDmaBufPlane, NativeGbmRenderedScanoutContext,
    NativeMultiPlaneDmaBufFrame, NativePixmapImportProbe, NativeRendererImageCompositionLayer,
    NativeRendererImageId,
};
use sophia_x_authority::{
    X11CoreTraceObserver, X11SetupSocketError, XServerFrontend, XServerFrontendConfig,
    XServerFrontendRouteBroker,
};

use crate::live_session::x_frontend::{LiveXPixmapAllocator, LiveXRenderDeviceProvider};

#[path = "dri3_layout_probe.rs"]
mod dri3_layout_probe;

const SIZE: u32 = 300;

struct ImportedFrame {
    descriptor: DmaBufDescriptor,
    fds: Vec<OwnedFd>,
}

struct PrivateFrontend {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), X11SetupSocketError>>>,
    socket: PathBuf,
}

impl PrivateFrontend {
    fn finish(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap().unwrap();
    }
}

impl Drop for PrivateFrontend {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = fs::remove_file(&self.socket);
    }
}

struct ProbeChild(Child);

impl ProbeChild {
    fn finish(&mut self) {
        self.0.stdin.take().unwrap().write_all(b"q\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "client cleanup timed out");
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut stdout = String::new();
        let mut stderr = String::new();
        self.0
            .stdout
            .take()
            .unwrap()
            .take(32_768)
            .read_to_string(&mut stdout)
            .unwrap();
        self.0
            .stderr
            .take()
            .unwrap()
            .take(32_768)
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(
            status.success(),
            "client failed: {status}; stdout={stdout}; stderr={stderr}"
        );
        assert!(
            stdout.contains("cleanup=complete"),
            "cleanup was not acknowledged: {stdout}"
        );
        eprintln!("{stdout}{stderr}");
    }
}

impl Drop for ProbeChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

struct ProbeBinary(PathBuf);
impl Drop for ProbeBinary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn open(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

#[test]
#[ignore = "requires SOPHIA_PIXMAP_TEST_DEVICE and GL/EGL/Xlib development files; SOPHIA_FIRST_FRAME_REQUIRE_AUX=1 requires auxiliary-plane coverage; private X server, no KMS"]
fn a_glx_window_submits_its_first_buffer_with_measured_modifiers() {
    first_frame("glx", 51_000);
}

#[test]
#[ignore = "requires SOPHIA_PIXMAP_TEST_DEVICE and GL/EGL/Xlib development files; SOPHIA_FIRST_FRAME_REQUIRE_AUX=1 requires auxiliary-plane coverage; private X server, no KMS"]
fn an_egl_window_submits_its_first_buffer_with_measured_modifiers() {
    first_frame("egl", 62_000);
}

fn first_frame(api: &str, display_base: u32) {
    let node =
        PathBuf::from(std::env::var_os("SOPHIA_PIXMAP_TEST_DEVICE").expect("select a render node"));
    let binary = ProbeBinary(
        std::env::temp_dir().join(format!("sophia-first-frame-{api}-{}", std::process::id())),
    );
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/probes/gl_first_frame.c");
    let compiled = Command::new("cc")
        .args(["-Wall", "-Wextra", "-Werror"])
        .arg(source)
        .arg("-o")
        .arg(&binary.0)
        .args(["-lX11", "-lGL", "-lEGL"])
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "compile client: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let formats = sophia_backend_live::query_dma_buf_import_formats(open(&node)).unwrap();
    assert!(
        !formats.is_empty(),
        "measured capability inventory must be present"
    );
    let display = display_base + std::process::id() % 10_000;
    let socket = PathBuf::from(format!("/tmp/.X11-unix/X{display}"));
    assert!(!socket.exists(), "private display is occupied");
    let config = XServerFrontendConfig::new(&socket, NamespaceId::from_raw(u64::from(display)))
        .unwrap()
        .with_render_device_provider(Arc::new(LiveXRenderDeviceProvider {
            device: open(&node),
            import_formats: formats
                .into_iter()
                .map(
                    |row| sophia_x_authority::XServerFrontendDmaBufImportFormat {
                        format: row.format,
                        modifiers: row.modifiers,
                    },
                )
                .collect(),
        }))
        .with_pixmap_allocator(Arc::new(LiveXPixmapAllocator::new(open(&node))));
    let mut frontend = XServerFrontend::bind(config).unwrap();
    let (submitted, received) = mpsc::sync_channel(1);
    let requests = Arc::new(Mutex::new(VecDeque::new()));
    let observer_requests = requests.clone();
    let imports = Mutex::new(BTreeMap::<BufferHandle, ImportedFrame>::new());
    let published = AtomicBool::new(false);
    let observer: Arc<X11CoreTraceObserver> = Arc::new(move |trace| {
        let mut recent = observer_requests.lock().unwrap();
        if recent.len() == 64 {
            recent.pop_front();
        }
        recent.push_back(format!(
            "seq={} op={}/{} failure={:?} import={:?} present={:?}",
            trace.sequence,
            trace.major_opcode,
            trace.minor_opcode,
            trace.failure,
            trace.dri3_pixmap_import,
            trace.present_submission
        ));
        drop(recent);
        let mut imports = imports.lock().unwrap();
        if trace.failure.is_none()
            && let Some(import) = trace.dri3_pixmap_import
        {
            if imports.len() >= 8 {
                return Err(X11SetupSocketError::new(
                    "first-frame import capacity exceeded",
                ));
            }
            imports.insert(
                import.descriptor.handle,
                ImportedFrame {
                    descriptor: import.descriptor,
                    fds: trace.received_fds,
                },
            );
        }
        if let Some(present) = trace.present_submission
            && !published.swap(true, Ordering::AcqRel)
        {
            let frame = imports.remove(&present.buffer).ok_or_else(|| {
                X11SetupSocketError::new("Present did not name an accepted import")
            })?;
            submitted
                .try_send(frame)
                .map_err(|_| X11SetupSocketError::new("first-frame consumer stopped"))?;
        }
        Ok(None)
    });
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker = std::thread::spawn(move || {
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(16).unwrap());
        while !worker_stop.load(Ordering::Acquire) {
            frontend.try_serve_next_concurrently_routed_traced(&broker, observer.clone())?;
            frontend.poll_client_workers()?;
            std::thread::sleep(Duration::from_millis(1));
        }
        frontend.shutdown_all_client_workers()?;
        frontend.wait_for_clients()
    });
    let mut server = PrivateFrontend {
        stop,
        worker: Some(worker),
        socket,
    };
    let mut child = ProbeChild(
        Command::new(&binary.0)
            .arg(api)
            .env("DISPLAY", format!(":{display}"))
            .env_remove("XAUTHORITY")
            .env_remove("LD_PRELOAD")
            .env_remove("LIBGL_ALWAYS_SOFTWARE")
            .env_remove("LIBGL_ALWAYS_INDIRECT")
            .env_remove("DRI_PRIME")
            .env_remove("MESA_LOADER_DRIVER_OVERRIDE")
            .env_remove("EGL_PLATFORM")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let imported = received
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|error| {
            panic!(
                "{api} first buffer missing: {error}; recent requests: {:?}",
                requests.lock().unwrap()
            )
        });
    eprintln!("first_frame api={api} descriptor={:?}", imported.descriptor);
    let descriptor = imported.descriptor;
    assert_eq!(
        descriptor.size,
        sophia_protocol::Size {
            width: SIZE as i32,
            height: SIZE as i32
        }
    );
    assert_eq!(descriptor.format, sophia_protocol::DRM_FORMAT_XRGB8888);
    let auxiliary_plane = descriptor.plane_count > 1;
    let auxiliary_stride_below_color_row = auxiliary_plane
        && descriptor.planes[1..usize::from(descriptor.plane_count)]
            .iter()
            .flatten()
            .any(|plane| plane.stride < SIZE * 4);
    eprintln!(
        "first_frame api={api} auxiliary_plane={auxiliary_plane} auxiliary_stride_below_color_row={auxiliary_stride_below_color_row}"
    );
    if std::env::var_os("SOPHIA_FIRST_FRAME_REQUIRE_AUX").is_some_and(|value| value == "1") {
        assert!(auxiliary_plane, "fixture must exercise an auxiliary plane");
        assert!(
            auxiliary_stride_below_color_row,
            "fixture must exercise auxiliary stride below the packed color-row bound"
        );
    }
    assert_eq!(imported.fds.len(), usize::from(descriptor.plane_count));

    let report = NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open(&node)));
    let mut renderer = report
        .context
        .unwrap_or_else(|| panic!("renderer startup: {:?}", report.status));
    let id = NativeRendererImageId::from_raw(777);
    assert!(
        renderer
            .capture_renderer_image(
                id,
                NativeMultiPlaneDmaBufFrame {
                    width: SIZE,
                    height: SIZE,
                    format: descriptor.format,
                    modifier: descriptor.modifier,
                    plane_count: descriptor.plane_count,
                    planes: std::array::from_fn(|index| descriptor.planes[index].map(|plane| {
                        NativeDmaBufPlane {
                            fd: imported.fds[index].as_fd(),
                            offset: plane.offset,
                            stride: plane.stride,
                        }
                    })),
                }
            )
            .expect("capture real client buffer")
    );
    assert_eq!(renderer.persistent_render_stats().snapshot_live_entries, 1);
    assert!(renderer.promote_renderer_image(id).unwrap());
    assert_pixels(&read_captured(&mut renderer, &node, id));
    drop(imported);
    child.finish();
    server.finish();
    assert_pixels(&read_captured(&mut renderer, &node, id));
    assert!(renderer.evict_renderer_image(id).unwrap());
    assert_eq!(renderer.persistent_render_stats().snapshot_live_entries, 0);
    eprintln!(
        "first_frame api={api} captured=exact retained_after_client_exit=exact cleanup=complete"
    );
}

fn read_captured(
    renderer: &mut NativeGbmRenderedScanoutContext<File>,
    device: &Path,
    id: NativeRendererImageId,
) -> Vec<u8> {
    let layers = [NativeCompositionLayer::RendererImage(
        NativeRendererImageCompositionLayer {
            image_id: id,
            target: NativeCompositionRect {
                x: 0,
                y: 0,
                width: SIZE as i32,
                height: SIZE as i32,
            },
            clip: None,
            alpha: 1.0,
            sampling: NativeCompositionSampling::ExactNearest,
        },
    )];
    let report = renderer.export_composed_owned_scanout_buffer_with_modifiers(
        NativeCompositionFrame {
            width: SIZE,
            height: SIZE,
            layers: &layers,
            trace: None,
            repaint: None,
        },
        &[0],
    );
    let output = report
        .buffer
        .unwrap_or_else(|| panic!("compose captured image: {:?}", report.detail));
    let fds = output.export_plane_fds().unwrap().into_plane_fds();
    let offsets = output.plane_offsets();
    let strides = output.plane_pitches();
    let reader = NativePixmapImportProbe::new(
        open(device),
        NativeMultiPlaneDmaBufFrame {
            width: SIZE,
            height: SIZE,
            format: output.format(),
            modifier: output.modifier().unwrap(),
            plane_count: output.plane_count(),
            planes: std::array::from_fn(|index| {
                fds[index].as_ref().map(|fd| NativeDmaBufPlane {
                    fd: fd.as_fd(),
                    offset: offsets[index],
                    stride: strides[index],
                })
            }),
        },
    )
    .expect("import composed output");
    reader.read_rgba().expect("read captured pixels")
}

fn assert_pixels(pixels: &[u8]) {
    let colors = [
        [0x21, 0x43, 0x65, 0xff],
        [0xab, 0xcd, 0xef, 0xff],
        [0x11, 0x99, 0xdd, 0xff],
    ];
    assert_eq!(pixels.len(), (SIZE * SIZE * 4) as usize);
    for (index, pixel) in pixels.chunks_exact(4).enumerate() {
        let x = index % SIZE as usize;
        assert_eq!(pixel, colors[x / (SIZE as usize / 3)], "pixel {index}");
    }
}

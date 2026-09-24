use std::{
    fs::OpenOptions,
    num::NonZeroUsize,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use sophia_protocol::NamespaceId;
use sophia_x_authority::{
    XServerFrontend, XServerFrontendConfig, XServerFrontendPixmapAllocator,
    XServerFrontendRouteBroker,
};

use crate::live_session::x_frontend::{LiveXPixmapAllocator, LiveXRenderDeviceProvider};

#[test]
#[ignore = "requires SOPHIA_PIXMAP_TEST_DEVICE and GL/Xlib development files; private X server, no visible windows"]
fn direct_glx_client_reads_live_and_retained_pixmap_exports() {
    let node = std::env::var_os("SOPHIA_PIXMAP_TEST_DEVICE").expect("select a DRM render node");
    let device = || {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&node)
            .unwrap()
    };
    let provider = Arc::new(LiveXPixmapAllocator::new(device()));
    assert!(
        provider.supports_pixmap_textures(),
        "renderer capability probe failed"
    );
    let display = 50000 + std::process::id() % 10000;
    let socket = std::path::PathBuf::from(format!("/tmp/.X11-unix/X{display}"));
    assert!(!socket.exists(), "test display is occupied");
    let config = XServerFrontendConfig::new(&socket, NamespaceId::from_raw(991))
        .unwrap()
        .with_render_device_provider(Arc::new(LiveXRenderDeviceProvider {
            device: device(),
            import_formats: Vec::new(),
        }))
        .with_pixmap_allocator(provider);
    let mut frontend = XServerFrontend::bind(config).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let requests = Arc::new(Mutex::new(std::collections::VecDeque::new()));
    let observed_requests = requests.clone();
    let worker = std::thread::spawn(move || {
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(16).unwrap());
        let observer: Arc<sophia_x_authority::X11CoreTraceObserver> = Arc::new(move |trace| {
            let mut requests = observed_requests.lock().unwrap();
            if requests.len() == 128 {
                requests.pop_front();
            }
            requests.push_back((trace.sequence, trace.major_opcode, trace.minor_opcode));
            Ok(None)
        });
        while !worker_stop.load(Ordering::Acquire) {
            frontend.try_serve_next_concurrently_routed_traced(&broker, observer.clone())?;
            frontend.poll_client_workers()?;
            std::thread::sleep(Duration::from_millis(1));
        }
        frontend.shutdown_all_client_workers()?;
        frontend.wait_for_clients()
    });
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/probes/glx_pixmap.c");
    let binary =
        std::env::temp_dir().join(format!("sophia-glx-pixmap-probe-{}", std::process::id()));
    let compiled = Command::new("cc")
        .args(["-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .args(["-lX11", "-lGL"])
        .status();
    let mut client_result = None;
    if compiled.as_ref().is_ok_and(|status| status.success()) {
        let child = Command::new(&binary)
            .env("DISPLAY", format!(":{display}"))
            .env_remove("XAUTHORITY")
            .env_remove("LD_PRELOAD")
            .env_remove("LIBGL_ALWAYS_SOFTWARE")
            .env_remove("LIBGL_ALWAYS_INDIRECT")
            .env_remove("DRI_PRIME")
            .env_remove("MESA_LOADER_DRIVER_OVERRIDE")
            .spawn();
        if let Ok(mut child) = child {
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    client_result = Some(status);
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
    stop.store(true, Ordering::Release);
    let server_result = worker.join();
    let _ = std::fs::remove_file(&socket);
    let _ = std::fs::remove_file(&binary);
    assert!(compiled.unwrap().success(), "compile GL client");
    server_result.unwrap().unwrap();
    assert!(
        client_result.is_some_and(|status| status.success()),
        "GL client failed or timed out; recent (sequence, major, minor): {:?}",
        requests.lock().unwrap(),
    );
}

#[path = "gl_first_frame.rs"]
mod gl_first_frame;

#[path = "egl_pixmap_export.rs"]
mod egl_pixmap_export;

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
#[ignore = "requires SOPHIA_PIXMAP_TEST_DEVICE and EGL/GLES2/Xlib development files; private X server, no visible windows"]
fn egl_client_binds_cpu_pixmap_rgba_texture() {
    check_pixmap(None, 0);
}

#[test]
#[ignore = "requires SOPHIA_PIXMAP_TEST_DEVICE and EGL/GLES2/Xlib development files; private X server, no visible windows"]
fn egl_client_binds_cpu_pixmap_from_another_connection() {
    check_pixmap(Some("--cross-connection"), 1);
}

#[test]
#[ignore = "requires SOPHIA_PIXMAP_TEST_DEVICE and EGL/GLES2/GBM/XCB/Xlib development files; private X server, no visible windows"]
fn egl_client_binds_imported_pixmap_from_another_connection() {
    check_pixmap(Some("--imported"), 2);
}

#[test]
#[ignore = "requires SOPHIA_PIXMAP_TEST_DEVICE and EGL/GLES2/GBM/XCB/Xlib development files; private X server, no visible windows"]
fn egl_client_binds_imported_pixmap_rgba_texture() {
    check_pixmap(Some("--imported-same-connection"), 3);
}

#[test]
#[ignore = "requires selected render node and GL/EGL/GBM/XCB development files; private X server"]
fn egl_client_binds_imported_pixmap_with_wire_implicit_modifier() {
    check_pixmap(Some("--imported-wire-implicit"), 4);
}

#[test]
#[ignore = "requires selected render node and GL/EGL/GBM/XCB development files; private X server"]
fn glx_client_initializes_legacy_imported_pixmap() {
    check_pixmap(Some("--glx-imported"), 5);
}

#[test]
#[ignore = "requires selected render node and GL/EGL/GBM/XCB development files; private X server"]
fn glx_client_initializes_imported_pixmap_with_wire_implicit_modifier() {
    check_pixmap(Some("--glx-imported-wire-implicit"), 6);
}

#[test]
#[ignore = "requires selected render node and GL/EGL/GBM/XCB development files; private X server"]
fn default_visual_context_binds_depth32_imported_pixmap_with_alpha() {
    check_pixmap(Some("--glx-default-visual-imported"), 7);
}

fn check_pixmap(mode: Option<&str>, case: u32) {
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
    let formats = sophia_backend_live::query_dma_buf_import_formats(device()).unwrap();
    assert!(!formats.is_empty(), "measured import inventory is empty");
    let display = 73000 + case * 10000 + std::process::id() % 10000;
    let socket = std::path::PathBuf::from(format!("/tmp/.X11-unix/X{display}"));
    assert!(!socket.exists(), "test display is occupied");
    let config = XServerFrontendConfig::new(&socket, NamespaceId::from_raw(994))
        .unwrap()
        .with_render_device_provider(Arc::new(LiveXRenderDeviceProvider {
            device: device(),
            import_formats: formats
                .into_iter()
                .map(
                    |entry| sophia_x_authority::XServerFrontendDmaBufImportFormat {
                        format: entry.format,
                        modifiers: entry.modifiers,
                    },
                )
                .collect(),
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
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/probes/egl_pixmap.c");
    let binary = std::env::temp_dir().join(format!(
        "sophia-egl-pixmap-probe-{}-{case}",
        std::process::id()
    ));
    let compiled = Command::new("cc")
        .args(["-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .args([
            "-lX11",
            "-lX11-xcb",
            "-lxcb",
            "-lxcb-dri3",
            "-lgbm",
            "-lEGL",
            "-lGLESv2",
        ])
        .arg("-lGL")
        .status();
    let mut client_result = None;
    if compiled.as_ref().is_ok_and(|status| status.success()) {
        let child = Command::new(&binary)
            .args(mode)
            .env("DISPLAY", format!(":{display}"))
            .env_remove("XAUTHORITY")
            .env_remove("LD_PRELOAD")
            .env_remove("LIBGL_ALWAYS_SOFTWARE")
            .env_remove("LIBGL_ALWAYS_INDIRECT")
            .env_remove("DRI_PRIME")
            .env_remove("MESA_LOADER_DRIVER_OVERRIDE")
            .env_remove("EGL_PLATFORM")
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
    assert!(compiled.unwrap().success(), "compile EGL client");
    server_result.unwrap().unwrap();
    assert!(
        client_result.is_some_and(|status| status.success()),
        "EGL client failed or timed out; recent (sequence, major, minor): {:?}",
        requests.lock().unwrap(),
    );
}

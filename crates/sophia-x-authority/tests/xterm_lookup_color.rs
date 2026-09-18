//! Explicit software-only application check; run only inside a private device-hidden namespace.
use sophia_protocol::{NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile};
use sophia_x_authority::{XServerFrontend, XServerFrontendConfig, XServerFrontendRouteBroker};
use std::{
    fs,
    num::NonZeroUsize,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires private /tmp, /dev, passwd and /usr/bin/xterm; no operator display"]
fn private_xterm_survives_color_lookup_and_exits_normally() {
    assert_eq!(
        std::env::var("SOPHIA_PRIVATE_XTERM_TEST").as_deref(),
        Ok("1")
    );
    assert!(!std::path::Path::new("/dev/dri").exists());
    assert!(!std::path::Path::new("/dev/input").exists());
    let directory = std::env::temp_dir().join(format!("xterm-color-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let marker = directory.join("shell-started");
    let shell = directory.join("shell");
    fs::write(
        &shell,
        format!(
            "#!/bin/sh\nprintf ready > '{}'\nprintf '\\033[31mcolor lookup\\033[0m\\n'\nsleep 2\nexit 0\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&shell, fs::Permissions::from_mode(0o700)).unwrap();
    let resources = directory.join("resources");
    // Explicit named ANSI color avoids depending on the host's X app-defaults.
    fs::write(
        &resources,
        "XTerm*VT100*colorMode: true\nXTerm*VT100*color1: red\n",
    )
    .unwrap();
    fs::create_dir_all("/tmp/.X11-unix").unwrap();
    let socket = "/tmp/.X11-unix/X91";
    assert!(!std::path::Path::new(socket).exists());
    let namespace = NamespaceContext::new(
        NamespaceId::from_raw(1),
        NamespaceProfile::ClassicShared,
        NamespaceCapabilities::NONE,
    )
    .unwrap();
    let config = XServerFrontendConfig::new_with_namespace_context(socket, namespace).unwrap();
    let mut frontend = XServerFrontend::bind(config).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let worker = thread::spawn(move || {
        let broker = XServerFrontendRouteBroker::new(NonZeroUsize::new(64).unwrap());
        while !stopping.load(Ordering::Relaxed) {
            frontend
                .try_serve_next_concurrently_routed_traced(&broker, Arc::new(|_| Ok(None)))
                .unwrap();
            thread::sleep(Duration::from_millis(1));
        }
    });
    let stderr = directory.join("stderr");
    // Exact catalog executable/argv; only the controlled login-shell fixture
    // substitutes for operator typing. No Session, WM or native renderer runs.
    let mut child = Command::new("/usr/bin/xterm")
        .env_clear()
        .env("DISPLAY", ":91")
        .env("HOME", &directory)
        .env("SHELL", &shell)
        .env("XENVIRONMENT", &resources)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(fs::File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > Duration::from_secs(10) {
            timed_out = true;
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        thread::sleep(Duration::from_millis(5));
    };
    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    let errors = fs::read_to_string(&stderr).unwrap();
    assert!(!timed_out, "xterm timed out: {errors}");
    assert!(status.success(), "xterm exited {status}: {errors}");
    assert!(
        marker.exists(),
        "terminal never started its shell: {errors}"
    );
    assert!(started.elapsed() >= Duration::from_secs(2));
    assert!(!errors.contains("X Error"), "{errors}");
    fs::remove_dir_all(directory).unwrap();
}

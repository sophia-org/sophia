use sophia_session::application_catalog::{
    ApplicationLaunchCommand, CatalogProcessEnvironment, spawn_catalog_process,
};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CHILD_MARKER: &str = "SOPHIA_INSPECTION_ENV_TEST_CHILD";

struct ChildGuard(Child);

impl ChildGuard {
    fn wait(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "environment child failed: {status}");
                return;
            }
            assert!(Instant::now() < deadline, "environment child timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn catalog_launch_scrubs_inherited_endpoints_and_sets_only_session_values() {
    // Give a separate process contaminated ambient values without changing the
    // concurrent test runner's environment.
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "catalog_environment_child", "--nocapture"])
            .env(CHILD_MARKER, "1")
            .env(
                sophia_runtime::SOPHIA_CONTROL_SOCKET_ENV,
                "/inherited/control.sock",
            )
            .env(
                sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV,
                "/inherited/inspection.sock",
            )
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    child.wait();
}

#[test]
fn catalog_environment_child() {
    if std::env::var_os(CHILD_MARKER).as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    assert_eq!(
        std::env::var(sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV).unwrap(),
        "/inherited/inspection.sock",
    );
    for (control_socket, inspection_socket) in [
        (None, None),
        (Some(Path::new("/session/control.sock")), None),
        (None, Some(Path::new("/session/inspection.sock"))),
        (
            Some(Path::new("/session/control.sock")),
            Some(Path::new("/session/inspection.sock")),
        ),
    ] {
        let command = ApplicationLaunchCommand {
            executable: "/bin/sh".into(),
            arguments: vec![
                "-c".into(),
                r#"test "${SOPHIA_CONTROL_SOCKET-unset}" = "$1" && test "${SOPHIA_WM_INSPECT_SOCKET-unset}" = "$2""#.into(),
                "inspection-environment".into(),
                control_socket.map_or("unset", |path| path.to_str().unwrap()).into(),
                inspection_socket.map_or("unset", |path| path.to_str().unwrap()).into(),
            ],
            working_directory: None,
        };
        let mut child = ChildGuard(
            spawn_catalog_process(
                &command,
                CatalogProcessEnvironment {
                    display: ":no-display-connection",
                    xauthority: Path::new("/nonexistent/xauthority"),
                    control_socket,
                    inspection_socket,
                },
            )
            .unwrap(),
        );
        child.wait();
    }
}

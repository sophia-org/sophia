use sophia_protocol::inspection::*;
use sophia_runtime::inspection::{InspectionService, PublishOutcome};
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Fixture {
    service: InspectionService,
    directory: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let directory = std::env::temp_dir().join(format!(
            "inspection-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut service = InspectionService::bind(&directory).unwrap();
        service.fence(7, None).unwrap();
        let fixture = Self { service, directory };
        fixture.publish(None);
        fixture
    }

    fn publish(&self, event: Option<InspectionEvent>) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match self.service.publisher().publish(view(), event).unwrap() {
                PublishOutcome::Published { .. } | PublishOutcome::Unchanged => return,
                PublishOutcome::Busy { .. } => {
                    assert!(Instant::now() < deadline);
                    std::thread::yield_now();
                }
            }
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sophia"));
        command
            .args(["inspect", "wm", "--socket"])
            .arg(self.service.socket_path())
            .args(args);
        command.env_remove(sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV);
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Service owns its child directory; remove only the test's parent.
        // An open parent directory is harmless until service Drop completes.
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn view() -> InspectionSnapshot {
    let rect = InspectionRect {
        x: -20,
        y: 10,
        width: 800,
        height: 600,
    };
    InspectionSnapshot {
        session_generation: 3,
        wm_epoch: 7,
        scene_generation: 11,
        selected_capabilities: 253951,
        wire: InspectionWire::Files,
        state: InspectionState::Ready,
        outputs: vec![InspectionOutput {
            id: 1,
            generation: 2,
            geometry: rect,
            work_area: rect,
            focus: None,
        }],
        surfaces: Vec::new(),
    }
}

struct Running(Child);
impl Running {
    fn finish(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "CLI exceeded test deadline");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn output(mut command: Command) -> std::process::Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = Running(command.spawn().unwrap());
    // These bounded single-object commands never fill a pipe in this fixture.
    let status = child.finish();
    use std::io::Read;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    std::process::Output {
        status,
        stdout,
        stderr,
    }
}

#[test]
fn cli_lists_and_validates_derived_records_from_the_admitted_service() {
    let fixture = Fixture::new();
    let listed = output(fixture.command(&["ls"]));
    assert!(
        listed.status.success(),
        "{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    for name in ["api", "status", "snapshot", "events"] {
        assert!(
            String::from_utf8_lossy(&listed.stdout)
                .lines()
                .any(|line| line.starts_with(&format!("{name}\t")))
        );
    }
    let stat = output(fixture.command(&["--json", "stat", "/snapshot"]));
    assert!(
        stat.status.success(),
        "{}",
        String::from_utf8_lossy(&stat.stderr)
    );
    assert!(String::from_utf8_lossy(&stat.stdout).contains("\"path\":\"snapshot\""));
    let status = output(fixture.command(&["--json", "status"]));
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(
        decode_inspection_status(&status.stdout).unwrap().wm_epoch,
        7
    );
    let snapshot = output(fixture.command(&["--json", "snapshot"]));
    assert!(
        snapshot.status.success(),
        "{}",
        String::from_utf8_lossy(&snapshot.stderr)
    );
    assert_eq!(
        decode_inspection_snapshot(&snapshot.stdout)
            .unwrap()
            .snapshot,
        view()
    );
    let human = output(fixture.command(&["snapshot"]));
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("not physical presentation"));
}

#[test]
fn cli_discovery_is_explicit_and_cannot_enable_or_mutate_inspection() {
    let fixture = Fixture::new();
    for args in [
        vec!["write", "submit"],
        vec!["stat", "../ack"],
        vec!["--json", "--json", "ls"],
        vec!["--socket", "", "snapshot"],
        vec!["--enable", "status"],
    ] {
        let result = output(fixture.command(&args));
        assert!(!result.status.success(), "accepted {args:?}");
        assert!(result.stdout.is_empty());
    }
    let mut absent = Command::new(env!("CARGO_BIN_EXE_sophia"));
    absent
        .args(["inspect", "wm", "snapshot"])
        .env_remove(sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV);
    let result = output(absent);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let mut explicit = fixture.command(&["snapshot"]);
    explicit.env(
        sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV,
        "/nonexistent/inherited",
    );
    assert!(output(explicit).status.success());
}

#[test]
fn watch_starts_at_the_snapshot_cursor_and_fails_on_replacement() {
    let mut fixture = Fixture::new();
    let mut command = fixture.command(&["--json", "watch"]);
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = Running(command.spawn().unwrap());
    let reader = child.0.stdout.take().unwrap();
    let (sent, received) = std::sync::mpsc::sync_channel(4);
    let thread = std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            if sent.send(line.unwrap()).is_err() {
                break;
            }
        }
    });
    let baseline = received.recv_timeout(Duration::from_secs(6)).unwrap();
    let baseline = decode_inspection_snapshot(baseline.as_bytes()).unwrap();
    fixture.publish(Some(InspectionEvent::ProjectionCommitted));
    let event = received.recv_timeout(Duration::from_secs(6)).unwrap();
    let event = decode_inspection_event(event.as_bytes()).unwrap();
    assert_eq!(event.sequence, baseline.sequence + 1);
    assert_eq!(event.event, InspectionEvent::ProjectionCommitted);
    fixture.service.fence(8, None).unwrap();
    assert!(
        !child.finish().success(),
        "watch silently crossed a new epoch"
    );
    drop(received);
    thread.join().unwrap();
}

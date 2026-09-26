use sophia_session::diagnostics::application::{
    ApplicationCapture, LaunchContext, LaunchSource, escape_bytes,
};
use sophia_session::diagnostics::{Retention, Store};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Fixture {
    root: PathBuf,
    store: Store,
    run: PathBuf,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "application-diagnostics-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Store::open(&root, Retention::default()).unwrap();
        let record = store.begin("test", std::process::id(), "").unwrap();
        Self {
            root,
            store,
            run: record.path,
            id: record.id,
        }
    }
    fn capture(&self) -> ApplicationCapture {
        let capture = ApplicationCapture::start(&self.run).unwrap();
        capture.set_enabled(true);
        capture
    }
    fn records(&self) -> sophia_session::diagnostics::application::ApplicationRecords {
        self.store.application_records(&self.id).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn context(source: LaunchSource) -> LaunchContext {
    LaunchContext {
        source,
        transaction: Some(7),
    }
}
fn command(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", script])
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
fn reap(capture: &ApplicationCapture, child: &mut Child) {
    let ticket = capture.registration(child.id());
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            capture.exited(ticket, status);
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("child blocked");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(Instant::now() < deadline, "capture did not progress");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn exact_binary_stderr_exit_and_private_opt_in_export() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    for source in [
        LaunchSource::Startup,
        LaunchSource::Shortcut,
        LaunchSource::Catalog,
    ] {
        let mut child = capture
            .spawn(
                &mut command(r"printf '\377\033[31mprivate-secret\n' >&2; exit 17"),
                context(source),
            )
            .unwrap();
        reap(&capture, &mut child);
    }
    wait_for(|| {
        fixture
            .records()
            .launches
            .values()
            .filter(|m| m.contains("eof=true") && m.contains("exit_code=17"))
            .count()
            == 3
    });
    drop(capture);
    let records = fixture.records();
    assert_eq!(records.launches.len(), 3);
    for id in 1..=3 {
        let bytes: Vec<u8> = records
            .chunks
            .iter()
            .filter(|(launch, _, _)| *launch == id)
            .flat_map(|(_, _, bytes)| bytes.iter().copied())
            .collect();
        assert_eq!(bytes, b"\xff\x1b[31mprivate-secret\n");
        assert!(!records.launches[&id].contains("private-secret"));
        assert!(records.launches[&id].contains("transaction=7"));
    }
    assert_eq!(escape_bytes(b"\xff\x1b\n"), "\\xff\\x1b\\n");
    assert!(
        !fixture
            .store
            .inspect(&fixture.id, None)
            .unwrap()
            .events
            .join("\n")
            .contains("private-secret")
    );
    let ordinary = fixture.store.keep(&fixture.id).unwrap();
    assert!(fs::read_dir(ordinary).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("application-")
    }));
    let private = fixture
        .store
        .keep_with_application_stderr(&fixture.id, true)
        .unwrap();
    let name = "application-stderr.0.bin";
    assert_eq!(
        fs::read(private.join(name)).unwrap(),
        fs::read(fixture.run.join(name)).unwrap()
    );
    let metadata = fs::metadata(private.join(name)).unwrap();
    assert_eq!(metadata.mode() & 0o777, 0o600);
    use sha2::{Digest, Sha256};
    let hash = format!(
        "{:x}  {name}",
        Sha256::digest(fs::read(private.join(name)).unwrap())
    );
    assert!(
        fs::read_to_string(private.join("SHA256SUMS"))
            .unwrap()
            .contains(&hash)
    );
}

#[test]
fn spawn_failure_and_signal_are_distinct_from_pipe_eof() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    assert!(
        capture
            .spawn(
                &mut Command::new("/does-not-exist-secret"),
                context(LaunchSource::Startup)
            )
            .is_err()
    );
    let mut child = capture
        .spawn(
            &mut command("exec sleep 30"),
            context(LaunchSource::Shortcut),
        )
        .unwrap();
    child.kill().unwrap();
    reap(&capture, &mut child);
    wait_for(|| {
        fixture
            .records()
            .launches
            .get(&2)
            .is_some_and(|v| v.contains("exit_signal=9") && v.contains("eof=true"))
    });
    drop(capture);
    let records = fixture.records();
    assert!(records.launches[&1].contains("spawn=failed"));
    assert!(records.launches[&1].contains("spawn_reason=not_found"));
    assert!(records.launches[&1].contains("exit_observed=false"));
    assert!(records.launches[&2].contains("exit_code=none"));
}

#[test]
fn repeated_old_exit_cannot_settle_a_new_launch() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    let mut old = capture
        .spawn(&mut command("exit 17"), context(LaunchSource::Startup))
        .unwrap();
    let old_ticket = capture.registration(old.id());
    let status = old.wait().unwrap();
    capture.exited(old_ticket, status);
    wait_for(|| {
        fixture
            .records()
            .launches
            .get(&1)
            .is_some_and(|m| m.contains("eof=true"))
    });
    let mut new = capture
        .spawn(
            &mut command("exec sleep 30"),
            context(LaunchSource::Startup),
        )
        .unwrap();
    let new_ticket = capture.registration(new.id());
    assert_ne!(old_ticket, new_ticket);
    capture.exited(old_ticket, status);
    wait_for(|| fixture.records().launches.contains_key(&2));
    assert!(fixture.records().launches[&2].contains("exit_observed=false"));
    new.kill().unwrap();
    let new_status = new.wait().unwrap();
    capture.exited(new_ticket, new_status);
    wait_for(|| fixture.records().launches[&2].contains("exit_signal=9"));
}

#[test]
fn flood_is_drained_after_launch_limit_and_peer_progresses() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    let mut flood = capture
        .spawn(
            &mut command("i=0; while [ $i -lt 24 ]; do head -c 131072 /dev/zero >&2; i=$((i+1)); sleep 0.02; done"),
            context(LaunchSource::Startup),
        )
        .unwrap();
    let mut peer = capture
        .spawn(
            &mut command("printf peer >&2"),
            context(LaunchSource::Catalog),
        )
        .unwrap();
    reap(&capture, &mut peer);
    wait_for(|| {
        fixture
            .records()
            .chunks
            .iter()
            .any(|(id, _, bytes)| *id == 2 && bytes == b"peer")
    });
    reap(&capture, &mut flood);
    wait_for(|| {
        fixture
            .records()
            .launches
            .get(&1)
            .is_some_and(|m| m.contains("eof=true") && m.contains("exit_observed=true"))
    });
    drop(capture);
    let records = fixture.records();
    let bytes: usize = records
        .chunks
        .iter()
        .filter(|(id, _, _)| *id == 1)
        .map(|(_, _, b)| b.len())
        .sum();
    assert!(bytes <= 1024 * 1024);
    assert!(records.launches[&1].contains("read_bytes=3145728"));
    assert!(!records.launches[&1].contains("dropped_bytes=0"));
}

#[test]
fn disabling_drains_existing_stream_and_only_new_launch_reenables_capture() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    let mut cmd = command(
        "printf before >&2; read first; printf hidden >&2; read second; printf still_hidden >&2",
    );
    cmd.stdin(Stdio::piped());
    let mut child = capture
        .spawn(&mut cmd, context(LaunchSource::Shortcut))
        .unwrap();
    wait_for(|| !fixture.records().chunks.is_empty());
    capture.set_enabled(false);
    child.stdin.as_mut().unwrap().write_all(b"go\n").unwrap();
    wait_for(|| fixture.records().launches[&1].contains("read_bytes=12"));
    capture.set_enabled(true);
    child.stdin.as_mut().unwrap().write_all(b"go\n").unwrap();
    reap(&capture, &mut child);
    let mut fresh = capture
        .spawn(
            &mut command("printf fresh >&2"),
            context(LaunchSource::Catalog),
        )
        .unwrap();
    reap(&capture, &mut fresh);
    wait_for(|| {
        fixture
            .records()
            .launches
            .get(&2)
            .is_some_and(|v| v.contains("eof=true"))
    });
    drop(capture);
    let bytes: Vec<_> = fixture
        .records()
        .chunks
        .into_iter()
        .flat_map(|(_, _, b)| b)
        .collect();
    assert_eq!(bytes, b"beforefresh");
}

#[test]
fn descendant_held_pipe_does_not_hold_capture_shutdown() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    let mut child = capture
        .spawn(
            &mut command("sleep 2 >&2 & printf parent >&2"),
            context(LaunchSource::Startup),
        )
        .unwrap();
    reap(&capture, &mut child);
    let start = Instant::now();
    drop(capture);
    assert!(start.elapsed() < Duration::from_millis(750));
    let records = fixture.records();
    assert!(records.launches[&1].contains("exit_observed=true"));
    assert!(records.launches[&1].contains("incomplete=true"));
    assert!(records.launches[&1].contains("eof=false"));
}

#[test]
fn unsafe_storage_cannot_redirect_bytes_or_fail_the_application() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    fs::write(&outside, b"untouched").unwrap();
    symlink(&outside, fixture.run.join("application-stderr.0.bin")).unwrap();
    let capture = fixture.capture();
    let mut child = capture
        .spawn(
            &mut command("printf private >&2; exit 0"),
            context(LaunchSource::Startup),
        )
        .unwrap();
    reap(&capture, &mut child);
    assert!(child.wait().unwrap().success());
    drop(capture);
    assert_eq!(fs::read(outside).unwrap(), b"untouched");
    assert!(
        !fs::read_to_string(fixture.run.join("application-health"))
            .unwrap()
            .contains("storage_errors=0")
    );
    assert!(fixture.store.application_records(&fixture.id).is_err());
}

#[test]
fn blocked_store_has_bounded_queue_and_does_not_block_child_or_logout() {
    let fixture = Fixture::new();
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.run.join("lock"))
        .unwrap();
    lock.lock().unwrap();
    let capture = fixture.capture();
    let drain_started = Instant::now();
    let mut child = capture
        .spawn(
            &mut command("head -c 3145728 /dev/zero >&2"),
            context(LaunchSource::Startup),
        )
        .unwrap();
    reap(&capture, &mut child);
    assert!(
        drain_started.elapsed() < Duration::from_secs(2),
        "an unavailable store must not throttle draining to one chunk per 5ms tick"
    );
    assert!(child.wait().unwrap().success());
    let now = Instant::now();
    drop(capture);
    assert!(now.elapsed() < Duration::from_millis(750));
    drop(lock);
    wait_for(|| fixture.run.join("application-health").exists());
    let size = fs::metadata(fixture.run.join("application-stderr.0.bin"))
        .unwrap()
        .len();
    assert!(
        size <= 1024 * 1024 + 4096,
        "queue plus one storage-owned frame: {size}"
    );
    let health = fs::read_to_string(fixture.run.join("application-health")).unwrap();
    assert!(
        !health.contains("metadata_lost=0"),
        "saturated shutdown must report missing final metadata"
    );
}

#[test]
fn stream_capacity_refusal_still_executes_the_next_application() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    let mut children = Vec::new();
    for _ in 0..64 {
        let mut command = Command::new("/bin/sleep");
        command.arg("30").stderr(Stdio::null());
        children.push(
            capture
                .spawn(&mut command, context(LaunchSource::Startup))
                .unwrap(),
        );
    }
    let mut overflow = capture
        .spawn(&mut command("exit 23"), context(LaunchSource::Catalog))
        .unwrap();
    reap(&capture, &mut overflow);
    assert_eq!(overflow.wait().unwrap().code(), Some(23));
    for mut child in children {
        child.kill().unwrap();
        reap(&capture, &mut child);
    }
    drop(capture);
    assert!(fixture.records().health.contains("refused=1"));
    assert!(fixture.records().launches.len() <= 64);
}

#[test]
fn real_catalog_spawn_uses_the_shared_registration_without_display_io() {
    use sophia_session::application_catalog::{
        ApplicationLaunchCommand, CatalogProcessEnvironment, spawn_catalog_process,
    };
    let fixture = Fixture::new();
    let capture = fixture.capture();
    capture.install().unwrap();
    let command = ApplicationLaunchCommand {
        executable: "/bin/sh".into(),
        arguments: vec!["-c".into(), "printf catalogue >&2".into()],
        working_directory: None,
    };
    let mut child = spawn_catalog_process(
        &command,
        CatalogProcessEnvironment {
            display: ":nonexistent",
            xauthority: std::path::Path::new("/nonexistent"),
            control_socket: None,
            inspection_socket: None,
        },
    )
    .unwrap();
    let ticket = sophia_session::diagnostics::application::registration(child.id());
    let status = child.wait().unwrap();
    sophia_session::diagnostics::application::exited(ticket, status);
    wait_for(|| {
        fixture
            .records()
            .launches
            .get(&1)
            .is_some_and(|m| m.contains("eof=true") && m.contains("exit_observed=true"))
    });
    drop(capture);
    assert!(fixture.records().launches[&1].contains("source=catalog"));
    assert_eq!(fixture.records().chunks[0].2, b"catalogue");
}

#[test]
fn rotation_bounds_total_store_and_retains_the_newest_launch() {
    let fixture = Fixture::new();
    let capture = fixture.capture();
    // Four streams per wave keep collector fairness exercised without a process storm.
    for _ in 0..5 {
        let mut children: Vec<_> = (0..4)
            .map(|_| {
                capture
                    .spawn(
                    &mut command("i=0; while [ $i -lt 8 ]; do head -c 131072 /dev/zero >&2; i=$((i+1)); sleep 0.02; done"),
                        context(LaunchSource::Startup),
                    )
                    .unwrap()
            })
            .collect();
        for child in &mut children {
            reap(&capture, child);
        }
        wait_for(|| {
            fixture
                .records()
                .launches
                .values()
                .all(|m| m.contains("eof=true"))
        });
    }
    let mut last = capture
        .spawn(
            &mut command("printf newest >&2"),
            context(LaunchSource::Catalog),
        )
        .unwrap();
    reap(&capture, &mut last);
    wait_for(|| {
        fixture
            .records()
            .launches
            .get(&21)
            .is_some_and(|m| m.contains("eof=true"))
    });
    drop(capture);
    let size: u64 = fs::read_dir(&fixture.run)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".bin"))
        .map(|e| e.metadata().unwrap().len())
        .sum();
    assert!(size <= 16 * 1024 * 1024);
    let records = fixture.records();
    assert!(!records.health.contains("rotations=0"));
    assert!(
        records
            .chunks
            .iter()
            .any(|(id, _, bytes)| *id == 21 && bytes == b"newest")
    );
}

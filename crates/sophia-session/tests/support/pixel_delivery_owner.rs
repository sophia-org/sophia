//! Real private sockets/storage with supplied activation. Native authentication
//! and the protected inner executable pin are separate qualification gates.
use super::*;
use std::os::unix::{fs::symlink, process::ExitStatusExt};
use std::sync::atomic::AtomicUsize;
use std::time::Duration;

struct Fixture {
    wake: sophia_wake::Wake,
    path: PathBuf,
    store: Arc<CaptureStore>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "capture-delivery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let store = CaptureStore::create(&path).unwrap();
        Self {
            path,
            store,
            wake: sophia_wake::Wake::new().unwrap(),
        }
    }
    fn begin(&self, generation: u64) -> (CaptureLaunch, UnixStream, OwnedFd) {
        let (mut launch, out, err) = self
            .store
            .prepare(
                generation,
                23,
                NamespaceId::from_raw(2),
                OutputId::from_raw(7),
                "connector-A",
                self.wake.notifier(),
            )
            .unwrap();
        self.store
            .attempts
            .lock()
            .unwrap()
            .get_mut(&generation)
            .unwrap()
            .active = true;
        launch.binding = Some(Binding {
            identity: read_process(std::process::id()).unwrap().identity,
            _pidfd: rustix::process::pidfd_open(
                rustix::process::getpid(),
                rustix::process::PidfdFlags::empty(),
            )
            .unwrap(),
        });
        (launch, out.into(), err)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn info() -> OutputCaptureFrameInfo {
    OutputCaptureFrameInfo {
        output: OutputId::from_raw(7),
        output_generation: 3,
        presentation_generation: 5,
        width: 2,
        height: 1,
    }
}
fn wait(launch: &CaptureLaunch) {
    launch.done.recv_timeout(Duration::from_secs(2)).unwrap();
}

#[test]
fn private_capture_needs_server_publication_and_clean_reap() {
    for mode in 0..8 {
        let f = Fixture::new();
        let (mut launch, mut writer, errors) = f.begin(1);
        let pixels = [7u8; 8];
        if mode != 1 {
            let mut server = info();
            if mode == 2 {
                server.presentation_generation += 1;
            }
            let bytes = if mode == 3 { [9u8; 8] } else { pixels };
            f.store.served(
                NamespaceId::from_raw(2),
                if mode == 4 { 2 } else { 1 },
                server,
                &bytes,
            );
            if mode == 5 {
                f.store.served(NamespaceId::from_raw(2), 1, server, &bytes);
            }
        }
        write_frame(&mut writer, 1, info(), &pixels).unwrap();
        if mode == 6 {
            std::fs::File::from(errors)
                .write_all(b"observer error\n")
                .unwrap();
        } else {
            drop(errors);
        }
        launch.reaped(std::process::ExitStatus::from_raw(if mode == 7 {
            256
        } else {
            0
        }));
        launch.finish.take();
        wait(&launch);
        assert_eq!(
            f.path.join("capture-1.json").exists(),
            mode == 0,
            "mode {mode}"
        );
        assert_eq!(
            f.path.join("capture-1.pam").exists(),
            mode == 0,
            "mode {mode}"
        );
        if mode == 0 {
            let record: serde_json::Value =
                serde_json::from_slice(&std::fs::read(f.path.join("capture-1.json")).unwrap())
                    .unwrap();
            assert_eq!(record["binding"], "launch_custody_not_per_message");
            assert_eq!(record["connector"], "connector-A");
            assert_eq!(record["presentation_generation"], 5);
            assert_eq!(record["transaction"], 23);
        } else {
            assert!(f.path.join("capture-1.refused").exists());
        }
    }
}

#[test]
fn private_capture_burns_attempts_and_refuses_existing_paths() {
    let f = Fixture::new();
    assert!(CaptureStore::create(&f.path).is_err());
    let linked = f.path.with_extension("link");
    symlink(&f.path, &linked).unwrap();
    assert!(CaptureStore::create(&linked).is_err());
    std::fs::remove_file(linked).unwrap();
    for generation in 1..=3 {
        let (mut launch, writer, errors) = f.begin(generation);
        assert!(
            f.store
                .prepare(
                    99,
                    1,
                    NamespaceId::from_raw(2),
                    OutputId::from_raw(7),
                    "connector-A",
                    f.wake.notifier()
                )
                .is_err()
        );
        drop(writer);
        drop(errors);
        launch.finish.take();
        wait(&launch);
        assert!(
            f.path
                .join(format!("capture-{generation}.refused"))
                .exists()
        );
    }
    assert!(
        f.store
            .prepare(
                4,
                1,
                NamespaceId::from_raw(2),
                OutputId::from_raw(7),
                "connector-A",
                f.wake.notifier()
            )
            .is_err()
    );
    f.store.write_new("unique", b"original").unwrap();
    assert!(f.store.write_new("unique", b"replacement").is_err());
    assert_eq!(std::fs::read(f.path.join("unique")).unwrap(), b"original");
    symlink("unique", f.path.join("alias")).unwrap();
    assert!(f.store.write_new("alias", b"replacement").is_err());
}

#[test]
fn private_capture_ignores_other_or_inactive_namespaces_and_expires() {
    let f = Fixture::new();
    let (mut launch, writer, errors) = f.begin(1);
    f.store.served(NamespaceId::from_raw(3), 1, info(), &[7; 8]);
    assert!(f.store.attempts.lock().unwrap()[&1].served.is_none());
    f.store.attempts.lock().unwrap().get_mut(&1).unwrap().active = false;
    f.store.served(NamespaceId::from_raw(2), 1, info(), &[7; 8]);
    assert!(f.store.attempts.lock().unwrap()[&1].served.is_none());
    launch.deadline = Instant::now();
    assert!(launch.failed());
    drop(writer);
    drop(errors);
    launch.finish.take();
    wait(&launch);
    assert!(!f.path.join("capture-1.pam").exists());
}

#[test]
fn private_capture_worker_failure_wakes_the_owner_without_other_events() {
    for stderr_failure in [false, true] {
        let f = Fixture::new();
        let (mut launch, mut writer, errors) = f.begin(1);
        f.wake.clear().unwrap();
        if stderr_failure {
            std::fs::File::from(errors).write_all(b"refused").unwrap();
        } else {
            writer.write_all(&[0; 80]).unwrap();
            drop(errors);
        }
        assert!(
            f.wake
                .wait(Some(Instant::now() + Duration::from_secs(2)))
                .unwrap()
        );
        assert!(launch.failed());
        drop(writer);
        launch.finish.take();
        wait(&launch);
        assert!(f.path.join("capture-1.refused").exists());
        assert!(!f.path.join("capture-1.json").exists());
    }
}

#[test]
fn private_capture_publication_never_overwrites_and_requires_the_receipt() {
    for collision in ["pam", "json"] {
        let f = Fixture::new();
        let (mut launch, mut writer, errors) = f.begin(1);
        let name = format!("capture-1.{collision}");
        f.store.write_new(&name, b"existing").unwrap();
        f.store.served(NamespaceId::from_raw(2), 1, info(), &[7; 8]);
        write_frame(&mut writer, 1, info(), &[7; 8]).unwrap();
        drop(errors);
        launch.reaped(std::process::ExitStatus::from_raw(0));
        wait(&launch);
        assert_eq!(std::fs::read(f.path.join(&name)).unwrap(), b"existing");
        assert!(f.path.join("capture-1.refused").exists());
        assert!(f.path.join(format!("{name}.partial")).exists());
    }
}

#[test]
fn private_capture_deadline_wakes_an_idle_owner_with_both_streams_open() {
    let f = Fixture::new();
    let (mut launch, writer, errors) = f.begin(1);
    f.wake.clear().unwrap();
    assert!(
        f.wake
            .wait(Some(launch.deadline + Duration::from_secs(2)))
            .unwrap()
    );
    assert!(launch.failed());
    drop(writer);
    drop(errors);
    launch.finish.take();
    wait(&launch);
    assert!(f.path.join("capture-1.refused").exists());
    assert!(!f.path.join("capture-1.json").exists());
}

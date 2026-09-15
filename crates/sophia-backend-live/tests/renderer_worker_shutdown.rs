#![cfg(all(target_os = "linux", feature = "libdrm-events", feature = "gbm-probe"))]

#[path = "../src/scanout/rendered_scanout/exporter/worker/lifecycle.rs"]
mod lifecycle;

use lifecycle::{DeviceIdentity, OutputClaim, WORKER_CAPACITY, WorkerControl, WorkerRegistry};
use sophia_backend_live::NativeGbmRendererWorkerCore;
use std::fs::File;
use std::io;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(2);

fn device(id: u64) -> DeviceIdentity {
    DeviceIdentity {
        device: 1,
        inode: id,
        special_device: 226 << 8,
    }
}

#[test]
fn a_stalled_worker_close_returns_before_the_driver_does() {
    let registry = Arc::new(WorkerRegistry::default());
    let control = Arc::new(WorkerControl::default());
    let worker_control = Arc::clone(&control);
    let (resume, gate) = mpsc::sync_channel(1);
    let (entered, running) = mpsc::sync_channel(1);
    let worker = registry
        .reserve(device(1))
        .unwrap()
        .start(|| {
            thread::Builder::new().spawn(move || {
                entered.send(()).unwrap();
                gate.recv_timeout(DEADLINE * 4).unwrap();
                assert!(worker_control.is_shutdown());
            })
        })
        .unwrap();
    running.recv_timeout(DEADLINE).unwrap();
    let (closed, close_result) = mpsc::sync_channel(1);
    let closer = thread::spawn(move || {
        control.shutdown();
        drop(worker);
        closed.send(()).unwrap();
    });
    let result = close_result.recv_timeout(DEADLINE);
    // Always release the fake driver, even if a regression made close wait.
    resume.send(()).unwrap();
    closer.join().unwrap();
    assert!(result.is_ok(), "worker close waited for the fake driver");
}

#[test]
fn an_unfinished_predecessor_refuses_only_its_exact_device() {
    let registry = Arc::new(WorkerRegistry::default());
    let (resume, gate) = mpsc::sync_channel(1);
    let worker = registry
        .reserve(device(2))
        .unwrap()
        .start(|| {
            thread::Builder::new().spawn(move || {
                gate.recv_timeout(DEADLINE * 4).unwrap();
            })
        })
        .unwrap();
    drop(worker);
    assert_eq!(
        registry.reserve(device(2)).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    let mut changed_inode = device(2);
    changed_inode.inode += 1;
    assert!(registry.reserve(changed_inode).is_ok());
    let mut changed_device = device(2);
    changed_device.device += 1;
    assert!(registry.reserve(changed_device).is_ok());
    let mut changed_special = device(2);
    changed_special.special_device += 1;
    assert!(registry.reserve(changed_special).is_ok());
    resume.send(()).unwrap();
}

#[test]
fn a_finished_retired_worker_is_reaped_before_replacement() {
    let registry = Arc::new(WorkerRegistry::default());
    let (resume, gate) = mpsc::sync_channel(1);
    let worker = registry
        .reserve(device(3))
        .unwrap()
        .start(|| {
            thread::Builder::new().spawn(move || {
                gate.recv_timeout(DEADLINE * 4).unwrap();
            })
        })
        .unwrap();
    drop(worker);
    assert!(registry.reserve(device(3)).is_err());
    resume.send(()).unwrap();
    let deadline = Instant::now() + DEADLINE;
    loop {
        if registry.reserve(device(3)).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "finished predecessor was not reaped"
        );
        thread::yield_now();
    }
}

#[test]
fn reservations_and_retired_threads_share_the_fixed_capacity() {
    let registry = Arc::clone(WorkerRegistry::shared());
    let (resume, gate) = mpsc::sync_channel(1);
    let retired = registry
        .reserve(device(1))
        .unwrap()
        .start(|| {
            thread::Builder::new().spawn(move || {
                gate.recv_timeout(DEADLINE * 4).unwrap();
            })
        })
        .unwrap();
    drop(retired);
    let reservations: Vec<_> = (2..=WORKER_CAPACITY as u64)
        .map(|id| registry.reserve(device(id)).unwrap())
        .collect();
    assert_eq!(
        registry.reserve(device(100)).err().unwrap().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(reservations);
    assert!(registry.reserve(device(100)).is_ok());
    resume.send(()).unwrap();
}

#[test]
fn a_failed_spawn_releases_its_reserved_capacity() {
    let registry = Arc::new(WorkerRegistry::default());
    let reservations: Vec<_> = (1..WORKER_CAPACITY as u64)
        .map(|id| registry.reserve(device(id)).unwrap())
        .collect();
    let result = registry
        .reserve(device(100))
        .unwrap()
        .start(|| Err(io::Error::other("injected thread creation failure")));
    assert!(result.is_err());
    assert!(registry.reserve(device(101)).is_ok());
    drop(reservations);
}

#[test]
fn malformed_or_missing_devices_are_refused_before_thread_creation() {
    for path in ["/dev/null", "/dev/zero", "/etc/hosts"] {
        let file = File::open(path).unwrap();
        assert_eq!(
            DeviceIdentity::from_device(&file).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            NativeGbmRendererWorkerCore::spawn(Ok(file))
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        NativeGbmRendererWorkerCore::spawn::<File>(Err(io::Error::from(io::ErrorKind::NotFound)))
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn output_detach_is_exact_and_survives_a_full_wakeup_queue() {
    let (wake, receive) = mpsc::sync_channel(1);
    wake.send(()).unwrap();
    let old = Arc::new(OutputClaim::default());
    let replacement = Arc::new(OutputClaim::default());
    let neighbour = Arc::new(OutputClaim::default());
    old.detach();
    assert!(wake.try_send(()).is_err());
    assert!(old.is_detached());
    assert!(!replacement.is_detached());
    assert!(!neighbour.is_detached());
    receive.recv().unwrap();
}

#[test]
fn explicit_join_waits_for_real_payload_destruction_without_blocking_a_neighbor() {
    struct Payload {
        entered: mpsc::SyncSender<()>,
        resume: mpsc::Receiver<()>,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }
    impl Drop for Payload {
        fn drop(&mut self) {
            self.entered.send(()).unwrap();
            self.resume.recv_timeout(DEADLINE * 4).unwrap();
            self.dropped
                .store(true, std::sync::atomic::Ordering::Release);
        }
    }
    let registry = Arc::new(WorkerRegistry::default());
    let (entered, wait) = mpsc::sync_channel(1);
    let (resume, gate) = mpsc::sync_channel(1);
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let payload = Payload {
        entered,
        resume: gate,
        dropped: Arc::clone(&dropped),
    };
    let mut worker = registry
        .reserve(device(17))
        .unwrap()
        .start(|| thread::Builder::new().spawn(move || drop(payload)))
        .unwrap();
    wait.recv_timeout(DEADLINE).unwrap();
    assert!(!worker.poll_join().unwrap());
    assert!(!dropped.load(std::sync::atomic::Ordering::Acquire));
    let mut neighbor = registry
        .reserve(device(18))
        .unwrap()
        .start(|| thread::Builder::new().spawn(|| {}))
        .unwrap();
    let deadline = Instant::now() + DEADLINE;
    while !neighbor.poll_join().unwrap() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(!worker.poll_join().unwrap());
    resume.send(()).unwrap();
    while !worker.poll_join().unwrap() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
    assert!(worker.poll_join().unwrap());
    assert!(neighbor.poll_join().unwrap());
}

#[test]
fn joined_panic_remains_a_failure_on_every_poll() {
    let registry = Arc::new(WorkerRegistry::default());
    let mut worker = registry
        .reserve(device(19))
        .unwrap()
        .start(|| thread::Builder::new().spawn(|| panic!("injected worker panic")))
        .unwrap();
    let deadline = Instant::now() + DEADLINE;
    loop {
        match worker.poll_join() {
            Err(_) => break,
            Ok(false) => {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            Ok(true) => panic!("thread panic was reported as clean completion"),
        }
    }
    assert!(worker.poll_join().is_err());
    assert!(worker.poll_join().is_err());
}

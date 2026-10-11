//! Supplied pending cleanup isolates listener readiness from authentication.
use super::*;
use crate::authenticated_files::FileAuthentication;
use sophia_9p::{
    ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt, export::AttachContext,
};
use sophia_protocol::lock_files::LockFileLimits;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Draining;
impl FileAuthentication for Draining {
    fn claims(&self, _: &AttachContext<'_>) -> Result<(), Errno> {
        Err(Errno::ESTALE)
    }
    fn begin(&mut self, _: Fid, _: Instant) -> Result<Attempt, Errno> {
        Err(Errno::ESTALE)
    }
    fn read(&mut self, _: Attempt, _: u64, _: u32, _: Instant) -> Result<ReadOutcome, Errno> {
        Err(Errno::ESTALE)
    }
    fn commit(&mut self, _: Attempt, _: Instant) -> Result<(), Errno> {
        Err(Errno::ESTALE)
    }
    fn check(&mut self, _: ConnectionId, _: Instant) -> Result<(), Errno> {
        Err(Errno::ESTALE)
    }
    fn release(&mut self, _: Attempt, _: Instant) {}
    fn reset(&mut self, _: Instant) -> Result<(), Errno> {
        Err(Errno::ESTALE)
    }
    fn close(&mut self, _: Instant) {}
    fn poll(&mut self, _: Instant) -> Result<(), Errno> {
        Ok(())
    }
    fn drained(&self) -> bool {
        false
    }
    fn admitted(&self) -> bool {
        false
    }
    fn wait(&self, _: Instant) -> Duration {
        Duration::from_secs(2)
    }
}

#[test]
fn full_authentication_cleanup_does_not_spin_on_a_queued_connector() {
    let path = std::env::temp_dir().join(format!("lock-auth-wait-{}", std::process::id()));
    let mut transport = LockFileTransport::bind_for_supervised_uid(
        &path,
        rustix::process::geteuid().as_raw(),
        1,
        LockFileLimits {
            max_outputs: 1,
            upload_slots: 1,
            max_chords: 0,
            max_width_px: 8,
            max_height_px: 8,
            max_resource_bytes: 256,
            max_live_resources: 2,
            journal_records: 32,
            journal_bytes: 8192,
            assembly_timeout_ms: 2000,
            ack_progress_timeout_ms: 2000,
        },
    )
    .unwrap();
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    transport.retiring = (0..8)
        .map(|_| Box::new(Draining) as Box<dyn FileAuthentication>)
        .collect();
    let peer = std::os::unix::net::UnixStream::connect(transport.socket_path()).unwrap();
    let wake = sophia_wake::Wake::new().unwrap();
    let notify = wake.notifier();
    let signalled = Arc::new(AtomicBool::new(false));
    let sent = signalled.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        sent.store(true, Ordering::Release);
        notify.notify();
    });
    let result = transport.wait_for_work(&wake, true);
    let saw_owner_wake = signalled.load(Ordering::Acquire);
    thread.join().unwrap();
    drop(peer);
    drop(transport);
    let _ = std::fs::remove_dir_all(path);
    result.unwrap();
    assert!(
        saw_owner_wake,
        "full authentication cleanup must wait past queued listener readiness"
    );
}

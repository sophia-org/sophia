#![cfg(test)]
//! Supplied pending cleanup isolates listener readiness from authentication.
use super::*;
use crate::authenticated_files::{FileAuthentication, FileAuthenticationFactory};
use sophia_9p::{
    ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt, export::AttachContext,
};
use sophia_protocol::lock_files::{LockFileLimits, LockObject, LockPhase};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

struct Draining;
impl FileAuthentication for Draining {
    fn admission_context(&self) -> Option<sophia_protocol::ClientAdmissionContext> {
        None
    }
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

fn transport(label: &str) -> LockFileTransport {
    let path = std::env::temp_dir().join(format!("lock-auth-{label}-{}", std::process::id()));
    LockFileTransport::bind_for_supervised_uid(
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
    .unwrap()
}

#[test]
fn full_authentication_cleanup_does_not_spin_on_a_queued_connector() {
    let mut transport = transport("wait");
    let path = transport.socket_path().parent().unwrap().to_owned();
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

struct RefuseAllocation(Arc<std::sync::atomic::AtomicUsize>);
impl FileAuthenticationFactory for RefuseAllocation {
    fn create(
        &mut self,
        _: ConnectionId,
        _: Instant,
        _: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn FileAuthentication>, Errno> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Errno::EAGAIN)
    }
}

#[test]
fn foreign_connectors_cannot_spend_authentication_capacity_while_locked() {
    let mut transport = transport("foreign");
    let path = transport.socket_path().parent().unwrap().to_owned();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    // Supplied assignee isolates the accept filter; no protected-spawn claim.
    transport
        .authorize_supervised_pid(rustix::process::getppid().unwrap().as_raw_pid() as u32)
        .unwrap();
    transport.authentication = Some(Box::new(RefuseAllocation(calls.clone())));
    let lock = LockObject {
        lock_epoch: 3,
        topology_generation: 1,
        phase: LockPhase::Locked,
        allocations: Vec::new(),
    };
    for _ in 0..64 {
        let peer = std::os::unix::net::UnixStream::connect(transport.socket_path()).unwrap();
        assert!(matches!(
            transport.poll_accept(&lock, &[]),
            Err(crate::lock_files::LockFileTransportError::Endpoint(
                crate::PolicyRoleEndpointError::UnauthorizedPeer { .. }
            ))
        ));
        assert!(transport.export().is_none());
        drop(peer);
    }
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "foreign connectors must not reach authentication allocation"
    );
    assert_eq!(
        transport.next_epoch(),
        1,
        "foreign connectors spend no epoch"
    );

    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    transport.authentication = Some(Box::new(RefuseAllocation(calls.clone())));
    let _peer = std::os::unix::net::UnixStream::connect(transport.socket_path()).unwrap();
    assert!(transport.poll_accept(&lock, &[]).is_err());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the assignee reaches the same capacity check after foreign traffic"
    );
    assert!(
        transport.export().is_none(),
        "quota refusal must publish no role"
    );
    drop(transport);
    let _ = std::fs::remove_dir_all(path);
}

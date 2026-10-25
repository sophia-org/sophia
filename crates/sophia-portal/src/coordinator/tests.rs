#![cfg(test)]

use super::*;
use crate::file_service::tests::fixture::Observer;
use crate::handlers::ClipboardPayloadHandler;
use sophia_protocol::{
    NamespaceCapabilities, NamespaceProfile, PortalDecision, PortalTransferKind,
};

const BOUND: Duration = Duration::from_secs(2);
struct Policy;
impl PortalPolicy for Policy {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        Some(
            NamespaceContext::new(id, NamespaceProfile::Confined, NamespaceCapabilities::ALL)
                .unwrap(),
        )
    }
    fn allow(&self, _: &PortalRequest) -> bool {
        true
    }
}
struct Source;
impl PortalSource for Source {
    fn generation(&self, _: &PortalRequest) -> Option<u64> {
        Some(1)
    }
    fn collect(
        &self,
        _: &PortalRequest,
        _: &PortalGrant,
        _: &PortalCollection,
    ) -> Result<Vec<u8>, PortalTransferFailure> {
        panic!("stopped queued work must never collect");
    }
}
#[derive(Default)]
struct Recipient(std::sync::Mutex<Vec<(PortalTransferId, PortalTransferFailure)>>);
impl PortalRecipient for Recipient {
    fn execute(&self, _: &PortalFileExecution, _: PortalAccessLease) -> bool {
        panic!("no execution expected")
    }
    fn failed(&self, id: PortalTransferId, reason: PortalTransferFailure) {
        self.0.lock().unwrap().push((id, reason));
    }
}
fn coordinator() -> (PortalTransferCoordinator, Arc<Recipient>) {
    let recipient = Arc::new(Recipient::default());
    let owner = PortalTransferCoordinator::start(
        Arc::new(Policy),
        Arc::new(Source),
        recipient.clone(),
        Arc::new(ClipboardPayloadHandler),
        PortalExecutionFence::default(),
        PortalFileLimits::defaults(1),
    )
    .unwrap();
    (owner, recipient)
}
fn both_waiting(observer: &Observer) {
    let mut names = std::collections::BTreeSet::new();
    while names.len() != 2 {
        let (name, timeout) = observer.events.recv_timeout(BOUND).unwrap();
        assert_eq!(timeout, None);
        names.insert(name);
    }
}
fn join(owner: PortalTransferCoordinator) {
    let (done, joined) = mpsc::channel();
    std::thread::spawn(move || {
        drop(owner);
        done.send(()).unwrap();
    });
    joined
        .recv_timeout(BOUND)
        .expect("both portal threads must join");
}

#[test]
fn dropping_a_quiescent_coordinator_wakes_and_joins_both_workers() {
    let observer = Observer::install();
    let (owner, recipient) = coordinator();
    both_waiting(&observer);
    assert!(matches!(
        observer.events.recv_timeout(Duration::from_millis(80)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    join(owner);
    assert!(recipient.0.lock().unwrap().is_empty());
}

#[test]
fn unexpected_file_exit_wakes_an_idle_transfer_receiver() {
    let observer = Observer::install();
    let failure = observer.pause("portal-files");
    let (mut owner, recipient) = coordinator();
    both_waiting(&observer);
    failure.entered.recv_timeout(BOUND).unwrap();
    failure.finish(false);
    // Join the worker without invoking coordinator Drop (which would send Stop itself).
    let thread = owner.thread.take().unwrap();
    let (done, joined) = mpsc::channel();
    std::thread::spawn(move || {
        thread.join().unwrap();
        done.send(()).unwrap();
    });
    joined
        .recv_timeout(BOUND)
        .expect("file exit must notify the idle transfer receiver");
    assert!(owner.stop.load(Ordering::Acquire));
    assert!(recipient.0.lock().unwrap().is_empty());
}

#[test]
fn stop_with_a_full_queue_cancels_the_dequeued_transfer_once() {
    let observer = Observer::install();
    let pause = observer.pause("portal-transfer");
    let (owner, recipient) = coordinator();
    both_waiting(&observer);
    pause.entered.recv_timeout(BOUND).unwrap();
    let id = PortalTransferId::from_raw(7);
    let result = owner.enqueue(PortalTransfer {
        transfer: id,
        source_namespace: NamespaceId::from_raw(1),
        target_namespace: NamespaceId::from_raw(2),
        kind: PortalTransferKind::Clipboard,
        mime_type: Some("text/plain".into()),
        byte_size: 1,
        decision: PortalDecision::Pending,
        generation: 1,
    });
    owner.stop.store(true, Ordering::Release);
    assert!(matches!(
        owner.sender.try_send(Command::Stop),
        Err(mpsc::TrySendError::Full(Command::Stop))
    ));
    pause.finish(true);
    assert_eq!(
        result.recv_timeout(BOUND).unwrap(),
        Err(PortalTransferFailure::Cancelled)
    );
    join(owner);
    assert_eq!(
        *recipient.0.lock().unwrap(),
        [(id, PortalTransferFailure::Cancelled)]
    );
}

#[test]
fn stop_commands_in_the_shutdown_drain_are_not_failed_transfers() {
    let observer = Observer::install();
    let pause = observer.pause("portal-transfer");
    let (owner, recipient) = coordinator();
    both_waiting(&observer);
    pause.entered.recv_timeout(BOUND).unwrap();
    owner.stop.store(true, Ordering::Release);
    owner
        .sender
        .try_send(Command::Stop)
        .unwrap_or_else(|_| panic!("empty queue"));
    pause.finish(false);
    join(owner);
    assert!(recipient.0.lock().unwrap().is_empty());
}

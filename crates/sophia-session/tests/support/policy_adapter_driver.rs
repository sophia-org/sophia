//! Scripted semantic peer, exercising the real worker/driver and bounded
//! channels. This is not wire, protected admission or presentation evidence.
#![cfg(test)]

use super::adapter::{PolicyAdapter, PolicyAdapterEvent, PolicyProfileAdmission};
use super::driver::{PolicyReceiveKind, PolicyReceivePermit};
use super::*;
use sophia_protocol::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug, PartialEq)]
enum Trace {
    Admission(u64, Option<PolicyProfileAdmission>),
    Configuration(TransactionId, u64, PolicyProjectionOutcome),
    Cycle(TransactionId, TransactionId, u64, PolicyProjectionRequest),
    Outcome(TransactionId, u64, u64, PolicyProjectionOutcome),
    Operation(TransactionId, u64, PolicyProjectionOutcome),
    Receipt(TransactionId, PolicyPresentationReceipt),
    Disconnected,
}

struct ScriptedAdapter {
    incoming: Receiver<PolicyAdapterEvent>,
    trace: SyncSender<Trace>,
    reject_profile: bool,
    received: Arc<AtomicUsize>,
    permits: Arc<std::sync::Mutex<Vec<(PolicyReceiveKind, bool)>>>,
}
impl PolicyAdapter for ScriptedAdapter {
    fn admit(
        &mut self,
        _: crate::live_session::policy_transport_worker::driver::PolicyAdmissionPermit,
        epoch: u64,
        profile: Option<PolicyProfileAdmission>,
    ) -> Result<(), String> {
        self.trace.send(Trace::Admission(epoch, profile)).unwrap();
        if self.reject_profile {
            Err("profile refused".into())
        } else {
            Ok(())
        }
    }
    fn selected_capabilities(&self) -> u64 {
        7
    }
    fn receive_within(
        &mut self,
        permit: PolicyReceivePermit,
        timeout: Duration,
    ) -> Result<PolicyAdapterEvent, String> {
        assert_eq!(timeout, Duration::from_secs(12));
        let event = self
            .incoming
            .recv_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        self.received.fetch_add(1, Ordering::SeqCst);
        self.permits
            .lock()
            .unwrap()
            .push((permit.kind(), permit.allows(&event)));
        Ok(event)
    }
    fn try_receive(
        &mut self,
        permit: PolicyReceivePermit,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        match self.incoming.try_recv() {
            Ok(event) => {
                self.permits
                    .lock()
                    .unwrap()
                    .push((permit.kind(), permit.allows(&event)));
                Ok(Some(event))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
    fn send(&mut self, command: &PolicyTransportCommand) -> Result<(), String> {
        let trace = match command {
            PolicyTransportCommand::ConfigurationOutcome {
                transaction,
                generation,
                outcome,
            } => Trace::Configuration(*transaction, *generation, *outcome),
            PolicyTransportCommand::Cycle {
                snapshot_transaction,
                request_transaction,
                scene,
                request,
                ..
            } => Trace::Cycle(
                *snapshot_transaction,
                *request_transaction,
                scene.generation,
                request.clone(),
            ),
            PolicyTransportCommand::ProjectionOutcome {
                transaction,
                request_id,
                scene_generation,
                outcome,
                ..
            } => Trace::Outcome(*transaction, *request_id, *scene_generation, *outcome),
            PolicyTransportCommand::SessionOperationOutcome {
                transaction,
                request_id,
                outcome,
            } => Trace::Operation(*transaction, *request_id, *outcome),
            PolicyTransportCommand::PresentationReceipt {
                transaction,
                receipt,
            } => Trace::Receipt(*transaction, *receipt),
            PolicyTransportCommand::Stop => panic!("stop must not reach the adapter"),
        };
        self.trace.send(trace).map_err(|e| e.to_string())
    }
    fn disconnect(&mut self) {
        self.trace.send(Trace::Disconnected).unwrap();
    }
}

struct Harness {
    worker: PolicyTransportWorker,
    incoming: SyncSender<PolicyAdapterEvent>,
    trace: Receiver<Trace>,
    received: Arc<AtomicUsize>,
    permits: Arc<std::sync::Mutex<Vec<(PolicyReceiveKind, bool)>>>,
}
impl Harness {
    fn new(reject: bool) -> Self {
        let (incoming, receiver) = sync_channel(8);
        let (trace, audit) = sync_channel(16);
        let received = Arc::new(AtomicUsize::new(0));
        let permits = Arc::new(std::sync::Mutex::new(Vec::new()));
        let worker = PolicyTransportWorker::spawn(
            ScriptedAdapter {
                incoming: receiver,
                trace,
                reject_profile: reject,
                received: received.clone(),
                permits: permits.clone(),
            },
            9,
            Some(profile()),
        )
        .unwrap();
        let harness = Self {
            worker,
            incoming,
            trace: audit,
            received,
            permits,
        };
        assert_eq!(harness.trace(), Trace::Admission(9, Some(profile())));
        harness
    }
    fn trace(&self) -> Trace {
        self.trace.recv_timeout(Duration::from_secs(2)).unwrap()
    }
    fn event(&self) -> PolicyTransportEvent {
        self.worker.event_timeout(Duration::from_secs(2)).unwrap()
    }
    fn command(&self, mut command: PolicyTransportCommand) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            match self.worker.try_command(command) {
                Ok(()) => break,
                Err(retained) => command = retained,
            }
            assert!(
                std::time::Instant::now() < deadline,
                "driver command queue did not progress"
            );
            std::thread::yield_now();
        }
    }
    fn configure(&self) {
        assert!(matches!(self.event(), PolicyTransportEvent::Negotiated));
        self.incoming
            .send(PolicyAdapterEvent::Configuration {
                transaction: tx(3),
                configuration: configuration(),
            })
            .unwrap();
        assert!(
            matches!(self.event(), PolicyTransportEvent::Configuration { transaction, configuration: c }
            if transaction == tx(3) && c == configuration())
        );
        self.command(PolicyTransportCommand::ConfigurationOutcome {
            transaction: tx(3),
            generation: 4,
            outcome: PolicyProjectionOutcome::Committed,
        });
        assert_eq!(
            self.trace(),
            Trace::Configuration(tx(3), 4, PolicyProjectionOutcome::Committed)
        );
        self.ready();
    }
    fn ready(&self) {
        assert!(matches!(
            self.event(),
            PolicyTransportEvent::ReadyForCycle { capabilities: 7 }
        ));
    }
    fn cycle(&self) {
        self.command(PolicyTransportCommand::Cycle {
            snapshot_transaction: tx(5),
            request_transaction: tx(6),
            scene: Box::new(PolicySceneSnapshot {
                generation: 11,
                active_output: output(),
                outputs: vec![],
                surfaces: vec![],
                session_operations: vec![],
            }),
            actions: vec![],
            classifications: vec![],
            launch_origins: vec![],
            request: request(),
        });
        assert_eq!(self.trace(), Trace::Cycle(tx(5), tx(6), 11, request()));
    }
}
fn tx(value: u64) -> TransactionId {
    TransactionId::from_raw(value)
}
fn output() -> OutputId {
    OutputId::from_raw(2)
}
fn profile() -> PolicyProfileAdmission {
    PolicyProfileAdmission {
        connection_epoch: 9,
        generation: 3,
        digest: [5; 32],
        prepare_transaction: tx(1),
        activate_transaction: tx(2),
    }
}
fn configuration() -> PolicyConfiguration {
    PolicyConfiguration {
        action_lifecycles: Vec::new(),
        connection_epoch: 9,
        generation: 4,
        actions: vec![],
        chrome: WmChromePolicy::default(),
    }
}
fn request() -> PolicyProjectionRequest {
    PolicyProjectionRequest {
        connection_epoch: 9,
        request_id: 8,
        scene_generation: 11,
        policy_generation: 0,
        affected_outputs: vec![output()],
        cause: PolicyRequestCause::SceneChanged,
    }
}
fn dirty() -> PolicyDirtyRequest {
    PolicyDirtyRequest {
        connection_epoch: 9,
        policy_generation: 1,
        affected_outputs: vec![output()],
    }
}
fn proposal() -> PolicyProjectionProposal {
    PolicyProjectionProposal {
        presentation: None,
        output_launch_contexts: vec![],
        launch_contexts: vec![],
        translation_groups: vec![],
        tab_groups: vec![],
        transaction: tx(7),
        connection_epoch: 9,
        request_id: 8,
        base_generation: 11,
        active_output: output(),
        outputs: vec![],
        indicators: vec![],
        output_statuses: vec![],
    }
}

#[test]
fn semantic_adapter_preserves_cycle_operation_receipt_and_stop_order() {
    let h = Harness::new(false);
    h.configure();
    h.incoming.send(PolicyAdapterEvent::Dirty(dirty())).unwrap();
    assert!(matches!(h.event(), PolicyTransportEvent::Dirty(d) if d == dirty()));
    h.cycle();
    h.incoming.send(PolicyAdapterEvent::Dirty(dirty())).unwrap();
    h.incoming
        .send(PolicyAdapterEvent::Projection(Box::new(proposal())))
        .unwrap();
    assert!(matches!(h.event(), PolicyTransportEvent::Dirty(d) if d == dirty()));
    assert!(matches!(h.event(), PolicyTransportEvent::Projection(p) if *p == proposal()));
    assert!(h.worker.try_event().unwrap().is_none());
    h.command(PolicyTransportCommand::ProjectionOutcome {
        transaction: tx(7),
        request_id: 8,
        scene_generation: 11,
        outcome: PolicyProjectionOutcome::Committed,
        expect_session_operation: true,
    });
    assert_eq!(
        h.trace(),
        Trace::Outcome(tx(7), 8, 11, PolicyProjectionOutcome::Committed)
    );
    let operation = PolicySessionOperationRequest {
        connection_epoch: 9,
        request_id: 8,
        operation: 12,
        target: None,
    };
    h.incoming
        .send(PolicyAdapterEvent::SessionOperation {
            transaction: tx(9),
            request: operation,
        })
        .unwrap();
    assert!(
        matches!(h.event(), PolicyTransportEvent::SessionOperation { transaction, request } if transaction == tx(9) && request == operation)
    );
    let receipt = PolicyPresentationReceipt {
        connection_epoch: 9,
        publication_generation: 2,
        output: output(),
        output_generation: 3,
        presentation_epoch: 17,
        outcome: PolicyPresentationOutcome::Revoked,
    };
    h.command(PolicyTransportCommand::PresentationReceipt {
        transaction: tx(10),
        receipt,
    });
    assert_eq!(h.trace(), Trace::Receipt(tx(10), receipt));
    h.command(PolicyTransportCommand::SessionOperationOutcome {
        transaction: tx(9),
        request_id: 8,
        outcome: PolicyProjectionOutcome::Committed,
    });
    assert_eq!(
        h.trace(),
        Trace::Operation(tx(9), 8, PolicyProjectionOutcome::Committed)
    );
    // An invented readiness on receipt delivery would leave two events here.
    h.ready();
    h.command(PolicyTransportCommand::Stop);
    assert_eq!(h.trace(), Trace::Disconnected);
    assert!(matches!(
        h.worker.event_timeout(Duration::from_secs(2)),
        Err(RecvTimeoutError::Disconnected)
    ));
    assert_eq!(
        *h.permits.lock().unwrap(),
        vec![
            (PolicyReceiveKind::Configuration, true),
            (PolicyReceiveKind::DirtyOnly, true),
            (PolicyReceiveKind::Projection { allow_dirty: true }, true),
            (PolicyReceiveKind::Projection { allow_dirty: true }, true),
            (PolicyReceiveKind::SessionOperation, true),
        ]
    );
}

#[test]
fn semantic_adapter_refuses_profile_before_negotiated() {
    let h = Harness::new(true);
    assert!(matches!(h.event(), PolicyTransportEvent::Failed(e) if e == "profile refused"));
    assert_eq!(h.trace(), Trace::Disconnected);
    assert!(matches!(
        h.worker.event_timeout(Duration::from_secs(2)),
        Err(RecvTimeoutError::Disconnected)
    ));
}

#[test]
fn semantic_adapter_refuses_control_records_during_projection() {
    for configuration_record in [false, true] {
        let h = Harness::new(false);
        h.configure();
        h.cycle();
        h.incoming
            .send(if configuration_record {
                PolicyAdapterEvent::Configuration {
                    transaction: tx(3),
                    configuration: configuration(),
                }
            } else {
                PolicyAdapterEvent::SessionOperation {
                    transaction: tx(9),
                    request: PolicySessionOperationRequest {
                        connection_epoch: 9,
                        request_id: 8,
                        operation: 12,
                        target: None,
                    },
                }
            })
            .unwrap();
        let expected = "policy client sent a control message during projection transfer";
        assert!(matches!(h.event(), PolicyTransportEvent::Failed(e) if e == expected));
        assert_eq!(h.trace(), Trace::Disconnected);
    }
}

#[test]
fn semantic_adapter_shutdown_disconnects_its_blocked_producer() {
    let h = Harness::new(false);
    // Negotiated fills the one-slot owner queue. Configuration then blocks
    // the actual driver send, not a stand-in producer thread.
    h.incoming
        .send(PolicyAdapterEvent::Configuration {
            transaction: tx(3),
            configuration: configuration(),
        })
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while h.received.load(Ordering::SeqCst) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "driver never read the configuration"
        );
        std::thread::yield_now();
    }
    let Harness {
        worker,
        incoming,
        trace,
        ..
    } = h;
    let (done, completion) = sync_channel(1);
    std::thread::spawn(move || {
        drop(worker);
        done.send(()).unwrap();
    });
    completion.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(
        trace.recv_timeout(Duration::from_secs(2)).unwrap(),
        Trace::Disconnected
    );
    drop(incoming);
}

#[test]
fn semantic_adapter_refuses_projection_before_configuration_or_cycle() {
    for before_configuration in [true, false] {
        let h = Harness::new(false);
        if before_configuration {
            assert!(matches!(h.event(), PolicyTransportEvent::Negotiated));
        } else {
            h.configure();
        }
        h.incoming
            .send(PolicyAdapterEvent::Projection(Box::new(proposal())))
            .unwrap();
        let expected = if before_configuration {
            "policy client did not configure before its first snapshot"
        } else {
            "policy client sent an out-of-phase control message"
        };
        assert!(matches!(h.event(), PolicyTransportEvent::Failed(error) if error == expected));
        assert_eq!(h.trace(), Trace::Disconnected);
    }
}

/// The real worker rings the Session owner after each event it publishes and
/// after its event queue disconnects, so the owner's idle wait never outlasts
/// policy traffic.
#[test]
fn worker_rings_the_owner_after_each_publication_and_after_disconnect() {
    let owner = crate::live_session::OwnerWake::new().unwrap();
    let Harness {
        worker,
        incoming,
        trace: _trace,
        ..
    } = Harness::new(false);
    // Negotiated may already be queued. Attaching rings once either way, so
    // an event published before the owner attached is not stranded.
    assert!(!worker.owner_wake_attached());
    worker.set_owner_wake(owner.notifier());
    assert!(worker.owner_wake_attached());
    let next = || {
        owner.begin_pass().unwrap();
        let started = std::time::Instant::now();
        let result = owner.receive(&worker.events, Duration::from_secs(30));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the owner waited out its deadline"
        );
        result
    };
    assert!(matches!(next(), Ok(PolicyTransportEvent::Negotiated)));
    incoming
        .send(PolicyAdapterEvent::Configuration {
            transaction: tx(3),
            configuration: configuration(),
        })
        .unwrap();
    assert!(matches!(
        next(),
        Ok(PolicyTransportEvent::Configuration { transaction, .. }) if transaction == tx(3)
    ));
    // A lost peer fails the driver: the failure is published and rung, then
    // the worker's queue disconnects and rings once more.
    drop(incoming);
    assert!(matches!(next(), Ok(PolicyTransportEvent::Failed(_))));
    assert!(matches!(next(), Err(RecvTimeoutError::Disconnected)));
}

#[test]
fn semantic_adapter_rejection_does_not_wait_for_a_session_operation() {
    let h = Harness::new(false);
    h.configure();
    h.cycle();
    h.incoming
        .send(PolicyAdapterEvent::Projection(Box::new(proposal())))
        .unwrap();
    assert!(matches!(h.event(), PolicyTransportEvent::Projection(_)));
    h.command(PolicyTransportCommand::ProjectionOutcome {
        transaction: tx(7),
        request_id: 8,
        scene_generation: 11,
        outcome: PolicyProjectionOutcome::RejectedStale,
        expect_session_operation: true,
    });
    assert_eq!(
        h.trace(),
        Trace::Outcome(tx(7), 8, 11, PolicyProjectionOutcome::RejectedStale)
    );
    h.ready();
    h.command(PolicyTransportCommand::Stop);
    assert_eq!(h.trace(), Trace::Disconnected);
}

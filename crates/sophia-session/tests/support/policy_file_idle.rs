//! Real private driver/reactor with a raw 9P peer. Supplied admission and
//! scripted outcomes only: no WM implementation, reducer or native receipts.
#![cfg(test)]
use super::*;
use crate::live_session::policy_transport_worker::{
    PolicyTransportWorker,
    adapter::{PolicyAdapterCommandWake, PolicyAdapterStop},
    driver::PolicyReceiveKind,
    ninep::runtime_adapter::NinePPolicyAdapter,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

const BOUND: Duration = Duration::from_secs(2);

struct IdleGate {
    entered: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
}

#[derive(Default)]
struct Probe {
    idle_entered: AtomicUsize,
    idle_returned: AtomicUsize,
    fallback: AtomicUsize,
    bells: AtomicUsize,
    active: AtomicUsize,
    sent: Mutex<Vec<u64>>,
    send_idle: Mutex<Vec<(u64, usize)>>,
    before_idle: Mutex<Option<IdleGate>>,
}
struct Bell {
    inner: Box<dyn PolicyAdapterCommandWake>,
    probe: Arc<Probe>,
}
impl PolicyAdapterCommandWake for Bell {
    fn wake(&self) {
        self.probe.bells.fetch_add(1, Ordering::SeqCst);
        self.inner.wake();
    }
}
struct Observed {
    inner: NinePPolicyAdapter,
    probe: Arc<Probe>,
}
impl PolicyAdapter for Observed {
    fn admit(
        &mut self,
        permit: PolicyAdmissionPermit,
        epoch: u64,
        profile: Option<PolicyProfileAdmission>,
    ) -> Result<(), String> {
        self.inner.admit(permit, epoch, profile)
    }
    fn selected_capabilities(&self) -> u64 {
        self.inner.selected_capabilities()
    }
    fn receive_within(
        &mut self,
        permit: PolicyReceivePermit,
        timeout: Duration,
    ) -> Result<PolicyAdapterEvent, String> {
        self.probe.active.fetch_add(1, Ordering::SeqCst);
        self.inner.receive_within(permit, timeout)
    }
    fn try_receive(
        &mut self,
        _: PolicyReceivePermit,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        self.probe.fallback.fetch_add(1, Ordering::SeqCst);
        eprintln!("policy_idle_control status=legacy_fallback_refused");
        Err("file adapter entered legacy polling fallback".into())
    }
    fn send(&mut self, command: &PolicyTransportCommand) -> Result<(), String> {
        let tx = match command {
            PolicyTransportCommand::ConfigurationOutcome { transaction, .. }
            | PolicyTransportCommand::ProjectionOutcome { transaction, .. }
            | PolicyTransportCommand::SessionOperationOutcome { transaction, .. }
            | PolicyTransportCommand::PresentationReceipt { transaction, .. } => transaction.raw(),
            PolicyTransportCommand::Cycle {
                request_transaction,
                ..
            } => request_transaction.raw(),
            PolicyTransportCommand::Stop => panic!("Stop must not reach adapter send"),
        };
        self.inner.send(command)?;
        self.probe.sent.lock().unwrap().push(tx);
        self.probe
            .send_idle
            .lock()
            .unwrap()
            .push((tx, self.probe.idle_entered.load(Ordering::SeqCst)));
        Ok(())
    }
    fn stop_handle(&self) -> Option<Box<dyn PolicyAdapterStop>> {
        self.inner.stop_handle()
    }
    fn command_wake_handle(&self) -> Option<Box<dyn PolicyAdapterCommandWake>> {
        Some(Box::new(Bell {
            inner: self.inner.command_wake_handle().unwrap(),
            probe: self.probe.clone(),
        }))
    }
    fn idle_receive(
        &mut self,
        permit: PolicyReceivePermit,
        cap: Duration,
    ) -> Result<Option<PolicyAdapterEvent>, String> {
        assert_eq!(permit.kind(), PolicyReceiveKind::DirtyOnly);
        assert_eq!(cap, Duration::from_secs(12));
        self.probe.idle_entered.fetch_add(1, Ordering::SeqCst);
        let gate = self.probe.before_idle.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.entered.send(()).unwrap();
            gate.release
                .recv_timeout(BOUND)
                .map_err(|e| e.to_string())?;
        }
        let result = self.inner.idle_receive(permit, cap);
        self.probe.idle_returned.fetch_add(1, Ordering::SeqCst);
        result
    }
    fn disconnect(&mut self) {
        self.inner.disconnect();
    }
}

fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + BOUND;
    while !condition() {
        assert!(Instant::now() < deadline, "bounded ordering observation");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn event(worker: &PolicyTransportWorker) -> PolicyTransportEvent {
    worker.event_timeout(BOUND).unwrap()
}
fn enqueue(worker: &PolicyTransportWorker, mut command: PolicyTransportCommand) {
    let deadline = Instant::now() + BOUND;
    loop {
        match worker.try_command(command) {
            Ok(()) => return,
            Err(value) => command = value,
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn accepted(peer: &mut Peer, bytes: &[u8]) {
    assert_eq!(peer.submit(bytes).unwrap().0, 119);
    let submitted = peer.next_event();
    assert_eq!(
        decode_wm_file_submitted(&submitted).unwrap().submission_id,
        decode_wm_file_record(bytes, WmFileClass::Candidate)
            .unwrap()
            .header
            .submission_id
    );
    peer.ack(&submitted);
    peer.clear_transaction();
}
fn configuration() -> WmFileConfiguration {
    WmFileConfiguration {
        transaction: TransactionId::from_raw(10),
        configuration: PolicyConfiguration {
            connection_epoch: 9,
            generation: 3,
            actions: vec![],
            chrome: WmChromePolicy::default(),
        },
    }
}
fn configured() -> (PolicyTransportWorker, Peer, u64, Arc<Probe>) {
    let caps = sophia_runtime::select_policy_capabilities(u64::MAX, u64::MAX, false);
    let (server, client) = UnixStream::pair().unwrap();
    let probe = Arc::new(Probe::default());
    let inner = NinePPolicyAdapter::supplied(
        server,
        9,
        WmFileLimits {
            capability_ceiling: caps,
            profile_required: false,
        },
        WmQids::new(),
    )
    .unwrap();
    let worker = PolicyTransportWorker::spawn(
        Observed {
            inner,
            probe: probe.clone(),
        },
        9,
        None,
    )
    .unwrap();
    let mut peer = Peer::from_stream(client);
    negotiate(&mut peer, caps);
    assert!(matches!(event(&worker), PolicyTransportEvent::Negotiated));
    accepted(
        &mut peer,
        &encode_wm_file_configuration(header(WmFileKind::Configuration, 2), &configuration(), caps)
            .unwrap(),
    );
    assert!(matches!(
        event(&worker),
        PolicyTransportEvent::Configuration { .. }
    ));
    enqueue(
        &worker,
        PolicyTransportCommand::ConfigurationOutcome {
            transaction: TransactionId::from_raw(10),
            generation: 3,
            outcome: PolicyProjectionOutcome::Committed,
        },
    );
    let record = peer.next_event();
    assert_eq!(
        decode_wm_file_configuration_outcome(&record, caps)
            .unwrap()
            .outcome,
        PolicyProjectionOutcome::Committed
    );
    peer.ack(&record);
    assert!(matches!(
        event(&worker),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    (worker, peer, caps, probe)
}
fn outcome(transaction: u64) -> PolicyTransportCommand {
    PolicyTransportCommand::ProjectionOutcome {
        transaction: TransactionId::from_raw(transaction),
        request_id: 5,
        scene_generation: 7,
        outcome: PolicyProjectionOutcome::Committed,
        expect_session_operation: false,
    }
}

fn begin_read(peer: &mut Peer) -> u16 {
    assert!(peer.queued.is_empty());
    peer.tag += 1;
    let body = [
        2u32.to_le_bytes().as_slice(),
        &peer.offset.to_le_bytes(),
        &65500u32.to_le_bytes(),
    ]
    .concat();
    let mut bytes = ((7 + body.len()) as u32).to_le_bytes().to_vec();
    bytes.push(116);
    bytes.extend(peer.tag.to_le_bytes());
    bytes.extend(body);
    peer.stream.write_all(&bytes).unwrap();
    peer.tag
}
fn finish_read(peer: &mut Peer, tag: u16) -> Vec<u8> {
    let mut header = [0; 7];
    peer.stream.read_exact(&mut header).unwrap();
    assert_eq!(header[4], 117);
    assert_eq!(u16::from_le_bytes(header[5..].try_into().unwrap()), tag);
    let length = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
    assert!((11..=65536).contains(&length));
    let mut body = vec![0; length - 7];
    peer.stream.read_exact(&mut body).unwrap();
    let count = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    assert_eq!(count, body.len() - 4);
    peer.offset += count as u64;
    body[4..].to_vec()
}

#[test]
fn idle_driver_delivers_appended_outcome_without_a_following_command() {
    let (worker, mut peer, caps, probe) = configured();
    let before = probe.idle_returned.load(Ordering::SeqCst);
    let tag = begin_read(&mut peer);
    wait_for(|| probe.idle_returned.load(Ordering::SeqCst) > before);
    enqueue(&worker, outcome(50));
    let record = finish_read(&mut peer, tag);
    assert_eq!(
        decode_wm_file_projection_outcome(&record, caps)
            .unwrap()
            .transaction
            .raw(),
        50
    );
    assert_eq!(*probe.sent.lock().unwrap(), vec![10, 50]);
    assert_eq!(probe.fallback.load(Ordering::SeqCst), 0);
    // No next Cycle/command caused this reply. ACK is subsequent client I/O.
    peer.ack(&record);
    assert!(matches!(
        event(&worker),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
}

#[test]
fn idle_driver_services_ack_clunk_flush_and_blocks_when_quiet() {
    let (worker, mut peer, _, probe) = configured();
    let commands = probe.sent.lock().unwrap().clone();
    peer.open(6, b"limits", 0);
    assert_eq!(peer.rpc(120, &6u32.to_le_bytes()).unwrap().0, 121);
    // Earlier ACKs released the journal prefix: stale reread is refused.
    let read = [
        2u32.to_le_bytes().as_slice(),
        &0u64.to_le_bytes(),
        &65500u32.to_le_bytes(),
    ]
    .concat();
    let refused = peer.rpc(116, &read).unwrap();
    assert_eq!(refused.0, 7);
    assert_eq!(u32::from_le_bytes(refused.1.try_into().unwrap()), 116); // ESTALE
    let tag = begin_read(&mut peer);
    assert_eq!(peer.rpc(108, &tag.to_le_bytes()).unwrap().0, 109);
    assert_eq!(*probe.sent.lock().unwrap(), commands);
    assert_eq!(probe.fallback.load(Ordering::SeqCst), 0);
    wait_for(|| {
        probe.idle_entered.load(Ordering::SeqCst) > probe.idle_returned.load(Ordering::SeqCst)
    });
    let before = probe.idle_entered.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(
        probe.idle_entered.load(Ordering::SeqCst),
        before,
        "quiet idle must remain in its blocking turn"
    );
    let bells = probe.bells.load(Ordering::SeqCst);
    let stopped = Instant::now();
    assert!(worker.try_command(PolicyTransportCommand::Stop).is_ok());
    drop(worker);
    assert!(stopped.elapsed() < BOUND);
    assert_eq!(
        probe.bells.load(Ordering::SeqCst),
        bells,
        "Stop uses its existing owner, not the command bell"
    );
}

#[test]
fn idle_command_bell_survives_empty_to_poll_and_only_rings_on_admission() {
    let (worker, mut peer, caps, probe) = configured();
    let (entered, enter) = sync_channel(1);
    let (release, released) = sync_channel(1);
    *probe.before_idle.lock().unwrap() = Some(IdleGate {
        entered,
        release: released,
    });
    worker.command_wake.as_ref().unwrap().wake(); // Test scheduling wake only.
    enter.recv_timeout(BOUND).unwrap(); // Driver already observed Empty.
    let bells = probe.bells.load(Ordering::SeqCst);
    assert!(worker.try_command(outcome(60)).is_ok());
    let refused = worker
        .try_command(outcome(61))
        .expect_err("one-slot command queue full");
    assert_eq!(probe.bells.load(Ordering::SeqCst), bells + 1);
    release.send(()).unwrap(); // Bell was rung before Server::turn/poll.
    let first = peer.next_event();
    assert_eq!(
        decode_wm_file_projection_outcome(&first, caps)
            .unwrap()
            .transaction
            .raw(),
        60
    );
    peer.ack(&first);
    assert!(matches!(
        event(&worker),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    enqueue(&worker, refused);
    let second = peer.next_event();
    assert_eq!(
        decode_wm_file_projection_outcome(&second, caps)
            .unwrap()
            .transaction
            .raw(),
        61
    );
    peer.ack(&second);
    assert!(matches!(
        event(&worker),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    assert_eq!(*probe.sent.lock().unwrap(), vec![10, 60, 61]);
    assert_eq!(probe.fallback.load(Ordering::SeqCst), 0);
}

#[test]
fn idle_dirty_only_refuses_configuration_and_still_forwards_dirty() {
    let (worker, mut peer, caps, probe) = configured();
    let bytes =
        encode_wm_file_configuration(header(WmFileKind::Configuration, 3), &configuration(), caps)
            .unwrap();
    let refused = peer.submit(&bytes).unwrap();
    assert_eq!(refused.0, 7);
    assert_eq!(u32::from_le_bytes(refused.1.try_into().unwrap()), 11); // EAGAIN, no runtime permission.
    peer.clear_transaction();
    let dirty = PolicyDirtyRequest {
        connection_epoch: 9,
        policy_generation: 3,
        affected_outputs: vec![OutputId::from_raw(1)],
    };
    accepted(
        &mut peer,
        &encode_wm_file_dirty(header(WmFileKind::Dirty, 3), &dirty, caps).unwrap(),
    );
    assert!(matches!(event(&worker), PolicyTransportEvent::Dirty(value) if value == dirty));
    assert_eq!(probe.fallback.load(Ordering::SeqCst), 0);
}

#[test]
fn active_receive_ignores_command_bell_then_handles_queued_command_before_idle() {
    let (worker, mut peer, caps, probe) = configured();
    let scene = array_fixture::scene();
    let request = PolicyProjectionRequest {
        connection_epoch: 9,
        request_id: 5,
        scene_generation: scene.generation,
        policy_generation: 3,
        affected_outputs: vec![scene.active_output],
        cause: PolicyRequestCause::SceneChanged,
    };
    enqueue(
        &worker,
        PolicyTransportCommand::Cycle {
            snapshot_transaction: TransactionId::from_raw(100),
            request_transaction: TransactionId::from_raw(101),
            scene: Box::new(scene),
            actions: vec![],
            classifications: vec![],
            launch_origins: vec![],
            request,
        },
    );
    let cycle = peer.next_event();
    assert_eq!(
        decode_wm_file_cycle(&cycle, caps)
            .unwrap()
            .request
            .request_id,
        5
    );
    peer.ack(&cycle);
    wait_for(|| probe.active.load(Ordering::SeqCst) == 2);
    let idle_before = probe.idle_entered.load(Ordering::SeqCst);
    enqueue(&worker, outcome(70)); // Bell drains during active receive; queue owns command.
    peer.open(6, b"limits", 0);
    assert_eq!(peer.rpc(120, &6u32.to_le_bytes()).unwrap().0, 121);
    assert_eq!(probe.idle_entered.load(Ordering::SeqCst), idle_before);
    assert_eq!(*probe.sent.lock().unwrap(), vec![10, 101]);
    let mut proposal = array_fixture::proposal();
    proposal.connection_epoch = 9;
    proposal.request_id = 5;
    proposal.transaction = TransactionId::from_raw(20);
    proposal.launch_contexts.clear();
    proposal.output_launch_contexts.clear();
    // Only the driver/transport is exercised; these rows are supplied facts.
    assert_eq!(
        peer.submit(
            &encode_wm_file_projection(header(WmFileKind::Projection, 3), &proposal, caps).unwrap()
        )
        .unwrap()
        .0,
        119
    );
    assert!(matches!(
        event(&worker),
        PolicyTransportEvent::Projection(_)
    ));
    let submitted = peer.next_event();
    assert_eq!(
        decode_wm_file_submitted(&submitted).unwrap().submission_id,
        3
    );
    let outcome = peer.next_event();
    assert_eq!(
        decode_wm_file_projection_outcome(&outcome, caps)
            .unwrap()
            .transaction
            .raw(),
        70
    );
    assert_eq!(*probe.sent.lock().unwrap(), vec![10, 101, 70]);
    assert_eq!(
        probe.send_idle.lock().unwrap().last(),
        Some(&(70, idle_before)),
        "queued command is sent before returning to idle after active receive"
    );
    assert!(matches!(
        event(&worker),
        PolicyTransportEvent::ReadyForCycle { .. }
    ));
    assert_eq!(probe.fallback.load(Ordering::SeqCst), 0);
}

#[test]
fn incomplete_readiness_opt_in_fails_once_instead_of_spinning() {
    struct Noop;
    impl PolicyAdapterCommandWake for Noop {
        fn wake(&self) {}
    }
    struct Incomplete;
    impl PolicyAdapter for Incomplete {
        fn admit(
            &mut self,
            _: PolicyAdmissionPermit,
            _: u64,
            _: Option<PolicyProfileAdmission>,
        ) -> Result<(), String> {
            Ok(())
        }
        fn selected_capabilities(&self) -> u64 {
            0
        }
        fn receive_within(
            &mut self,
            _: PolicyReceivePermit,
            _: Duration,
        ) -> Result<PolicyAdapterEvent, String> {
            Ok(PolicyAdapterEvent::Configuration {
                transaction: TransactionId::from_raw(10),
                configuration: configuration().configuration,
            })
        }
        fn try_receive(
            &mut self,
            _: PolicyReceivePermit,
        ) -> Result<Option<PolicyAdapterEvent>, String> {
            panic!("invalid opt-in must fail closed")
        }
        fn send(&mut self, _: &PolicyTransportCommand) -> Result<(), String> {
            panic!("no command supplied")
        }
        fn command_wake_handle(&self) -> Option<Box<dyn PolicyAdapterCommandWake>> {
            Some(Box::new(Noop))
        }
        fn disconnect(&mut self) {}
    }
    let (_commands, receive) = sync_channel(1);
    let (events, _audit) = sync_channel(2);
    assert_eq!(
        run_policy_transport(&mut Incomplete, 9, None, &receive, &events),
        Err("adapter does not support readiness-driven idle".into())
    );
}

#[test]
fn disconnected_command_queue_does_not_ring() {
    struct Noop;
    impl PolicyAdapterCommandWake for Noop {
        fn wake(&self) {}
    }
    let probe = Arc::new(Probe::default());
    let (commands, receiver) = sync_channel(1);
    drop(receiver);
    let (_sender, events) = sync_channel(1);
    let worker = PolicyTransportWorker {
        commands: Some(commands),
        events,
        thread: None,
        stop: None,
        command_wake: Some(Box::new(Bell {
            inner: Box::new(Noop),
            probe: probe.clone(),
        })),
    };
    assert!(worker.try_command(outcome(90)).is_err());
    assert_eq!(probe.bells.load(Ordering::SeqCst), 0);
}

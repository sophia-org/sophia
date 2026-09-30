//! Output-owner recovery when the output file peer disconnects, through
//! Session's public policy state, the real output file service and a generic
//! raw 9P peer. The peer runs in a thread authorized by this process's PID;
//! each step waits for an explicit command over a bounded channel.
//!
//! Limits: Session's native owner loop (owner_loop/wm_phase.rs) and KMS
//! effects are not exercised. The tests supply native cancellation results to
//! the loop's cancellation function and call its public observation seams; no
//! preparation abort, apply, first presentation or rollback reaches a device,
//! and LiveOutputTopologyOwner gating is not involved. The child-exit case
//! uses a controlled stand-in supervisor child; the 9P peer still runs in this
//! test process, so protected child identity is covered by separate tests.
use super::*;
use crate::live_output_authority::LiveOutputAuthorityOwner;
use crate::live_session::tests::shell_file_peer as raw_peer;
use crate::live_session::{
    LiveOutputTopologyExecutionPhase as Execution, NativeOutputCancellationRequest,
    cancel_output_topology_execution,
};
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
    project_live_output_authority_snapshot,
};
use sophia_engine::{OutputTopologyTransactionFailure, OutputTopologyTransactionPhase};
use sophia_protocol::output_files::*;
use sophia_protocol::{
    DisplayModeId, OutputAuthoritySnapshot, OutputHeadTargetProposal, OutputLogicalGroupProposal,
    OutputTopologyCandidate, OutputTopologyIntent, OutputTransform, OutputV1ClientHello,
    OutputV1Proposal, OutputVrrPolicy, SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
    SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The file role's first connection epoch; distinct from topology epoch 7.
const EPOCH: u64 = 3;
const TOPOLOGY_EPOCH: u64 = 7;

#[path = "output_file_startup_recovery.rs"]
mod startup_recovery;

enum Step {
    Negotiate,
    Propose(TransactionId, OutputTopologyCandidate),
    Quiet,
    Close,
}

#[derive(Debug)]
enum Seen {
    Negotiated {
        epoch: u64,
        topology: OutputAuthoritySnapshot,
        qid: u64,
    },
    Submitted,
    Quiet,
    Closed,
}

/// A generic output file peer: raw 9P requests and native output records.
struct Peer {
    steps: SyncSender<Step>,
    seen: Receiver<Seen>,
    thread: Option<JoinHandle<()>>,
}

impl Peer {
    fn spawn(socket: PathBuf) -> Self {
        let (steps, step_rx) = sync_channel(1);
        let (seen_tx, seen) = sync_channel(1);
        let thread = std::thread::spawn(move || run_peer(&socket, &step_rx, &seen_tx));
        Self {
            steps,
            seen,
            thread: Some(thread),
        }
    }

    fn call(&mut self, step: Step) -> Seen {
        self.steps.send(step).expect("peer thread running");
        match self.seen.recv_timeout(Duration::from_secs(5)) {
            Ok(seen) => seen,
            Err(_) => {
                // Surface the peer's own assertion when it has already ended.
                // Never wait on a live peer: it may be back in steps.recv, and
                // unwinding drops our channel ends so it returns promptly.
                if let Some(thread) = self.thread.take_if(|thread| thread.is_finished()) {
                    thread.join().expect("peer thread");
                }
                panic!("peer step did not complete");
            }
        }
    }

    fn negotiate(&mut self) -> (u64, OutputAuthoritySnapshot, u64) {
        match self.call(Step::Negotiate) {
            Seen::Negotiated {
                epoch,
                topology,
                qid,
            } => (epoch, topology, qid),
            other => panic!("expected negotiation, saw {other:?}"),
        }
    }

    fn propose(&mut self, transaction: u64, candidate: OutputTopologyCandidate) {
        let step = Step::Propose(TransactionId::from_raw(transaction), candidate);
        assert!(matches!(self.call(step), Seen::Submitted));
    }

    /// No further event (in particular no Outcome) reaches this connection.
    fn quiet(&mut self) {
        assert!(matches!(self.call(Step::Quiet), Seen::Quiet));
    }

    fn close(mut self) {
        assert!(matches!(self.call(Step::Close), Seen::Closed));
        self.thread.take().unwrap().join().expect("peer thread");
    }
}

fn event(peer: &mut raw_peer::Peer) -> (OutputFileKind, u64, Vec<u8>) {
    let bytes = peer.next_event();
    let record = decode_output_file_record(&bytes, OutputFileClass::Event).unwrap();
    (
        record.header.kind,
        record.header.sequence,
        record.body.to_vec(),
    )
}

fn acknowledge(peer: &mut raw_peer::Peer, epoch: u64, sequence: u64) {
    let ack = encode_output_file_ack(OutputFileAck {
        connection_epoch: epoch,
        sequence,
    })
    .unwrap();
    assert_eq!(peer.write(4, &ack).0, 119);
}

/// Stages one candidate in a fresh transaction fid and submits it.
fn submit(peer: &mut raw_peer::Peer, epoch: u64, submission: u64, record: &[u8]) {
    peer.open(5, b"transaction", 2);
    assert_eq!(peer.write(5, record).0, 119);
    let control = encode_output_file_submit(OutputFileSubmit {
        connection_epoch: epoch,
        submission_id: submission,
        candidate_bytes: record.len() as u32,
    })
    .unwrap();
    assert_eq!(peer.write(3, &control).0, 119);
}

fn run_peer(socket: &Path, steps: &Receiver<Step>, seen: &SyncSender<Seen>) {
    let mut peer = raw_peer::Peer::connect(socket);
    peer.setup();
    let mut epoch = 0;
    let mut submission = 0;
    while let Ok(step) = steps.recv() {
        let reply = match step {
            Step::Negotiate => {
                peer.open(6, b"limits", 0);
                let limits = peer.read(6, 0);
                peer.clunk(6);
                epoch = decode_output_file_record(&limits, OutputFileClass::Object)
                    .unwrap()
                    .header
                    .connection_epoch;
                submission += 1;
                let record = encode_output_file_record(
                    OutputFileHeader {
                        kind: OutputFileKind::Negotiate,
                        connection_epoch: epoch,
                        submission_id: submission,
                        sequence: 0,
                    },
                    &encode_output_file_negotiate(OutputV1ClientHello {
                        minimum_revision: 1,
                        maximum_revision: 1,
                        capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE
                            | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
                    }),
                )
                .unwrap();
                submit(&mut peer, epoch, submission, &record);
                assert_eq!(event(&mut peer).0, OutputFileKind::Submitted);
                assert_eq!(event(&mut peer).0, OutputFileKind::Negotiated);
                let (kind, sequence, body) = event(&mut peer);
                assert_eq!(kind, OutputFileKind::ObjectPublished);
                let publication = decode_output_file_publication(&body).unwrap();
                // The announcement is acknowledged only after a full read.
                peer.open(7, b"topology", 0);
                let object = peer.read(7, 0);
                peer.clunk(7);
                let record = decode_output_file_record(&object, OutputFileClass::Object).unwrap();
                let topology = decode_output_file_topology(record.body, epoch)
                    .unwrap()
                    .snapshot;
                acknowledge(&mut peer, epoch, sequence);
                peer.clunk(5);
                Seen::Negotiated {
                    epoch,
                    topology,
                    qid: publication.qid_path,
                }
            }
            Step::Propose(transaction, candidate) => {
                submission += 1;
                let body = encode_output_file_proposal(
                    transaction,
                    &OutputV1Proposal {
                        connection_epoch: epoch,
                        candidate,
                    },
                )
                .unwrap();
                let record = encode_output_file_record(
                    OutputFileHeader {
                        kind: OutputFileKind::Proposal,
                        connection_epoch: epoch,
                        submission_id: submission,
                        sequence: 0,
                    },
                    &body,
                )
                .unwrap();
                submit(&mut peer, epoch, submission, &record);
                let (kind, sequence, _) = event(&mut peer);
                assert_eq!(kind, OutputFileKind::Submitted);
                acknowledge(&mut peer, epoch, sequence);
                peer.clunk(5);
                Seen::Submitted
            }
            Step::Quiet => {
                peer.no_event();
                Seen::Quiet
            }
            Step::Close => {
                peer.shutdown();
                let _ = seen.send(Seen::Closed);
                return;
            }
        };
        if seen.send(reply).is_err() {
            return;
        }
    }
}

struct Rig {
    fixture: ReloadFixture,
    capability: LibdrmNativeOutputCapability,
    snapshot: OutputAuthoritySnapshot,
    alternate: DisplayModeId,
    socket: PathBuf,
}

/// One head with two supplied modes (60 and 75 Hz at the same size), a
/// published topology on the current mode, the real file service with its
/// first epoch at EPOCH, and an initially idle supervisor.
fn rig(label: &str) -> Rig {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let output = public.outputs[0];
    let size = |refresh| {
        LibdrmNativeOutputTiming::new(
            u32::try_from(output.size.width).unwrap(),
            u32::try_from(output.size.height).unwrap(),
            refresh,
        )
    };
    let current = size(60_000);
    let capability = LibdrmNativeOutputCapability::new(
        output.id,
        11,
        "DP-1",
        [current, size(75_000)],
        Some(current),
        current,
        LibdrmNativeVrrPropertyDiscoveryStatus::Discovered,
    )
    .unwrap()
    .bind_head(sophia_engine::RenderHeadId::from_raw(11))
    .unwrap();
    let snapshot = project_live_output_authority_snapshot(
        std::slice::from_ref(&capability),
        &[output],
        TOPOLOGY_EPOCH,
    )
    .unwrap();
    let head = &snapshot.heads[0];
    assert_eq!(head.modes.len(), 2, "two supplied modes");
    let alternate = head
        .modes
        .iter()
        .map(|mode| mode.mode)
        .find(|mode| Some(*mode) != head.current_mode)
        .unwrap();
    public.output_authority = Some(LiveOutputAuthorityOwner::new(EPOCH, snapshot.clone()).unwrap());
    public.output_capabilities = vec![capability.clone()];
    let directory = std::env::temp_dir().join(format!(
        "sophia-output-recovery-{}-{label}",
        std::process::id()
    ));
    let mut transport = sophia_runtime::OutputFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        EPOCH,
        OutputFileLimits::default(),
    )
    .unwrap();
    // Test owner: the in-process peer thread carries this process's PID.
    transport
        .authorize_supervised_pid(std::process::id())
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let service = sophia_runtime::OutputFileService::spawn(transport, snapshot.clone()).unwrap();
    let supervisor = ProcessSupervisor::new(
        SupervisedProcessKind::OutputAuthority,
        ProcessLaunchSpec::new("/bin/true"),
    );
    public.output_service = Some(LiveOutputService::Files {
        service,
        supervisor: Box::new(supervisor),
    });
    Rig {
        fixture,
        capability,
        snapshot,
        alternate,
        socket,
    }
}

impl Rig {
    fn public(&mut self) -> &mut LivePublicPolicyState {
        self.fixture.wm.public.as_mut().unwrap()
    }

    fn authority(&mut self) -> &LiveOutputAuthorityOwner {
        self.public().output_authority.as_ref().unwrap()
    }

    fn supervisor(&mut self) -> &mut ProcessSupervisor {
        let LiveOutputService::Files { supervisor, .. } =
            self.public().output_service.as_mut().unwrap()
        else {
            panic!("file service expected")
        };
        supervisor
    }

    fn peer(&self) -> Peer {
        Peer::spawn(self.socket.clone())
    }

    /// An apply that changes only the refresh rate of the one head.
    fn apply(&self) -> OutputTopologyCandidate {
        let head = &self.snapshot.heads[0];
        let group = &self.snapshot.groups[0];
        OutputTopologyCandidate {
            base_topology_epoch: self.snapshot.topology_epoch,
            intent: OutputTopologyIntent::Apply,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: head.head,
                head_generation: head.generation,
                mode: self.alternate,
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Disabled,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: group.output,
                logical: group.logical,
                members: group.members.clone(),
            }],
        }
    }

    /// Runs Session's output turn until `done` holds, within a fixed bound.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut LivePublicPolicyState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.fixture.wm.poll_output_authority().unwrap();
            if done(self.public()) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn admitted(&mut self, transaction: u64) {
        let transaction = TransactionId::from_raw(transaction);
        self.until("the proposal's admission", |public| {
            public
                .output_authority
                .as_ref()
                .unwrap()
                .active_transaction()
                == Some(transaction)
        });
    }

    fn cancellation_requested(&mut self, transaction: u64) {
        let transaction = TransactionId::from_raw(transaction);
        self.until("the disconnect's cancellation request", |public| {
            public
                .output_candidate_cancellation_reason(transaction)
                .is_some()
        });
    }

    /// Debt is outstanding: nothing may publish and the connection epoch
    /// waits for local settlement.
    fn assert_debt(&mut self, transaction: u64) {
        let capability = self.capability.clone();
        let snapshot = self.snapshot.clone();
        let public = self.public();
        assert!(public.output_candidate_active());
        assert!(
            public
                .output_candidate_cancellation_reason(TransactionId::from_raw(transaction))
                .is_some()
        );
        assert_eq!(public.output_pending_connection_epoch, Some(EPOCH + 1));
        assert!(!public.take_output_topology_reload_request());
        assert!(
            public
                .publish_output_authority_snapshot(snapshot.clone(), vec![capability])
                .is_err(),
            "publication must not race cancellation debt"
        );
        assert_eq!(public.published_output_snapshot(), Some(snapshot));
        assert_eq!(self.authority().connection_epoch(), EPOCH);
    }

    /// Settled with the original topology, on the replacement epoch.
    fn assert_preserved(&mut self) {
        let snapshot = self.snapshot.clone();
        let public = self.public();
        assert_eq!(public.published_output_snapshot(), Some(snapshot));
        assert!(!public.output_candidate_active());
        assert!(public.output_cancel_requested.is_none());
        assert!(!public.output_effect_dispatched);
        assert!(public.take_output_topology_effect().is_none());
        assert!(public.output_service.is_some(), "service kept");
        assert_eq!(self.authority().connection_epoch(), EPOCH + 1);
        assert!(self.authority().active_transaction().is_none());
    }

    /// A replacement client sees the expected topology on a fresh epoch and
    /// Qid, and nothing from the abandoned connection.
    fn replacement(&mut self, expected: &OutputAuthoritySnapshot, old_qid: u64) -> Peer {
        let mut peer = self.peer();
        let (epoch, topology, qid) = peer.negotiate();
        assert_eq!(epoch, EPOCH + 1);
        assert_eq!(&topology, expected);
        assert_ne!(qid, old_qid);
        peer.quiet();
        peer
    }

    fn dispatch(&mut self, transaction: u64) -> Vec<sophia_engine::RenderHeadId> {
        let effect = self.public().take_output_topology_effect().unwrap();
        assert_eq!(effect.transaction, TransactionId::from_raw(transaction));
        assert_eq!(effect.published_snapshot, self.snapshot);
        assert!(self.public().output_effect_dispatched);
        effect.resolved.affected_heads().collect()
    }

    fn apply_heads(&mut self, transaction: u64, heads: &[sophia_engine::RenderHeadId]) {
        let transaction = TransactionId::from_raw(transaction);
        let public = self.public();
        public
            .begin_output_topology_apply(transaction, heads)
            .unwrap();
        public
            .observe_output_topology_applied(transaction, heads)
            .unwrap();
        assert_eq!(
            self.authority().active_phase(),
            Some(OutputTopologyTransactionPhase::AwaitingFirstPresentation)
        );
    }

    /// Run the loop's cancellation dispatch with a supplied native result and
    /// the actual policy rejection. Native effects remain outside this rig.
    fn cancel_execution(
        &mut self,
        transaction: u64,
        mut phase: Execution,
        report: sophia_backend_live::LiveProductionNativeTopologyPreparationPhase,
    ) -> Execution {
        let expected_request = match phase {
            Execution::Preparing | Execution::Applying => {
                NativeOutputCancellationRequest::AbortPreparation
            }
            Execution::AwaitingFirstPresentation | Execution::Reconciling => {
                NativeOutputCancellationRequest::Rollback
            }
            _ => panic!("this rig expects a native cancellation request"),
        };
        cancel_output_topology_execution(
            &mut phase,
            |request| {
                assert_eq!(request, expected_request);
                Ok(Some(report))
            },
            || {
                self.public().reject_output_topology_effect(
                    TransactionId::from_raw(transaction),
                    OutputTopologyTransactionFailure::Stale,
                )
            },
        )
        .unwrap();
        phase
    }
}

/// A: the peer leaves before Session dispatches the effect. The candidate is
/// abandoned at once; a replacement's proposal is admitted against the
/// preserved topology rather than refused as stale.
#[test]
fn disconnect_before_dispatch_abandons_the_candidate_and_keeps_topology() {
    let mut rig = rig("before-dispatch");
    let mut first = rig.peer();
    let (epoch, topology, qid) = first.negotiate();
    assert_eq!((epoch, &topology), (EPOCH, &rig.snapshot));
    first.propose(11, rig.apply());
    rig.admitted(11);
    assert!(rig.public().output_topology_effect_pending());
    assert!(!rig.public().output_effect_dispatched);
    first.close();
    rig.until("the disconnect's epoch replacement", |public| {
        public.output_authority.as_ref().unwrap().connection_epoch() == EPOCH + 1
    });
    rig.assert_preserved();
    let snapshot = rig.snapshot.clone();
    let mut second = rig.replacement(&snapshot, qid);
    second.propose(12, rig.apply());
    rig.admitted(12);
    second.quiet();
    second.close();
}

/// A2: a second proposal queued behind the undispatched one is abandoned
/// with it and never promoted.
#[test]
fn disconnect_abandons_a_queued_proposal_without_promotion() {
    let mut rig = rig("queued");
    let mut first = rig.peer();
    let (_, _, qid) = first.negotiate();
    first.propose(11, rig.apply());
    rig.admitted(11);
    // Transport custody: 11 is active in the owner, 12 waits behind it.
    first.propose(12, rig.apply());
    first.close();
    rig.until("the disconnect's epoch replacement", |public| {
        public.output_authority.as_ref().unwrap().connection_epoch() == EPOCH + 1
    });
    rig.assert_preserved();
    let snapshot = rig.snapshot.clone();
    let second = rig.replacement(&snapshot, qid);
    for _ in 0..32 {
        rig.fixture.wm.poll_output_authority().unwrap();
    }
    assert!(
        rig.authority().active_transaction().is_none(),
        "no promotion"
    );
    second.close();
}

/// B: the peer leaves after dispatch, before apply. Cancellation debt holds
/// the replacement's events until the preparation terminal settles locally.
#[test]
fn disconnect_after_dispatch_holds_debt_until_preparation_settles() {
    let mut rig = rig("after-dispatch");
    let mut first = rig.peer();
    let (_, _, qid) = first.negotiate();
    first.propose(11, rig.apply());
    rig.admitted(11);
    rig.dispatch(11);
    first.close();
    rig.cancellation_requested(11);
    rig.assert_debt(11);
    // The replacement negotiates at the transport; Session holds its events.
    let snapshot = rig.snapshot.clone();
    let mut second = rig.peer();
    let (epoch, topology, second_qid) = second.negotiate();
    assert_eq!((epoch, &topology), (EPOCH + 1, &snapshot));
    assert_ne!(second_qid, qid);
    for _ in 0..32 {
        rig.fixture.wm.poll_output_authority().unwrap();
    }
    rig.assert_debt(11);
    assert_eq!(
        rig.cancel_execution(
            11,
            Execution::Preparing,
            sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::Aborting,
        ),
        Execution::Preparing
    );
    rig.assert_debt(11);
    // The owner loop's preparation-phase cancellation ends in this terminal.
    rig.public()
        .reject_output_topology_effect(
            TransactionId::from_raw(11),
            OutputTopologyTransactionFailure::Stale,
        )
        .unwrap();
    rig.assert_preserved();
    second.propose(12, rig.apply());
    rig.admitted(12);
    second.quiet();
    second.close();
}

/// B-exit: the supervised process exits while the test peer remains connected.
/// Session must request pause itself, retain preparation debt, and keep the
/// listener paused after settlement instead of starting the one-shot again.
#[test]
fn supervised_exit_pauses_admission_and_preserves_preparation_debt() {
    let mut rig = rig("child-exit");
    rig.supervisor()
        .replace_launch_spec(ProcessLaunchSpec::new("/bin/sleep").arg("60").process_group())
        .unwrap();
    rig.supervisor()
        .apply(sophia_runtime::SupervisorCommand::StartProcess {
            process: SupervisedProcessKind::OutputAuthority,
            delay: Duration::ZERO,
        })
        .unwrap()
        .expect("controlled child started");
    let child = rig.supervisor().child_id().unwrap();

    let mut peer = rig.peer();
    peer.negotiate();
    peer.propose(11, rig.apply());
    rig.admitted(11);
    rig.dispatch(11);
    peer.quiet();
    for _ in 0..32 {
        rig.fixture.wm.poll_output_authority().unwrap();
    }
    assert!(
        rig.public()
            .output_candidate_cancellation_reason(TransactionId::from_raw(11))
            .is_none()
    );
    assert!(rig.public().output_pending_connection_epoch.is_none());
    assert_eq!(rig.supervisor().child_id(), Some(child));
    // This is our unreaped stand-in child. Its exit, not a peer close or a
    // direct service command, must cause Session to pause the worker.
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child as i32).unwrap(),
        rustix::process::Signal::TERM,
    )
    .unwrap();
    rig.cancellation_requested(11);
    assert!(rig.supervisor().child_id().is_none());
    rig.assert_debt(11);
    assert_eq!(
        rig.cancel_execution(
            11,
            Execution::Preparing,
            sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::Aborting,
        ),
        Execution::Preparing
    );
    rig.assert_debt(11);
    rig.public()
        .reject_output_topology_effect(
            TransactionId::from_raw(11),
            OutputTopologyTransactionFailure::Stale,
        )
        .unwrap();
    rig.assert_preserved();
    peer.close();

    // A bare socket connect would produce no Connected event even on an
    // accepting worker: that event requires role negotiation. Send Tversion
    // and require that not even the first response byte is served.
    use std::io::{Read, Write};
    let mut replacement = std::os::unix::net::UnixStream::connect(&rig.socket).unwrap();
    replacement
        .set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    replacement
        .set_write_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    let version = [
        21u32.to_le_bytes().as_slice(),
        &[100, 255, 255],
        &65536u32.to_le_bytes(),
        &8u16.to_le_bytes(),
        b"9P2000.L",
    ]
    .concat();
    replacement.write_all(&version).unwrap();
    let response = replacement.read(&mut [0]);
    assert!(
        matches!(&response, Err(error) if matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        )),
        "paused listener served a replacement: {response:?}"
    );
    assert!(matches!(
        rig.public()
            .output_service
            .as_ref()
            .unwrap()
            .event_timeout(Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    for _ in 0..32 {
        rig.fixture.wm.poll_output_authority().unwrap();
    }
    assert!(rig.supervisor().child_id().is_none(), "one-shot not restarted");
    rig.assert_preserved();
    drop(replacement);
}

/// C: the peer leaves after apply. Rejection moves the candidate to rollback
/// without settling it; only the rollback observation settles, locally.
#[test]
fn disconnect_after_apply_settles_only_after_rollback() {
    let mut rig = rig("after-apply");
    let mut first = rig.peer();
    let (_, _, qid) = first.negotiate();
    first.propose(11, rig.apply());
    rig.admitted(11);
    let heads = rig.dispatch(11);
    rig.apply_heads(11, &heads);
    first.close();
    rig.cancellation_requested(11);
    rig.assert_debt(11);
    let transaction = TransactionId::from_raw(11);
    assert_eq!(
        rig.cancel_execution(
            11,
            Execution::AwaitingFirstPresentation,
            sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::RollingBack,
        ),
        Execution::RollingBack
    );
    assert_eq!(
        rig.authority().active_phase(),
        Some(OutputTopologyTransactionPhase::RollingBack)
    );
    rig.assert_debt(11);
    rig.public()
        .observe_output_topology_rolled_back(transaction, &heads)
        .unwrap();
    rig.assert_preserved();
    let snapshot = rig.snapshot.clone();
    rig.replacement(&snapshot, qid).close();
}

/// C2: a failed rollback settles as Failed, locally, and publishes nothing.
/// The physical state after a failed rollback is outside this test.
#[test]
fn disconnect_after_apply_with_failed_rollback_publishes_nothing() {
    let mut rig = rig("rollback-failed");
    let mut first = rig.peer();
    let (_, _, qid) = first.negotiate();
    first.propose(11, rig.apply());
    rig.admitted(11);
    let heads = rig.dispatch(11);
    rig.apply_heads(11, &heads);
    first.close();
    rig.cancellation_requested(11);
    let transaction = TransactionId::from_raw(11);
    assert_eq!(
        rig.cancel_execution(
            11,
            Execution::AwaitingFirstPresentation,
            sophia_backend_live::LiveProductionNativeTopologyPreparationPhase::RollingBack,
        ),
        Execution::RollingBack
    );
    rig.assert_debt(11);
    rig.public()
        .observe_output_topology_rollback_failed(transaction)
        .unwrap();
    rig.assert_preserved();
    let snapshot = rig.snapshot.clone();
    rig.replacement(&snapshot, qid).close();
}

/// D, the contrast: first presentation commits before Session sees the
/// disconnect. A committed topology is not made private again; the service
/// publishes it to the next client even though its requester vanished.
#[test]
fn commit_before_the_disconnect_is_observed_still_publishes() {
    let mut rig = rig("commit-wins");
    let mut first = rig.peer();
    let (_, _, qid) = first.negotiate();
    first.propose(11, rig.apply());
    rig.admitted(11);
    let heads = rig.dispatch(11);
    rig.apply_heads(11, &heads);
    let outputs = rig
        .authority()
        .active_resolved()
        .unwrap()
        .outputs
        .iter()
        .map(|output| output.id)
        .collect::<Vec<_>>();
    // Close without an output turn, so the Disconnected event stays queued.
    first.close();
    let committed = rig
        .public()
        .observe_output_topology_first_presented(TransactionId::from_raw(11), &outputs)
        .unwrap()
        .expect("committed snapshot");
    assert_eq!(committed.topology_epoch, TOPOLOGY_EPOCH + 1);
    assert_eq!(committed.heads[0].current_mode, Some(rig.alternate));
    assert_eq!(
        rig.public().published_output_snapshot(),
        Some(committed.clone())
    );
    rig.until("the disconnect's epoch replacement", |public| {
        public.output_authority.as_ref().unwrap().connection_epoch() == EPOCH + 1
    });
    let public = rig.public();
    assert!(!public.output_candidate_active());
    assert!(public.output_service.is_some(), "service kept");
    assert_eq!(public.published_output_snapshot(), Some(committed.clone()));
    rig.replacement(&committed, qid).close();
}

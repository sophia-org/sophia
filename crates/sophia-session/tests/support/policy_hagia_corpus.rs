//! The eleven revision-1 behaviour scenarios through the same frozen normal
//! Hagia over current IPC and over files, compared proposal by proposal.
//!
//! The protected startup helper admits Hagia and publishes its configuration;
//! the corpus then takes that already configured worker, so the shared
//! LivePublicPolicyState is no longer driving it. A test-owned canonical
//! reducer issues each scenario's scene and cause and decides its outcome, as
//! the runtime conformance host does. This is policy behaviour parity through
//! the production workers and adapters, not LivePublicPolicyState layout or
//! native settlement.
use super::*;
use sophia_protocol::{
    OutputId, PolicyProjectionOutcome, PolicyProjectionProposal, SOPHIA_WM_V1_BEHAVIOR_SCENARIOS,
    sophia_wm_v1_behavior_cause, sophia_wm_v1_behavior_scene,
};
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
struct ScenarioObservation {
    scenario: &'static str,
    proposal: PolicyProjectionProposal,
    outcome: PolicyProjectionOutcome,
}

fn next_event(
    worker: &policy_transport_worker::PolicyTransportWorker,
    deadline: Instant,
) -> policy_transport_worker::PolicyTransportEvent {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .expect("real Hagia corpus response deadline");
    worker
        .event_timeout(remaining)
        .expect("the same Hagia keeps its transport")
}

/// One configured worker taken from Session, driven one canonical cycle at a
/// time. Shared with the behaviour-coverage corpus.
pub(super) struct CorpusWorker {
    pub(super) worker: policy_transport_worker::PolicyTransportWorker,
    pub(super) epoch: u64,
    pub(super) selected: u64,
}

impl CorpusWorker {
    /// Takes the configured worker; nothing else polls it afterwards.
    pub(super) fn take(wm: &mut LiveWmSession) -> Self {
        let public = wm.public.as_mut().unwrap();
        assert!(public.configured && public.transport_ready);
        assert!(public.in_flight_request.is_none());
        // The configured catalog stays with Hagia's configuration. Each corpus
        // snapshot carries the conformance host's exact vocabulary instead: no
        // actions and no session operations, as encode_wm_v1_policy_snapshot
        // receives them there.
        assert!(!public.actions.is_empty(), "Hagia configured its catalog");
        Self {
            epoch: public.connection_epoch,
            selected: public.selected_capabilities,
            worker: public.worker.take().expect("configured worker"),
        }
    }

    /// Issues one cycle for `scene` and `cause`, lets `decide` settle Hagia's
    /// proposal on the test-owned canonical reducer, returns the outcome and
    /// waits until the driver is ready for the next cycle with the admitted
    /// selection unchanged.
    pub(super) fn cycle(
        &self,
        label: &str,
        reducer: &mut sophia_engine::PolicyProjectionReducer,
        scene: &sophia_protocol::PolicySceneSnapshot,
        cause: sophia_protocol::PolicyRequestCause,
        transaction: u64,
        launch_origins: Vec<sophia_protocol::PolicyLaunchContext>,
        decide: impl FnOnce(
            &mut sophia_engine::PolicyProjectionReducer,
            &PolicyProjectionProposal,
        ) -> PolicyProjectionOutcome,
    ) -> (PolicyProjectionProposal, PolicyProjectionOutcome) {
        if reducer.scene().generation != scene.generation {
            reducer.observe_scene(scene.clone()).unwrap();
        }
        let mut affected_outputs = scene
            .outputs
            .iter()
            .map(|output| output.output)
            .collect::<Vec<_>>();
        affected_outputs.sort_by_key(|output| (*output != scene.active_output, output.raw()));
        let request = reducer
            .issue_request_with_cause(affected_outputs, cause)
            .unwrap();
        assert!(
            self.worker
                .try_command(policy_transport_worker::PolicyTransportCommand::Cycle {
                    snapshot_transaction: TransactionId::from_raw(transaction),
                    request_transaction: TransactionId::from_raw(transaction + 1),
                    scene: Box::new(scene.clone()),
                    actions: Vec::new(),
                    classifications: Vec::new(),
                    launch_origins,
                    request: request.clone(),
                })
                .is_ok(),
            "{label}: the corpus cycle is the worker's only command"
        );
        let deadline = Instant::now() + Duration::from_secs(8);
        let proposal = match next_event(&self.worker, deadline) {
            policy_transport_worker::PolicyTransportEvent::Projection(proposal) => *proposal,
            policy_transport_worker::PolicyTransportEvent::Failed(error) => {
                panic!("{label}: transport failed: {error}")
            }
            _ => panic!("{label}: expected Hagia's projection"),
        };
        assert_eq!(proposal.connection_epoch, self.epoch);
        assert_eq!(proposal.request_id, request.request_id);
        assert_eq!(proposal.base_generation, request.scene_generation);
        // The canonical reducer decides each outcome, as the conformance host
        // does; nothing here settles Session layout.
        let outcome = decide(reducer, &proposal);
        assert!(
            self.worker
                .try_command(
                    policy_transport_worker::PolicyTransportCommand::ProjectionOutcome {
                        transaction: proposal.transaction,
                        request_id: proposal.request_id,
                        scene_generation: reducer.scene().generation,
                        outcome,
                        // No corpus action names a session operation.
                        expect_session_operation: false,
                    }
                )
                .is_ok()
        );
        match next_event(&self.worker, deadline) {
            policy_transport_worker::PolicyTransportEvent::ReadyForCycle { capabilities } => {
                assert_eq!(capabilities, self.selected, "{label}: selection changed");
            }
            policy_transport_worker::PolicyTransportEvent::Failed(error) => {
                panic!("{label}: transport failed after its outcome: {error}")
            }
            _ => panic!("{label}: expected readiness for the next cycle"),
        }
        (proposal, outcome)
    }
}

fn run_corpus(case: &str, transport: WmTransportSelection) -> (u64, Vec<ScenarioObservation>) {
    with_normal_hagia_transport(case, transport, |wm, _, _, _, _, identity| {
        let corpus = CorpusWorker::take(wm);
        let selected = corpus.selected;
        let scenarios = SOPHIA_WM_V1_BEHAVIOR_SCENARIOS.as_slice();
        let mut reducer = sophia_engine::PolicyProjectionReducer::new(
            sophia_wm_v1_behavior_scene(scenarios[0]).unwrap(),
        )
        .unwrap();
        reducer.connect(corpus.epoch).unwrap();
        let mut observations = Vec::new();
        for (index, scenario) in scenarios.iter().copied().enumerate() {
            let scene = sophia_wm_v1_behavior_scene(scenario).unwrap();
            assert!(
                scene.session_operations.is_empty(),
                "{scenario}: canonical scene"
            );
            let (proposal, outcome) = corpus.cycle(
                scenario,
                &mut reducer,
                &scene,
                sophia_wm_v1_behavior_cause(scenario).unwrap(),
                29 + u64::try_from(index).unwrap() * 2,
                Vec::new(),
                |reducer, proposal| match scenario {
                    "timeout-discard" => reducer.timeout(proposal.request_id),
                    "stale-discard" => {
                        let successor = sophia_wm_v1_behavior_scene(scenarios[index + 1]).unwrap();
                        reducer.observe_scene(successor).unwrap();
                        reducer.apply_proposal(proposal)
                    }
                    "invalid-discard" => {
                        // Host behaviour: the reducer judges a copy with no
                        // active output. Hagia's own wire proposal was well
                        // formed.
                        let mut invalid = proposal.clone();
                        invalid.active_output = OutputId::from_raw(0);
                        reducer.apply_proposal(&invalid)
                    }
                    _ => reducer.apply_proposal(proposal),
                },
            );
            let expected = match scenario {
                "timeout-discard" => PolicyProjectionOutcome::TimedOut,
                "stale-discard" => PolicyProjectionOutcome::RejectedStale,
                "invalid-discard" => PolicyProjectionOutcome::RejectedInvalid,
                _ => PolicyProjectionOutcome::Committed,
            };
            assert_eq!(outcome, expected, "{scenario}: {proposal:?}");
            if outcome == PolicyProjectionOutcome::Committed {
                let committed = reducer.committed();
                let expected_surfaces = scene
                    .surfaces
                    .iter()
                    .filter(|surface| surface.current_output.is_some())
                    .map(|surface| surface.surface)
                    .collect::<BTreeSet<_>>();
                let committed_surfaces = committed
                    .iter()
                    .flat_map(|output| output.placements.iter())
                    .map(|placement| placement.surface)
                    .collect::<BTreeSet<_>>();
                assert_eq!(committed.len(), scene.outputs.len(), "{scenario}");
                assert_eq!(committed_surfaces, expected_surfaces, "{scenario}");
                assert_eq!(
                    reducer.scene().active_output,
                    scene.active_output,
                    "{scenario}"
                );
            }
            observations.push(ScenarioObservation {
                scenario,
                proposal,
                outcome,
            });
        }
        assert!(wm.supervisor.peer_id().is_some());
        // Deterministic record of exactly what the comparison saw.
        let digest = format!(
            "{:x}",
            Sha256::digest(format!("{selected:#x}\n{observations:#?}").as_bytes())
        );
        writeln!(
            identity,
            "corpus=revision1 scenarios={}\nselected_capabilities={selected:#x}\nobservations_sha256={digest}\nsnapshot_vocabulary=host-exact (no actions, no session operations); configuration catalog admitted separately\nreducer=test-owned canonical; outcomes decided as the conformance host\ninvalid_discard=host reducer judges a copy without an active output, not a malformed Hagia wire\nworker=taken from the configured Session state\nlayout_settlement=false\nnative_settlement=false",
            observations.len()
        )
        .unwrap();
        drop(corpus);
        (selected, observations)
    })
}

#[test]
#[ignore = "requires exact frozen normal Hagia and explicit fresh evidence inputs"]
fn normal_hagia_behavior_corpus_matches_over_current_ipc_and_files() {
    let (ipc_selected, ipc) = run_corpus("corpus-ipc", WmTransportSelection::CurrentIpc);
    let (files_selected, files) = run_corpus("corpus-files", WmTransportSelection::NineP2000L);
    assert_eq!(
        ipc_selected, files_selected,
        "transports admitted different selections"
    );
    assert_eq!(ipc.len(), SOPHIA_WM_V1_BEHAVIOR_SCENARIOS.len());
    assert_eq!(files.len(), ipc.len());
    for (ipc, files) in ipc.iter().zip(&files) {
        assert_eq!(ipc, files, "transport changed scenario {}", ipc.scenario);
    }
    eprintln!(
        "hagia_behavior_corpus_parity scenarios={} selected_capabilities={ipc_selected:#x} observations_sha256={:x} status=pass layout_settlement=false native_settlement=false",
        ipc.len(),
        Sha256::digest(format!("{ipc_selected:#x}\n{ipc:#?}").as_bytes())
    );
}

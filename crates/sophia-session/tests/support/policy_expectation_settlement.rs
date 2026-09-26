use super::*;
use sophia_protocol::{
    PolicyActionRegistration, PolicyOutputProjection, PolicyProjectionOutcome,
    PolicyProjectionProposal, PolicyRequestCause,
};

// A projection outcome expects a session operation only when the projection
// committed and its action names one. The settlement identity comes from the
// real request path; refusals are owner-command units with a supplied layout
// outcome, and prove neither real resize expiry nor layout retention.

const CLOSE: WmActionId = WmActionId::from_raw(31);

struct Staged {
    fixture: ReloadFixture,
    layout: PersistentLiveLayout,
    proposal: LiveWmProposal,
    commands: std::sync::mpsc::Receiver<policy_transport_worker::PolicyTransportCommand>,
}

/// Issues one close-window action through the real cycle and returns the
/// proposal whose settlement identity the request path derived.
fn staged_close_action() -> Staged {
    let mut fixture = ReloadFixture::new();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let (worker, commands, events) = policy_transport_worker::worker_capture::capturing_worker();
    let public = fixture.wm.public.as_mut().unwrap();
    public.worker = Some(worker);
    public.actions = vec![PolicyActionRegistration {
        action: CLOSE,
        name: "close-window".into(),
        session_operation_slot: Some(3),
    }];
    public.queue.push_back(LivePublicPolicyCause {
        source: LiveWmProposalSource::Action(CLOSE),
        cause: PolicyRequestCause::Action {
            activation_serial: 19,
            action: CLOSE,
        },
        affected_outputs: vec![output.id],
    });
    let mut layout = PersistentLiveLayout::default();
    assert!(
        fixture
            .wm
            .poll_request(&mut layout, output, true)
            .unwrap()
            .is_none()
    );
    let policy_transport_worker::PolicyTransportCommand::Cycle { request, .. } =
        commands.try_recv().expect("the action issues a cycle")
    else {
        panic!("the first command is the cycle");
    };
    events
        .send(policy_transport_worker::PolicyTransportEvent::Projection(
            Box::new(PolicyProjectionProposal {
                presentation: None,
                transaction: TransactionId::from_raw(40),
                connection_epoch: request.connection_epoch,
                request_id: request.request_id,
                base_generation: request.scene_generation,
                active_output: output.id,
                outputs: vec![PolicyOutputProjection {
                    output: output.id,
                    placements: vec![],
                    focus: None,
                }],
                launch_contexts: Vec::new(),
                output_launch_contexts: Vec::new(),
                translation_groups: vec![],
                tab_groups: vec![],
                indicators: vec![],
                output_statuses: vec![],
            }),
        ))
        .unwrap();
    let proposal = fixture
        .wm
        .poll_request(&mut layout, output, true)
        .unwrap()
        .expect("the projection stages");
    let settlement = proposal
        .policy_settlement
        .expect("a policy settlement identity");
    assert!(settlement.expect_session_operation);
    assert_eq!(
        fixture.wm.public.as_ref().unwrap().expected_operation_slot,
        Some(3)
    );
    Staged {
        fixture,
        layout,
        proposal,
        commands,
    }
}

fn submitted_outcome(
    commands: &std::sync::mpsc::Receiver<policy_transport_worker::PolicyTransportCommand>,
) -> (PolicyProjectionOutcome, bool) {
    match commands.try_recv().expect("a projection outcome") {
        policy_transport_worker::PolicyTransportCommand::ProjectionOutcome {
            outcome,
            expect_session_operation,
            ..
        } => (outcome, expect_session_operation),
        _ => panic!("the owner submits a projection outcome"),
    }
}

/// Settles the staged close action with a supplied layout outcome, and
/// checks that the refusal expects no session operation and leaves none owed.
fn assert_refusal_expects_no_operation(stale: bool) {
    let Staged {
        mut fixture,
        proposal,
        commands,
        ..
    } = staged_close_action();
    let output = sophia_engine::HeadlessOutput::deterministic();
    if stale {
        // The scene moves before the layout settles, so the staged
        // successor revalidates as stale.
        let public = fixture.wm.public.as_mut().unwrap();
        let mut scene = public.reducer.scene().clone();
        scene.generation += 1;
        public.reducer.observe_scene(scene).unwrap();
    }
    // Supplied layout outcome: a unit control of the owner command only.
    let result = LiveWmCommitResult {
        update: WmTransactionUpdate {
            commit: TransactionCommit {
                transaction: proposal.transaction,
                outcome: TransactionOutcome::TimedOut,
                applied_surfaces: vec![],
            },
        },
        source: Some(LiveWmProposalSource::Action(CLOSE)),
        policy_settlement: proposal.policy_settlement,
    };
    fixture
        .wm
        .apply_commit_result(result, None, output.id)
        .unwrap();
    let expected = if stale {
        PolicyProjectionOutcome::RejectedStale
    } else {
        PolicyProjectionOutcome::TimedOut
    };
    assert_eq!(submitted_outcome(&commands), (expected, false));
    let public = fixture.wm.public.as_ref().unwrap();
    assert_eq!(public.expected_operation_slot, None);
    assert!(public.pending_operation.is_none());
}

#[test]
fn a_timed_out_session_action_expects_no_session_operation() {
    assert_refusal_expects_no_operation(false);
}

#[test]
fn a_stale_session_action_expects_no_session_operation() {
    assert_refusal_expects_no_operation(true);
}

#[test]
fn a_committed_session_action_still_expects_its_session_operation() {
    let Staged {
        mut fixture,
        mut layout,
        proposal,
        commands,
    } = staged_close_action();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let result = layout.commit_proposal(proposal);
    assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
    fixture
        .wm
        .apply_commit_result(result, None, output.id)
        .unwrap();
    assert_eq!(
        submitted_outcome(&commands),
        (PolicyProjectionOutcome::Committed, true)
    );
    assert_eq!(
        fixture.wm.public.as_ref().unwrap().expected_operation_slot,
        Some(3)
    );
}

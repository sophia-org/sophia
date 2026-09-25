use super::*;
#[path = "policy_presentation_routing.rs"]
mod routing;
use sophia_protocol::{
    PolicyPresentation, PolicyPresentationMode, PolicyPresentationOutcome,
    PolicyPresentationOutput, PolicyPresentationRegion, PolicyPresentationRegionRole,
};

fn publication(public: &LivePublicPolicyState) -> PolicyPresentation {
    let output = &public.reducer.scene().outputs[0];
    PolicyPresentation {
        generation: 1,
        keyboard_output: None,
        outputs: vec![PolicyPresentationOutput {
            output: output.output,
            generation: output.generation,
            coverage: output.bounds,
            mode: PolicyPresentationMode::Overlay,
        }],
        instances: vec![],
        bindings: vec![],
        regions: vec![PolicyPresentationRegion {
            id: 1,
            generation: 1,
            output: output.output,
            geometry: output.bounds,
            clip: output.bounds,
            z_index: 0,
            role: PolicyPresentationRegionRole::Backdrop,
            action: Some(WmActionId::from_raw(77)),
        }],
    }
}

fn complete(public: &mut LivePublicPolicyState) -> sophia_protocol::PolicyPresentationReceipt {
    let p = publication(public);
    let output = p.outputs[0];
    public.presentation_input.admit(1, p).unwrap();
    public
        .presentation_input
        .complete(sophia_engine::PolicyPresentationCompletion {
            owner_epoch: 1,
            publication_generation: 1,
            output: output.output,
            output_generation: output.generation,
        })
        .unwrap()
}

#[test]
fn policy_presentation_revocation_survives_full_lifecycle_delivery_queue() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let receipt = complete(public);
    public.presentation_receipts =
        std::iter::repeat_n(receipt, MAX_POLICY_PRESENTATION_RECEIPTS).collect();
    public.revoke_live_presentation();
    assert!(public.presentation_input.publication().is_none());
    assert!(public.presentation_withdrawal_pending);
    assert!(public.transport_unavailable);
    assert_eq!(
        public.presentation_receipts.len(),
        MAX_POLICY_PRESENTATION_RECEIPTS
    );
    assert!(public.presentation_withdrawals.is_empty());
}

#[test]
fn policy_presentation_withdrawal_receipt_requires_an_actual_replacement_frame() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let receipt = complete(public);
    public.retain_presentation_receipts([receipt]);
    public.revoke_live_presentation();
    assert_eq!(public.presentation_receipts.len(), 2);
    let runtime = LiveProductionVisualRuntime::new(&public.outputs, None).unwrap();
    let mut projections = runtime.input_projections().to_vec();
    public.settle_presented_withdrawals(&projections);
    assert_eq!(
        public.presentation_receipts.len(),
        2,
        "no frame cannot attest withdrawal"
    );
    projections[0].frame_completed = true;
    public.settle_presented_withdrawals(&projections);
    assert_eq!(public.presentation_receipts.len(), 3);
    let withdrawn = public.presentation_receipts.back().unwrap();
    assert_eq!(withdrawn.outcome, PolicyPresentationOutcome::Withdrawn);
    assert_eq!(withdrawn.presentation_epoch, receipt.presentation_epoch);
    public.settle_presented_withdrawals(&projections);
    assert_eq!(
        public.presentation_receipts.len(),
        3,
        "terminal receipt is one-shot"
    );
}

#[test]
fn policy_presentation_catalog_refusal_leaves_both_owners_unchanged() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let p = publication(public);
    let output = p.outputs[0].output;
    public.native_presentation_capable = true;
    public
        .actions
        .push(sophia_protocol::PolicyActionRegistration {
            action: WmActionId::from_raw(77),
            name: "opaque-policy-action".into(),
            session_operation_slot: None,
        });
    let request = public
        .reducer
        .issue_request_with_cause(
            vec![output],
            sophia_protocol::PolicyRequestCause::SceneChanged,
        )
        .unwrap();
    let proposal = sophia_protocol::PolicyProjectionProposal {
        transaction: TransactionId::from_raw(700),
        connection_epoch: 1,
        request_id: request.request_id,
        base_generation: request.scene_generation,
        active_output: output,
        presentation: Some(p),
        outputs: vec![sophia_protocol::PolicyOutputProjection {
            output,
            placements: vec![],
            focus: None,
        }],
        launch_contexts: vec![],
        output_launch_contexts: vec![],
        translation_groups: vec![],
        tab_groups: vec![],
        indicators: vec![],
        output_statuses: vec![],
    };
    public.staged = Some(public.reducer.stage_proposal(&proposal).unwrap());
    let previous = public.reducer.committed();
    let serial = public.reducer.commit_serial();
    let runtime = LiveProductionVisualRuntime::new(&public.outputs, None).unwrap();
    assert!(
        !fixture
            .wm
            .preflight_staged_presentation(Some(&runtime), false)
    );
    assert!(
        fixture
            .wm
            .preflight_staged_presentation(Some(&runtime), true)
    );
    fixture.wm.public.as_mut().unwrap().actions.clear();
    assert!(
        !fixture
            .wm
            .preflight_staged_presentation(Some(&runtime), true)
    );
    let public = fixture.wm.public.as_ref().unwrap();
    assert_eq!(public.reducer.committed(), previous);
    assert_eq!(public.reducer.commit_serial(), serial);
    assert!(public.reducer.presentation_publication().is_none());
    assert!(runtime.policy_presentation().is_none());
}

#[test]
fn policy_presentation_consumer_refuses_an_uncompleted_stamp() {
    for was_presented in [false, true] {
        let mut fixture = ReloadFixture::new();
        let public = fixture.wm.public.as_mut().unwrap();
        if was_presented {
            complete(public);
        } else {
            public
                .presentation_input
                .admit(1, publication(public))
                .unwrap();
        }
        let runtime = LiveProductionVisualRuntime::new(&public.outputs, None).unwrap();
        let mut projections = runtime.input_projections().to_vec();
        let output = &public.reducer.scene().outputs[0];
        projections[0].policy_publication =
            Some(sophia_backend_live::LivePresentedPolicyPublication {
                owner_epoch: 1,
                generation: 1,
                output: output.output,
                output_generation: output.generation,
                instances: vec![],
                regions: vec![(1, 1)],
            });
        projections[0].policy_visible = true;
        assert!(!projections[0].frame_completed);
        public.observe_presented_policy(&projections);
        assert!(public.presentation_input.publication().is_none());
        assert!(
            public
                .presentation_receipts
                .iter()
                .all(|receipt| receipt.outcome == PolicyPresentationOutcome::Revoked)
        );
        assert_eq!(
            public.presentation_receipts.len(),
            usize::from(was_presented)
        );
    }
}

#[test]
fn policy_presentation_mixed_heads_shield_both_input_classes_and_delay_withdrawal() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    complete(public);
    public.revoke_live_presentation();
    let runtime = LiveProductionVisualRuntime::new(&public.outputs, None).unwrap();
    let mut projections = runtime.input_projections().to_vec();
    projections[0].policy_visible = true;
    projections[0].frame_completed = false;
    let mut capture = sophia_engine::PolicyInputCapture::default();
    let routing = PolicyPresentedInputRouting {
        state: &public.presentation_input,
        capture: &mut capture,
        protected_actions: vec![],
    };
    assert!(routing.keyboard_needs_shield(Some(&projections)));
    assert_eq!(
        routing.pointer_hit(
            Some(projections[0].output),
            Some(Point { x: 10.0, y: 10.0 }),
            Some(&projections)
        ),
        sophia_engine::PolicyPointerHit::Blocked
    );
    public.settle_presented_withdrawals(&projections);
    assert_eq!(public.presentation_withdrawals.len(), 1);
    assert!(
        public
            .presentation_receipts
            .iter()
            .all(|receipt| receipt.outcome != PolicyPresentationOutcome::Withdrawn)
    );
    projections[0].policy_visible = false;
    projections[0].frame_completed = true;
    public.settle_presented_withdrawals(&projections);
    assert!(public.presentation_withdrawals.is_empty());
    assert_eq!(
        public.presentation_receipts.back().unwrap().outcome,
        PolicyPresentationOutcome::Withdrawn
    );
}

#[test]
fn policy_presentation_reconnect_rejects_old_identity_at_enqueue_and_settlement() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    public.queue.clear();
    public
        .actions
        .push(sophia_protocol::PolicyActionRegistration {
            action: WmActionId::from_raw(77),
            name: "opaque-policy-action".into(),
            session_operation_slot: None,
        });
    let old_receipt = complete(public);
    let point = Point { x: 10.0, y: 10.0 };
    let sophia_engine::PolicyPointerHit::Action(old) = public
        .presentation_input
        .pointer_hit(old_receipt.output, point)
    else {
        panic!("old target missing");
    };
    fixture.wm.enqueue_presented_action(old).unwrap();
    let public = fixture.wm.public.as_mut().unwrap();
    let old_cause = public.queue.pop_back().unwrap().cause;
    assert!(public.presented_cause_is_current(old_cause));
    public.revoke_live_presentation();
    public.connection_epoch = 2;
    let p = publication(public);
    let output = p.outputs[0];
    public.presentation_input.admit(2, p).unwrap();
    let receipt = public
        .presentation_input
        .complete(sophia_engine::PolicyPresentationCompletion {
            owner_epoch: 2,
            publication_generation: 1,
            output: output.output,
            output_generation: output.generation,
        })
        .unwrap();
    public.retain_presentation_receipts([receipt]);
    assert!(receipt.presentation_epoch > old_receipt.presentation_epoch);
    assert!(!public.presented_cause_is_current(old_cause));
    let sophia_engine::PolicyPointerHit::Action(new) =
        public.presentation_input.pointer_hit(output.output, point)
    else {
        panic!("new target missing");
    };
    assert_eq!(
        old.identity.publication_generation,
        new.identity.publication_generation
    );
    assert_eq!(
        old.identity.target_generation,
        new.identity.target_generation
    );
    fixture.wm.enqueue_presented_action(old).unwrap();
    fixture
        .wm
        .enqueue_presented_action(sophia_engine::PresentedPolicyAction {
            connection_epoch: 2,
            ..old
        })
        .unwrap();
    assert!(fixture.wm.public.as_ref().unwrap().queue.is_empty());
    fixture.wm.enqueue_presented_action(new).unwrap();
    let public = fixture.wm.public.as_ref().unwrap();
    assert_eq!(public.queue.len(), 1);
    assert!(public.presented_cause_is_current(public.queue[0].cause));
    assert_eq!(public.queue[0].affected_outputs, vec![output.output]);
}

#[test]
fn policy_replacement_install_waits_for_application_capture_to_settle() {
    check_deferred_replacement(false);
    check_deferred_replacement(true);
}

fn check_deferred_replacement(remove_action_while_deferred: bool) {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let mut p = publication(public);
    p.outputs[0].mode = PolicyPresentationMode::ReplaceApplications;
    p.regions[0].action = None;
    let output = p.outputs[0].output;
    p.keyboard_output = Some(output);
    p.bindings.push(sophia_protocol::PolicyPresentationBinding {
        action: WmActionId::from_raw(77),
        keycode: 28,
        modifiers: sophia_protocol::WmModifierMask { bits: 0 },
    });
    public
        .actions
        .push(sophia_protocol::PolicyActionRegistration {
            action: WmActionId::from_raw(77),
            name: "opaque-policy-action".into(),
            session_operation_slot: None,
        });
    let request = public
        .reducer
        .issue_request_with_cause(
            vec![output],
            sophia_protocol::PolicyRequestCause::SceneChanged,
        )
        .unwrap();
    let proposal = sophia_protocol::PolicyProjectionProposal {
        transaction: TransactionId::from_raw(701),
        connection_epoch: 1,
        request_id: request.request_id,
        base_generation: request.scene_generation,
        active_output: output,
        presentation: Some(p),
        outputs: vec![sophia_protocol::PolicyOutputProjection {
            output,
            placements: vec![],
            focus: None,
        }],
        launch_contexts: vec![],
        output_launch_contexts: vec![],
        translation_groups: vec![],
        tab_groups: vec![],
        indicators: vec![],
        output_statuses: vec![],
    };
    let staged = public.reducer.stage_proposal(&proposal).unwrap();
    public.reducer.commit_staged(staged);
    let committed_serial = public.reducer.commit_serial();
    let mut runtime = LiveProductionVisualRuntime::new(&public.outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(Size {
        width: 100,
        height: 100,
    });
    fixture
        .wm
        .install_committed_policy_presentation(&mut runtime, &scene, true, true, None)
        .unwrap();
    assert!(
        runtime.policy_presentation().is_none(),
        "replacement must not remove the captured application's hit layers"
    );
    assert!(
        fixture
            .wm
            .public
            .as_ref()
            .unwrap()
            .presentation_input
            .publication()
            .is_none()
    );
    assert!(
        fixture
            .wm
            .public
            .as_ref()
            .unwrap()
            .presentation_receipts
            .is_empty()
    );
    if remove_action_while_deferred {
        fixture.wm.public.as_mut().unwrap().actions.clear();
    }
    fixture
        .wm
        .install_committed_policy_presentation(&mut runtime, &scene, true, false, None)
        .unwrap();
    if remove_action_while_deferred {
        let public = fixture.wm.public.as_ref().unwrap();
        assert!(runtime.policy_presentation().is_none());
        assert!(public.reducer.presentation_publication().is_none());
        assert!(public.presentation_input.publication().is_none());
        assert!(public.presentation_receipts.is_empty());
        return;
    }
    assert!(runtime.policy_presentation().is_some());
    let public = fixture.wm.public.as_ref().unwrap();
    assert_eq!(
        public.reducer.commit_serial(),
        committed_serial,
        "capture settlement retries the existing candidate without a new proposal"
    );
    assert!(public.presentation_input.publication().is_some());
    assert!(
        public.presentation_input.output_receipt(output).is_none(),
        "installation alone is not presentation"
    );
}

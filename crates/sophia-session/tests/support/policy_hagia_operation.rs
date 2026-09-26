//! Normal Hagia's committed action through Session's accepted-intent boundary.
//! The returned LaunchApplication tuple stays local: no queue, executor or
//! application launch is invoked. Historical admission, CPU facts and frontend
//! ACKs are supplied as in the parent fixtures; no native receipt is claimed.
use super::*;

#[test]
#[ignore = "requires exact frozen normal Hagia and explicit fresh evidence inputs"]
fn normal_hagia_committed_action_returns_intent_and_answers_next_request() {
    with_normal_hagia(
        "committed-operation",
        |wm, layout, _, output, checkpoint, identity| {
            let peer = wm.supervisor.peer_id();
            retain_existing_surface(layout);
            wm.enqueue_relayout(layout, output).unwrap();
            let baseline = next_proposal(wm, layout, output);
            let mut controls = crate::session_control::SessionControlQueue::default();
            assert!(layout.stage(baseline, &mut controls).unwrap().is_none());
            acknowledge_frontend_controls(layout, &mut controls);
            assert!(!layout.pending_is_ready());
            matching_pixels(layout);
            assert!(layout.pending_is_ready());
            assert!(wm.prepare_public_layout_commit(layout).unwrap());
            let baseline = layout.resolve_pending().unwrap();
            assert_eq!(
                baseline.update.commit.outcome,
                TransactionOutcome::Committed
            );
            assert!(
                wm.apply_commit_result(baseline, None, output.id)
                    .unwrap()
                    .session_action
                    .is_none()
            );
            await_ready(wm, layout, output);
            let (_, baseline_checkpoint_identity) = await_checkpoint(checkpoint);
            assert_managed_baseline(layout);

            let public = wm.public.as_ref().unwrap();
            let operation = public
                .session_operations
                .iter()
                .find(|entry| entry.slot == 1)
                .expect("terminal operation is actually admitted");
            assert!(!operation.permits_surface_target);
            let token = operation.token;
            let expected_action = WmSessionAction::LaunchApplication {
                application: SessionApplicationId::from_raw(1),
            };
            assert_eq!(public.operation_actions[&token], expected_action);
            let action = public
                .actions
                .iter()
                .find(|entry| entry.session_operation_slot == Some(1))
                .expect("configured Hagia advertises the admitted slot")
                .action;
            wm.enqueue_action(action, layout, output).unwrap();
            let projection = next_proposal(wm, layout, output);
            assert_eq!(
                projection.source,
                Some(LiveWmProposalSource::Action(action))
            );
            let projection_identity = projection.policy_settlement.unwrap();
            assert!(projection_identity.expect_session_operation);
            assert!(!projection_identity.session_operation);
            let request = wm
                .public
                .as_ref()
                .unwrap()
                .in_flight_request
                .as_ref()
                .unwrap();
            assert_eq!(request.request_id, projection_identity.request_id);
            let activation_serial = match request.cause {
                PolicyRequestCause::Action {
                    activation_serial,
                    action: actual,
                } => {
                    assert_eq!(actual, action);
                    activation_serial
                }
                ref other => panic!("expected actual action cause, got {other:?}"),
            };
            assert!(activation_serial > 0);
            // Fixture discrimination only: production permits coincidence
            // between these namespaces, but this case must catch substitution.
            assert_ne!(activation_serial, projection_identity.request_id);
            assert_eq!(wm.public.as_ref().unwrap().expected_operation_slot, Some(1));
            assert!(wm.public.as_ref().unwrap().pending_operation.is_none());
            // The baseline already installed this geometry. Do not patch a
            // returned proposal or manufacture a layout result to commit it.
            let result = layout
                .stage(projection, &mut controls)
                .unwrap()
                .expect("unchanged action projection commits without a resize wait");
            assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
            let applied = wm.apply_commit_result(result, None, output.id).unwrap();
            assert!(applied.session_action.is_none());
            // Committed Action projections return this diagnostic marker;
            // the production owner logs it, independently of executor custody.
            assert_eq!(applied.physical_action, Some(action));
            let public = wm.public.as_ref().unwrap();
            assert!(public.staged.is_none());
            assert!(public.pending_operation.is_none());
            assert_eq!(public.expected_operation_slot, Some(1));
            assert!(layout.pending.is_none());
            assert_managed_baseline(layout);
            let retained = layout.layers.clone();
            let committed_projection = wm.public.as_ref().unwrap().reducer.committed();

            // This is the real driver event and public owner's validated
            // operation proposal, not a scripted request or outcome.
            let deadline = Instant::now() + Duration::from_secs(5);
            let operation = loop {
                if let Some(proposal) = wm.poll_public_request(layout, output, false).unwrap() {
                    break proposal;
                }
                assert!(!wm.degraded && !wm.force_transport_restart);
                assert_eq!(wm.supervisor.peer_id(), peer);
                assert!(Instant::now() < deadline, "normal Hagia operation deadline");
                std::thread::sleep(Duration::from_millis(2));
            };
            let operation_identity = operation.policy_settlement.unwrap();
            assert!(operation_identity.session_operation);
            assert!(!operation_identity.expect_session_operation);
            assert_eq!(
                operation_identity.connection_epoch,
                projection_identity.connection_epoch
            );
            assert_eq!(operation_identity.connection_epoch, 1);
            assert!(operation_identity.transaction.raw() > projection_identity.transaction.raw());
            // Activation serial, projection request ID and operation domain
            // transaction are separate namespaces; numeric coincidence is valid.
            assert_eq!(operation_identity.request_id, activation_serial);
            let public = wm.public.as_ref().unwrap();
            let (transaction, request) = public.pending_operation.as_ref().unwrap();
            assert_eq!(*transaction, operation_identity.transaction);
            assert_eq!(request.connection_epoch, 1);
            assert_eq!(request.request_id, activation_serial);
            assert_eq!(request.operation, token);
            assert_eq!(request.target, None);
            assert!(public.expected_operation_slot.is_none());
            assert_eq!(
                operation.layers,
                retained.values().cloned().collect::<Vec<_>>()
            );
            assert!(operation.requested_sizes.is_empty());
            assert!(operation.presentation_states.is_empty());
            assert_eq!(operation.focus, None);
            // Hagia saves the committed projection before sending this operation.
            // Waiting for Ready alone would not prove that peer-side ordering.
            let checkpoint_before_operation = await_checkpoint(checkpoint);
            assert_ne!(checkpoint_before_operation.1, baseline_checkpoint_identity);
            let result = layout
                .stage(operation, &mut controls)
                .unwrap()
                .expect("actual unchanged operation proposal has no resize obligation");
            assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
            let accepted = wm.apply_commit_result(result, None, output.id).unwrap();
            let retained_intent = accepted
                .session_action
                .expect("owner accepted the typed intent");
            assert_eq!(
                retained_intent,
                (operation_identity.transaction, expected_action, None)
            );
            assert!(accepted.physical_action.is_none());
            assert!(wm.public.as_ref().unwrap().pending_operation.is_none());
            assert_eq!(layout.layers, retained);
            assert_eq!(
                wm.public.as_ref().unwrap().reducer.committed(),
                committed_projection
            );
            assert_managed_baseline(layout);

            // The production acceptance outcome releases Hagia's operation wait.
            // Keep the intent local rather than queueing or executing it.
            wm.enqueue_relayout(layout, output).unwrap();
            let next = next_proposal(wm, layout, output);
            let next_identity = next.policy_settlement.unwrap();
            assert!(!next_identity.session_operation);
            assert!(next_identity.request_id > projection_identity.request_id);
            assert!(next_identity.transaction.raw() > operation_identity.transaction.raw());
            assert_eq!(next_identity.connection_epoch, 1);
            assert_eq!(wm.supervisor.peer_id(), peer);
            assert_eq!(await_checkpoint(checkpoint), checkpoint_before_operation);
            writeln!(identity,
                "operation=accepted_typed_intent\nactivation_serial={activation_serial}\nprojection_request_id={}\nprojection_domain_transaction={}\noperation_domain_transaction={}\noperation_token={token}\nnext_request_id={}\nnext_domain_transaction={}\nintent={retained_intent:?}\nexecutor_called=false\napplication_success=unproven\ncheckpoint_before_operation=true\nsame_child=true\nnative_receipt=false\nwhole_owner_loop=false\ncontinuation=proposal_only_not_second_commit",
                projection_identity.request_id, projection_identity.transaction.raw(),
                operation_identity.transaction.raw(), next_identity.request_id,
                next_identity.transaction.raw()).unwrap();
        },
    );
}

//! M4 uses public Session ownership and real private sockets. The dedicated
//! xtask runner supplies isolation, deadlines and attested executables.
#![cfg(unix)]

#[path = "support/private_input_acceptance/mod.rs"]
mod support;

use sophia_session::private_input::{
    PrivateInputGrantPolicy, PrivateInputIssueRefusal, PrivateInputReadiness, PrivateInputService,
};
use support::{Evidence, Instance, Order, Peer, config};

#[test]
#[ignore = "run through cargo xtask check m4-acceptance"]
fn containment() {
    support::containment::containment();
}

#[test]
#[ignore = "run through cargo xtask check m4-acceptance"]
fn no_ambient_fallback() {
    support::containment::no_ambient_fallback();
}

#[test]
#[ignore = "run through cargo xtask check m4-acceptance"]
fn construction() {
    let mut evidence = Evidence::default();
    let instance = Instance::start(PrivateInputGrantPolicy::Disabled);
    assert_eq!(instance.handle().readiness(), PrivateInputReadiness::Ready);
    assert_eq!(instance.handle().socket_path(), instance.socket());
    for order in [Order::Little, Order::Big] {
        let peer = Peer::connect(instance.handle().socket_path(), order, None).unwrap();
        assert_eq!(peer.root_size(), (320, 240));
        drop(peer);
    }
    // A second production owner must not displace the live listener.
    let second =
        PrivateInputService::start(config(instance.socket(), PrivateInputGrantPolicy::Disabled))
            .expect("the valid second configuration must reach the listener bind");
    assert_eq!(
        second.await_ready(support::WAIT).unwrap(),
        PrivateInputReadiness::Stopped
    );
    let refused = second.stop();
    match refused.failure.as_ref() {
        Some(sophia_x_authority::PrivateServiceFailure::Failed { error, .. }) => {
            assert!(error.to_string().contains("binds exclusively"), "{error}");
        }
        other => panic!("expected the occupied listener refusal, got {other:?}"),
    }
    evidence.collect(refused, true);
    // Verify that the first owner still serves after the refused bind.
    drop(Peer::connect(instance.socket(), Order::Little, None).unwrap());
    evidence.collect(instance.finish(), false);
    evidence.emit(
        "construction",
        &[
            "default_build",
            "explicit_topology",
            "bind_refusal",
            "ready_after_preparation",
        ],
    );
}

#[test]
#[ignore = "run through cargo xtask check m4-acceptance"]
fn authorization() {
    let mut evidence = Evidence::default();
    for order in [Order::Little, Order::Big] {
        let instance = Instance::start(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
        let (ordinary, ordinary_context) = instance.connect(order, None);
        assert!(matches!(
            instance
                .handle()
                .issue(ordinary_context, support::device(1)),
            Err(PrivateInputIssueRefusal::NotInstanceVerified)
        ));
        let (authenticated, context) = instance.connect(order, Some(support::COOKIE));
        let submission = instance
            .handle()
            .issue(context, support::device(2))
            .unwrap();
        assert_eq!(submission.connection().admission, context.client_id);
        let before = instance.handle().admitted().unwrap().len();
        let mut wrong = support::COOKIE;
        wrong[31] ^= 1;
        assert!(Peer::connect(instance.socket(), order, Some(wrong)).is_err());
        assert_eq!(instance.handle().admitted().unwrap().len(), before);
        drop((ordinary, authenticated, submission));
        evidence.collect(instance.finish(), false);

        let disabled = Instance::start(PrivateInputGrantPolicy::Disabled);
        let (peer, context) = disabled.connect(order, Some(support::COOKIE));
        assert!(matches!(
            disabled.handle().issue(context, support::device(1)),
            Err(PrivateInputIssueRefusal::GrantsDisabled)
        ));
        drop(peer);
        evidence.collect(disabled.finish(), false);

        let foreign = Instance::start_foreign_evidence().unwrap();
        let (peer, context) = foreign.connect(order, Some(support::COOKIE));
        assert!(matches!(
            foreign.handle().issue(context, support::device(1)),
            Err(PrivateInputIssueRefusal::NotInstanceVerified)
        ));
        drop(peer);
        evidence.collect(foreign.finish(), false);
    }
    evidence.emit(
        "authorization",
        &[
            "authenticated_enabled",
            "disabled",
            "ordinary_recipient",
            "wrong_credentials",
            "foreign_instance",
        ],
    );
}

#[test]
#[ignore = "run through cargo xtask check m4-acceptance"]
fn connection_identity() {
    use sophia_protocol::SurfaceId;
    use sophia_session::private_input::PrivateInputSubmitError;
    use sophia_x_authority::PrivateSendError;
    use std::time::{Duration, Instant};

    let instance = Instance::start(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let (first, first_context) = instance.connect(Order::Little, Some(support::COOKIE));
    let (second, second_context) = instance.connect(Order::Big, Some(support::COOKIE));
    let first_input = instance
        .handle()
        .issue(first_context, support::device(11))
        .unwrap();
    let second_input = instance
        .handle()
        .issue(second_context, support::device(12))
        .unwrap();
    assert_ne!(first_context.client_id, second_context.client_id);
    assert_ne!(
        first_input.connection().client,
        second_input.connection().client
    );
    assert_eq!(first_input.device(), support::device(11));
    assert_eq!(second_input.device(), support::device(12));
    for input in [&first_input, &second_input] {
        let actual = instance
            .handle()
            .admitted()
            .unwrap()
            .into_iter()
            .find(|row| row.admission == input.connection().admission)
            .unwrap();
        assert_eq!(input.connection().client, actual.client);
        assert_eq!(
            input.connection().connection_generation,
            actual.connection_generation
        );
    }
    let revoked = instance.handle().revoke(first_context).unwrap();
    assert_eq!(revoked, first_input.connection());
    assert!(matches!(
        instance.handle().issue(first_context, support::device(13)),
        Err(PrivateInputIssueRefusal::ConnectionGone)
    ));
    // Revocation must reject before consulting a target or taking queue space.
    // This deliberately unbound surface is not a delivery or applied-state fixture.
    assert!(matches!(
        first_input.submit_key(SurfaceId::INVALID, 42, true),
        Err(PrivateInputSubmitError::Refused(PrivateSendError::Denied(
            _
        )))
    ));
    assert!(
        instance
            .handle()
            .admitted()
            .unwrap()
            .iter()
            .any(|row| row.admission == second_context.client_id
                && !row.closed
                && row.lifecycle_open)
    );
    drop(first);
    let deadline = Instant::now() + support::WAIT;
    while instance
        .handle()
        .admission_record(first_context.client_id)
        .unwrap()
        .is_some()
    {
        assert!(
            Instant::now() < deadline,
            "departed admission stayed current"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let (replacement, replacement_context) = instance.connect(Order::Little, Some(support::COOKIE));
    let replacement_input = instance
        .handle()
        .issue(replacement_context, support::device(11))
        .unwrap();
    assert_ne!(replacement_context.client_id, first_context.client_id);
    assert_ne!(replacement_input.connection(), first_input.connection());
    assert!(matches!(
        first_input.submit_key(SurfaceId::INVALID, 42, true),
        Err(PrivateInputSubmitError::Refused(PrivateSendError::Denied(
            _
        )))
    ));
    assert!(matches!(
        instance.handle().issue(first_context, support::device(14)),
        Err(PrivateInputIssueRefusal::UnknownAdmission)
    ));
    drop((
        replacement,
        second,
        first_input,
        second_input,
        replacement_input,
    ));
    let mut evidence = Evidence::default();
    evidence.collect(instance.finish(), false);
    evidence.emit(
        "connection_identity",
        &[
            "independent_callers",
            "revoked_admission",
            "reconnect",
            "stale_handle",
        ],
    );
}

#[test]
#[ignore = "run through cargo xtask check m4-acceptance"]
fn committed_routing() {
    use sophia_protocol::Rect;
    use sophia_session::private_input::{PrivateInputAccepted, PrivateInputAction};
    use sophia_x_authority::{
        XAuthorityControlKind, XAuthorityControlOutcome, XAuthorityInputDeliveryOutcome,
    };
    use std::time::{Duration, Instant};

    let mut evidence = Evidence::default();
    for order in [Order::Little, Order::Big] {
        let mut instance = Instance::start(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
        let (mut peer, context) = instance.connect(order, Some(support::COOKIE));
        let submission = instance
            .handle()
            .issue(context, support::device(1))
            .unwrap();
        let window = peer.create_map_and_draw();
        let deadline = Instant::now() + support::WAIT;
        let admitted = loop {
            let committed = instance
                .handle_mut()
                .apply_committed(Duration::from_millis(10))
                .unwrap();
            assert!(committed.refused.is_empty(), "{committed:?}");
            let admission = committed
                .effects
                .iter()
                .find(|effect| effect.kind() == XAuthorityControlKind::AdmitSurface);
            if let Some(admission) = admission {
                assert!(
                    committed.committed > 0,
                    "an observation alone is not a commit"
                );
                assert_eq!(
                    admission.geometry(),
                    Some(Rect {
                        x: 0,
                        y: 0,
                        width: 8,
                        height: 8
                    })
                );
                assert!(admission.committed_transaction().is_valid());
                break *admission;
            }
            assert!(
                Instant::now() < deadline,
                "real draw never committed and admitted"
            );
        };
        let mapped = admitted
            .submitted()
            .expect("committed admission reached the real order");
        let focused = instance
            .handle()
            .submit_action(
                submission.connection(),
                PrivateInputAction::FocusSurface {
                    surface: admitted.surface(),
                },
            )
            .unwrap();
        let mut acknowledgements = Vec::new();
        while acknowledgements.len() < 2 {
            acknowledgements.extend(
                instance
                    .handle()
                    .drain_acknowledgements_within(Duration::from_millis(10)),
            );
            assert!(
                Instant::now() < deadline,
                "control acknowledgement deadline"
            );
        }
        assert_eq!(acknowledgements.len(), 2);
        for expected in [mapped, focused] {
            assert_eq!(
                acknowledgements
                    .iter()
                    .filter(|ack| ack.client == submission.connection().client
                        && ack.acknowledgement.transaction == expected.transaction
                        && ack.acknowledgement.surface == expected.surface
                        && ack.acknowledgement.kind == expected.kind
                        && ack.acknowledgement.outcome == XAuthorityControlOutcome::Delivered)
                    .count(),
                1
            );
        }
        peer.focus_event(window);
        for (key, pressed, kind, detail, state) in [
            (false, true, 4, 1, 0),
            (false, false, 5, 1, 1 << 8),
            (true, true, 2, 50, 0),
            (true, false, 3, 50, 1),
        ] {
            let accepted: PrivateInputAccepted = if key {
                submission
                    .submit_key(admitted.surface(), 42, pressed)
                    .unwrap()
            } else {
                submission
                    .submit_pointer_button(admitted.surface(), 272, pressed)
                    .unwrap()
            };
            peer.input_event(
                window,
                kind,
                detail,
                u32::try_from(accepted.time_msec).unwrap(),
                state,
            );
            let receipts = instance.handle().drain_deliveries_within(support::WAIT);
            assert_eq!(receipts.len(), 1);
            assert_eq!(receipts[0].client, submission.connection().client);
            assert_eq!(receipts[0].delivery, accepted.delivery);
            assert_eq!(receipts[0].outcome, XAuthorityInputDeliveryOutcome::Flushed);
        }
        peer.empty_tail();
        assert!(instance.handle().drain_deliveries().is_empty());
        drop((peer, submission));
        evidence.collect(instance.finish(), false);
    }
    evidence.emit(
        "committed_routing",
        &[
            "real_surface_commit",
            "focus_acknowledged",
            "little_endian",
            "big_endian",
            "receipt_identity",
        ],
    );
}

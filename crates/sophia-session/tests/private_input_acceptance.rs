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
        PrivateInputService::start(config(instance.socket(), PrivateInputGrantPolicy::Disabled));
    if let Ok(second) = second {
        assert_ne!(
            second.await_ready(support::WAIT).unwrap(),
            PrivateInputReadiness::Ready
        );
        evidence.collect(second.stop(), true);
    }
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

        let foreign = Instance::start_foreign_evidence();
        match foreign {
            Ok(foreign) => {
                let (peer, context) = foreign.connect(order, Some(support::COOKIE));
                assert!(matches!(
                    foreign.handle().issue(context, support::device(1)),
                    Err(PrivateInputIssueRefusal::NotInstanceVerified)
                ));
                drop(peer);
                evidence.collect(foreign.finish(), false);
            }
            Err(_) => {} // Construction itself may reject the foreign binding.
        }
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
fn committed_routing() {
    use sophia_protocol::Rect;
    use sophia_session::private_input::{PrivateInputAccepted, PrivateInputAction};
    use sophia_x_authority::{
        XAuthorityControlKind, XAuthorityControlOutcome, XAuthorityInputDeliveryOutcome,
    };
    use std::time::{Duration, Instant};

    let mut evidence = Evidence::default();
    for order in [Order::Little, Order::Big] {
        let instance = Instance::start(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
        let (mut peer, context) = instance.connect(order, Some(support::COOKIE));
        let submission = instance
            .handle()
            .issue(context, support::device(1))
            .unwrap();
        let window = peer.create_map_and_draw();
        let deadline = Instant::now() + support::WAIT;
        let admitted = loop {
            let committed = instance
                .handle()
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

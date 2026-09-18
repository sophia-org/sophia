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

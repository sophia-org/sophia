//! Real wire mapping edges around already committed pixels.

use super::{Instance, Order, WAIT};
use sophia_protocol::{SurfaceId, TransactionOutcome};
use sophia_session::private_input::{
    PrivateInputCommittedEffect, PrivateInputGrantPolicy, PrivateInputOutcome,
};
use sophia_x_authority::{XAuthorityControlKind, XAuthorityControlOutcome};
use std::time::{Duration, Instant};

pub fn exercise(order: Order) -> PrivateInputOutcome {
    let mut instance = Instance::start(PrivateInputGrantPolicy::EnabledWithVerifiedEvidence);
    let (mut peer, context) = instance.connect(order, Some(super::COOKIE));
    let window = peer.create_unmapped();
    peer.draw(window);
    peer.confirm_geometry(window);
    let deadline = Instant::now() + WAIT;
    let surface = loop {
        let report = instance
            .handle_mut()
            .apply_committed(Duration::from_millis(10))
            .unwrap();
        assert!(report.refused.is_empty(), "{report:?}");
        assert!(
            report.effects.is_empty(),
            "unmapped pixels gained an input effect: {report:?}"
        );
        let applied = report
            .outcomes
            .iter()
            .find(|outcome| !outcome.applied.is_empty());
        if let Some(outcome) = applied {
            assert_eq!(outcome.outcome, TransactionOutcome::Committed);
            assert_eq!(outcome.applied.len(), 1);
            assert!(outcome.mapped.is_empty());
            break outcome.applied[0];
        }
        assert!(
            Instant::now() < deadline,
            "unmapped draw did not commit: {report:?}"
        );
    };
    assert!(instance.handle().drain_acknowledgements().is_empty());
    peer.empty_tail();

    // A mapping request uses those committed pixels without asking the client
    // to draw them a second time. Subsequent drawing configures the same map.
    peer.map(window);
    peer.confirm_geometry(window);
    let admitted = effect(&mut instance, surface, XAuthorityControlKind::AdmitSurface);
    acknowledge(&instance, context.client_id, admitted);
    peer.draw(window);
    peer.confirm_geometry(window);
    let configured = effect(
        &mut instance,
        surface,
        XAuthorityControlKind::ConfigureSurface,
    );
    assert_ne!(
        configured.committed_transaction(),
        admitted.committed_transaction()
    );
    acknowledge(&instance, context.client_id, configured);

    peer.unmap(window);
    peer.confirm_geometry(window);
    let withdrawn = effect(
        &mut instance,
        surface,
        XAuthorityControlKind::WithdrawSurface,
    );
    acknowledge(&instance, context.client_id, withdrawn);
    peer.map(window);
    peer.confirm_geometry(window);
    let remapped = effect(&mut instance, surface, XAuthorityControlKind::AdmitSurface);
    assert_ne!(remapped.submitted(), admitted.submitted());
    acknowledge(&instance, context.client_id, remapped);
    peer.empty_tail();
    drop(peer);
    instance.finish()
}

fn effect(
    instance: &mut Instance,
    surface: SurfaceId,
    kind: XAuthorityControlKind,
) -> PrivateInputCommittedEffect {
    let deadline = Instant::now() + WAIT;
    loop {
        let report = instance
            .handle_mut()
            .apply_committed(Duration::from_millis(10))
            .unwrap();
        assert!(report.refused.is_empty(), "{report:?}");
        if let [effect] = report.effects.as_slice() {
            assert_eq!(effect.surface(), surface);
            assert_eq!(effect.kind(), kind);
            return *effect;
        }
        assert!(
            report.effects.is_empty(),
            "duplicate mapping effect: {report:?}"
        );
        assert!(Instant::now() < deadline, "missing {kind:?}: {report:?}");
    }
}

fn acknowledge(
    instance: &Instance,
    admission: sophia_protocol::ClientAdmissionId,
    effect: PrivateInputCommittedEffect,
) {
    let submitted = effect.submitted().expect("effect transferred to the order");
    let rows = instance.handle().admitted().unwrap();
    let owner = rows.iter().find(|row| row.admission == admission).unwrap();
    let receipts = instance.handle().drain_acknowledgements_within(WAIT);
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    let receipt = receipts[0];
    assert_eq!(receipt.client, owner.client);
    assert_eq!(receipt.acknowledgement.transaction, submitted.transaction);
    assert_eq!(receipt.acknowledgement.surface, submitted.surface);
    assert_eq!(receipt.acknowledgement.kind, submitted.kind);
    assert_eq!(
        receipt.acknowledgement.outcome,
        XAuthorityControlOutcome::Delivered
    );
}

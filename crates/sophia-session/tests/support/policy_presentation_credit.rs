//! Production Session routing and receipt delivery under actual journal pressure.
//! The peer is scripted; completed presentation facts and input are supplied.
use super::*;
use crate::live_session::policy_transport_worker::ninep::receipt_credit_peer;
use sophia_protocol::{PolicyPresentationReceipt, wm_files::WM_FILE_SEND_TIMEOUT_MILLIS};
use std::time::{Duration, Instant};

#[test]
fn journal_credit_does_not_block_local_revocation_or_release() {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    let (worker, mut peer) = receipt_credit_peer::configured(public.connection_epoch);
    public.worker = Some(worker);
    let mut projections = presented(public);
    assert_eq!(public.presentation_receipts.len(), 1);
    let receipt = *public.presentation_receipts.front().unwrap();
    assert!(public.flush_presentation_receipts().unwrap());
    peer.acknowledge_receipt(receipt);
    let report = route(public, &projections, &[key(28, true)], None);
    assert_eq!(actions(&report).len(), 1);

    let blocked_since = peer.exhaust_record_credit(public.worker.as_ref().unwrap(), receipt);
    projections[0].policy_publication = None;
    public.observe_presented_policy(&projections);
    assert!(
        public.presentation_input.publication().is_none(),
        "journal credit must not delay local presentation revocation"
    );
    let revoked = PolicyPresentationReceipt {
        outcome: PolicyPresentationOutcome::Revoked,
        ..receipt
    };
    assert_eq!(public.presentation_receipts, VecDeque::from([revoked]));

    // Remove the visible-policy shield so only the captured release debt can
    // swallow the release. A duplicate must reach the empty-focus path.
    projections[0].policy_visible = false;
    let report = route(public, &projections, &[key(28, false)], None);
    assert!(actions(&report).is_empty());
    assert_eq!(
        report.keys_suppressed_no_focus, 0,
        "owed release must be swallowed under journal pressure"
    );
    let duplicate = route(public, &projections, &[key(28, false)], None);
    assert_eq!(
        duplicate.keys_suppressed_no_focus, 1,
        "release debt must retire under journal pressure"
    );

    let output = public.outputs[0];
    let epoch = public.connection_epoch;
    let mut layout = PersistentLiveLayout::default();
    let started = Instant::now();
    assert!(
        fixture
            .wm
            .poll_public_request(&mut layout, output, false)
            .unwrap()
            .is_none()
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "Session poll must not wait for journal credit"
    );
    let public = fixture.wm.public.as_ref().unwrap();
    assert_eq!(
        public.presentation_receipts,
        VecDeque::from([revoked]),
        "full worker slot must retain the exact Revoked receipt"
    );
    assert!(!fixture.wm.force_transport_restart && !public.transport_unavailable);

    // No ACK is ever sent after saturation. The production send budget must
    // fail closed; this is a liveness bound, not a desktop latency budget.
    let deadline = blocked_since
        + Duration::from_millis(WM_FILE_SEND_TIMEOUT_MILLIS as u64)
        + Duration::from_secs(2);
    while !fixture.wm.force_transport_restart {
        assert!(
            Instant::now() < deadline,
            "exhausted journal credit fails closed within the send deadline"
        );
        assert!(
            fixture
                .wm
                .poll_public_request(&mut layout, output, false)
                .unwrap()
                .is_none()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        blocked_since.elapsed() >= Duration::from_millis(WM_FILE_SEND_TIMEOUT_MILLIS as u64 - 100),
        "journal pressure must fail at the send deadline, not an earlier transport error"
    );
    assert_eq!(fixture.wm.public.as_ref().unwrap().connection_epoch, epoch);
    // Dropping the worker also joins its reactor thread; no peer process exists.
    fixture.wm.public.as_mut().unwrap().worker.take();
}

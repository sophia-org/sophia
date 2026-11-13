//! G3-5d d-0: the capture recheck in the accept's shared consumption guards,
//! run both as G3-5c's check before any classification (`check`) and as the
//! final preflight. Each capture source is set after the held result was
//! reserved and settled, so reserve's eligibility never saw it.
//!
//! The owner's endpoint is a real `CaptureOwner` read through its own
//! `may_capture`, as `combined_shell_capture` forwards it in production; the
//! queue's reservation comes from its real `request_capture`; the exporter's
//! flags from its real request and discard, except the labelled
//! reservation-only state, which no production path sets alone.

use super::*;
use crate::production_session::native_scanout::persistent_native_scanout::shell_capture::{
    CaptureOwner, tests::combined_capture_predicate as owner,
};

/// `owner` becomes the endpoint the guards read.
fn endpoint(f: &mut Fixture, owner: CaptureOwner) {
    f.capture = Box::new(move |output| owner.may_capture(output));
}

/// Queues on head `index`'s output a generation that owes no retirement,
/// then reserves its capture through the queue's own request.
fn queue_capture(f: &mut Fixture, index: usize) {
    let head = &f.heads[index];
    let output = head.output.id;
    let frame = LiveProductionNativeFrameId::from_raw(FRAME - 1);
    let generation = LiveProductionQueuedMirrorGeneration {
        output,
        frame,
        logical_content_checksum: Some(frame.raw()),
        heads: vec![LiveProductionQueuedMirrorHeadFrame {
            capture_reservation: None,
            head_index: index,
            identity: f
                .native_owner
                .frame(output, head.head, head.target_generation, frame.raw()),
            content: content(frame.raw()),
            frame: Default::default(),
            output_damage_snapshot: Some(snapshot(head.output)),
            cpu_nonzero_pixel_bytes: 0,
        }],
    };
    let configured = f.heads.iter().map(|head| head.output.id).collect();
    assert!(
        f.deferred
            .admit_batch(vec![generation], &configured)
            .is_ok()
    );
    assert!(!f.deferred.protected(output), "it owes no retirement");
    assert!(
        f.deferred.request_capture(output, frame),
        "the queue reserves"
    );
}

/// The exporter state preview recovery's discard leaves (REV2 Q6): an
/// identified ordinary frame queued on head `index` after reserve, its
/// capture requested, then the queued frame discarded with both flags kept.
/// While the frame is queued, OrdinaryPending answers first.
fn capture_left_by_discard(f: &mut Fixture, index: usize, accept: &CombinedAccept) {
    let head = &f.heads[index];
    let identity =
        f.native_owner
            .frame(head.output.id, head.head, head.target_generation, FRAME + 1);
    let frame = mixed(head);
    let reservation = sophia_renderer_live::LiveSnapshotReservation::for_image(16, 16).unwrap();
    let exporter = &mut f.exporters[index];
    exporter.set_pending_identified_mixed_frame(frame, Some(identity));
    assert!(exporter.request_pending_frame_capture(identity, reservation));
    if index == 0 {
        assert_eq!(
            f.check(accept).err(),
            Some(CombinedAcceptRefusal::OrdinaryPending),
            "a queued ordinary frame keeps its precedence"
        );
    }
    let exporter = &mut f.exporters[index];
    assert!(exporter.discard_pending_frame(), "the queued frame goes");
    assert!(!exporter.pending_frame());
    assert!(exporter.combined_capture_pending(), "the flags stay");
}

/// One capture case on output 1: the pre-classification check and the
/// preflight both answer CaptureActive and nothing moves (`refused`).
fn capture_refused(name: &str, set: impl FnOnce(&mut Fixture, &CombinedAccept)) {
    refused(name, CombinedAcceptRefusal::CaptureActive, |f, accept| {
        assert!(f.check(accept).is_ok(), "{name}: acceptable before");
        set(f, accept);
        assert_eq!(
            f.check(accept).err(),
            Some(CombinedAcceptRefusal::CaptureActive),
            "{name}: the check before classification refuses too"
        );
    });
}

#[test]
fn a_capture_acquired_after_reserve_refuses_at_both_boundaries() {
    capture_refused("owner pending install", |f, _| {
        endpoint(f, owner::pending_on(1));
    });
    capture_refused("owner active install", |f, _| {
        endpoint(f, owner::active_on(1));
    });
    capture_refused("queued generation's reservation", |f, _| {
        queue_capture(f, 0);
    });
    capture_refused("discarded frame's capture", |f, accept| {
        capture_left_by_discard(f, 0, accept);
    });
    // Labelled: G3-5b R4's declared state, a reservation with no request.
    capture_refused("exporter reservation only", |f, _| {
        f.exporters[0].reserve_combined_capture_for_control();
    });
}

#[test]
fn a_closing_owner_refuses_every_output() {
    capture_refused("closing owner with an install elsewhere", |f, _| {
        endpoint(f, owner::closing_on(2));
    });
}

#[test]
fn a_capture_on_another_output_does_not_refuse_this_one() {
    let mut f = Fixture::held();
    let accept = f.accept();
    endpoint(&mut f, owner::active_on(2));
    queue_capture(&mut f, 1);
    capture_left_by_discard(&mut f, 1, &accept);
    assert_eq!(f.check(&accept), Ok(0), "output 2's captures are its own");
    let plan = f
        .preflight(accept)
        .expect("output 1's result is acceptable");
    f.commit(plan);
    assert!(f.exporters[0].worker_in_flight(), "the accept committed");
}

#[test]
fn earlier_refusals_keep_their_precedence_over_a_capture() {
    use CombinedAcceptRefusal as R;
    let captured = |f: &mut Fixture| endpoint(f, owner::active_on(1));
    // As the G3-4 issuer control: the owner is restored before the held
    // result's disposal, which only the issuing owner can release.
    let mut f = Fixture::held();
    captured(&mut f);
    let accept = f.accept();
    let before = f.observed();
    let issuing = f.native_owner;
    f.native_owner = crate::NativeFrameOwner::new();
    assert_eq!(
        f.check(&accept).err(),
        Some(R::NotIssuing),
        "capture and a rebuilt owner: the check"
    );
    assert_eq!(
        f.preflight(accept).err(),
        Some(R::NotIssuing),
        "capture and a rebuilt owner"
    );
    f.native_owner = issuing;
    assert!(
        f.observed() == before,
        "capture and a rebuilt owner: nothing moved"
    );
    assert_eq!(f.stub.releases(), 0);
    assert_eq!(f.release(), CombinedRelease::Transferred);
    assert_eq!(f.stub.releases(), 1);
    refused("capture and a stale job", R::NotHeld, |f, accept| {
        captured(f);
        accept.job += 1;
    });
    refused("capture and a queued frame", R::OrdinaryPending, |f, _| {
        captured(f);
        let frame = mixed(&f.heads[0]);
        f.exporters[0].set_pending_mixed_frame(frame);
    });
    refused("capture and a disabled head", R::Topology, |f, _| {
        captured(f);
        f.heads[0].enabled = false;
    });
    refused("capture and a stale target", R::TargetGeneration, |f, _| {
        captured(f);
        f.heads[0].target_generation += 1;
    });
    refused(
        "capture and pending content",
        R::HeadPendingContent,
        |f, _| {
            captured(f);
            f.heads[0].pending_content = Some(content(99));
        },
    );
    refused(
        "capture and a protected queue",
        R::RetirementProtected,
        |f, _| {
            captured(f);
            f.queue_protected(0);
        },
    );
    refused("capture and another identity", R::Identity, |f, accept| {
        captured(f);
        accept.frame = LiveProductionNativeFrameId::from_raw(FRAME + 1);
    });
    refused(
        "capture and a rendering frame",
        R::RenderingTransition,
        |f, _| {
            captured(f);
            let output = f.heads[0].output;
            f.heads[0].output_frames.queue(snapshot(output)).unwrap();
            f.heads[0].output_frames.mark_rendering().unwrap();
        },
    );
}

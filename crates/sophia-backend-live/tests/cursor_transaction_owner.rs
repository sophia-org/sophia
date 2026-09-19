#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

//! The rules the cursor transaction owner follows, and why each exists.

use sophia_backend_live::{
    CursorCommitPlan, HardwareCursorPath, LegacyHardwareCursorAdmission,
    LibdrmNativeCursorPlacement, cursor_only_quiet, plan_cursor_commit, settle_pending_cursor,
};

fn placement(x: i32) -> LibdrmNativeCursorPlacement {
    LibdrmNativeCursorPlacement {
        framebuffer: drm::control::from_u32(7).unwrap(),
        x,
        y: 100,
        width: 64,
        height: 64,
    }
}

/// Every case below a quiet client, which is the state these rules were
/// written for: a cursor-only commit is the answer only when nothing else
/// needs the CRTC.
fn plan(
    admission: LegacyHardwareCursorAdmission,
    pending: Option<Option<LibdrmNativeCursorPlacement>>,
    committed: Option<LibdrmNativeCursorPlacement>,
    primary_going_out: bool,
) -> CursorCommitPlan {
    plan_while(admission, pending, committed, primary_going_out, true)
}

fn plan_while(
    admission: LegacyHardwareCursorAdmission,
    pending: Option<Option<LibdrmNativeCursorPlacement>>,
    committed: Option<LibdrmNativeCursorPlacement>,
    primary_going_out: bool,
    client_quiet: bool,
) -> CursorCommitPlan {
    plan_cursor_commit(
        HardwareCursorPath::AtomicPlane,
        admission,
        pending,
        committed,
        primary_going_out,
        client_quiet,
    )
}

/// A session on the legacy ioctl is not this owner's business.
#[test]
fn the_legacy_path_is_left_alone() {
    assert_eq!(
        plan_cursor_commit(
            HardwareCursorPath::LegacyIoctl,
            LegacyHardwareCursorAdmission::Update,
            Some(Some(placement(10))),
            None,
            false,
            true,
        ),
        CursorCommitPlan::Idle
    );
}

/// The cheap case: a frame is going out anyway, so the cursor rides it. This
/// is why the submit policy carries a cursor at all.
#[test]
fn a_cursor_rides_a_frame_that_is_going_out() {
    assert_eq!(
        plan(
            LegacyHardwareCursorAdmission::Update,
            Some(Some(placement(10))),
            None,
            true,
        ),
        CursorCommitPlan::RideNextPrimary
    );
    // Even while a flip is in flight: the frame being prepared is the one the
    // cursor joins, and its commit has not been issued yet.
    assert_eq!(
        plan(
            LegacyHardwareCursorAdmission::DeferredUpdate,
            Some(Some(placement(10))),
            None,
            true,
        ),
        CursorCommitPlan::RideNextPrimary
    );
}

/// The case the model exists for. Nothing is going out and the CRTC is free,
/// so the cursor commits alone -- a client repainting on a cursor blink would
/// otherwise leave the pointer frozen for most of a second.
#[test]
fn an_idle_crtc_takes_a_cursor_only_commit() {
    assert_eq!(
        plan(
            LegacyHardwareCursorAdmission::Update,
            Some(Some(placement(10))),
            None,
            false,
        ),
        CursorCommitPlan::CommitCursorOnly
    );
}

/// A busy CRTC holds the position rather than dropping it. Waiting is what
/// makes the cursor eventually arrive; dropping is what makes it stutter.
#[test]
fn a_busy_crtc_holds_the_position() {
    for admission in [
        LegacyHardwareCursorAdmission::DeferredUpdate,
        LegacyHardwareCursorAdmission::DeferredInitialization,
    ] {
        assert_eq!(
            plan(admission, Some(Some(placement(10))), None, false),
            CursorCommitPlan::Wait,
            "{admission:?} must hold the position"
        );
    }
}

/// Superseding in place means the cell can hold what the plane already shows.
/// Committing to change nothing is the unbounded work the model forbids.
#[test]
fn a_position_already_on_the_plane_costs_no_commit() {
    assert_eq!(
        plan(
            LegacyHardwareCursorAdmission::Update,
            Some(Some(placement(10))),
            Some(placement(10)),
            false,
        ),
        CursorCommitPlan::Idle
    );
    assert_eq!(
        plan(
            LegacyHardwareCursorAdmission::Update,
            Some(Some(placement(11))),
            Some(placement(10)),
            false,
        ),
        CursorCommitPlan::CommitCursorOnly,
        "a different position is still worth a commit"
    );
}

/// Hiding is a change, not an absence. A head the pointer left has something
/// to say, and saying nothing is how a cursor ends up showing on two
/// monitors at once.
#[test]
fn a_head_the_pointer_left_still_commits() {
    assert_eq!(
        plan(
            LegacyHardwareCursorAdmission::Update,
            Some(None),
            Some(placement(10)),
            false,
        ),
        CursorCommitPlan::CommitCursorOnly
    );
    // And once hidden, it stays quiet.
    assert_eq!(
        plan(
            LegacyHardwareCursorAdmission::Update,
            Some(None),
            None,
            false,
        ),
        CursorCommitPlan::Idle
    );
}

/// Nothing pending is nothing to do, whatever the CRTC is doing.
#[test]
fn nothing_pending_is_idle() {
    for going_out in [false, true] {
        assert_eq!(
            plan(
                LegacyHardwareCursorAdmission::Update,
                None,
                Some(placement(10)),
                going_out,
            ),
            CursorCommitPlan::Idle
        );
    }
}

/// Completion only consumes the position that actually entered the accepted
/// KMS request. A newer latest-wins cell remains owed to the display.
#[test]
fn settling_an_older_cursor_preserves_a_newer_pending_position() {
    assert_eq!(
        settle_pending_cursor(Some(Some(placement(11))), Some(placement(10))),
        Some(Some(placement(11)))
    );
}

#[test]
fn settling_the_latest_cursor_clears_the_pending_cell() {
    assert_eq!(
        settle_pending_cursor(Some(Some(placement(10))), Some(placement(10))),
        None
    );
    assert_eq!(settle_pending_cursor(Some(None), None), None);
}

/// The t120 gate. A cursor-only commit blocks until the kernel applies it at
/// a vblank, so one issued while a client is drawing spends the vblank that
/// client's next frame needed -- a measured run paid 234 of them for a fifth
/// of its frame rate. The position waits for the frame that will carry it.
#[test]
fn a_drawing_client_keeps_its_vblank() {
    for admission in [
        LegacyHardwareCursorAdmission::Update,
        LegacyHardwareCursorAdmission::InitializeThenUpdate,
    ] {
        assert_eq!(
            plan_while(admission, Some(Some(placement(10))), None, false, false),
            CursorCommitPlan::Wait,
            "{admission:?} must not take a vblank from a drawing client"
        );
        assert_eq!(
            plan_while(admission, Some(Some(placement(10))), None, false, true),
            CursorCommitPlan::CommitCursorOnly,
            "{admission:?} must still serve a quiet one"
        );
    }
}

/// Riding is free and is decided before quietness is consulted, so a frame on
/// its way carries the cursor whatever the client has been doing.
#[test]
fn a_frame_going_out_carries_the_cursor_either_way() {
    for quiet in [true, false] {
        assert_eq!(
            plan_while(
                LegacyHardwareCursorAdmission::Update,
                Some(Some(placement(10))),
                None,
                true,
                quiet,
            ),
            CursorCommitPlan::RideNextPrimary,
            "a frame is going out; quiet={quiet} cannot matter"
        );
    }
}

/// Two refreshes without a retirement is quiet. One is not: a client pacing
/// exactly on the refresh would otherwise flicker between the two answers.
#[test]
fn quiet_means_two_refreshes_without_a_frame() {
    // 60Hz: 16666us per refresh.
    assert!(
        cursor_only_quiet(0, 5_000_000, 60_000),
        "nothing has ever retired, which is startup"
    );
    assert!(
        !cursor_only_quiet(1_000_000, 1_000_000, 60_000),
        "a frame just retired"
    );
    assert!(
        !cursor_only_quiet(1_000_000, 1_016_666, 60_000),
        "one refresh is a client still keeping pace"
    );
    assert!(
        cursor_only_quiet(1_000_000, 1_033_332, 60_000),
        "two refreshes with nothing drawn is quiet"
    );
    // The same boundary moves with the head's own refresh: at 120Hz an
    // interval is 8333us, so the gate opens at 16666 rather than 33332.
    assert!(
        !cursor_only_quiet(1_000_000, 1_016_665, 120_000),
        "at 120Hz this is one microsecond short of two refreshes"
    );
    assert!(
        cursor_only_quiet(1_000_000, 1_016_666, 120_000),
        "at 120Hz two refreshes have passed"
    );
}

/// A head reporting no refresh is read as 60Hz. Treating it as infinitely
/// quiet would restore exactly the cost the gate removes.
#[test]
fn an_unknown_refresh_does_not_open_the_gate() {
    assert!(!cursor_only_quiet(1_000_000, 1_016_666, 0));
    assert!(cursor_only_quiet(1_000_000, 1_033_332, 0));
}

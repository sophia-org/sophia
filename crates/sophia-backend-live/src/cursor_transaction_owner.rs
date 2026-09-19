//! What a head should do with a cursor that has moved.
//!
//! The kernel serializes atomic commits per CRTC, so a cursor cannot simply
//! be moved when the pointer moves -- it joins the queue the primary already
//! waits in. `CursorPlaneTransactionOwner.tla` settles what happens then, and
//! this is that decision written as a function of the head's state.
//!
//! The interesting case is the one that looks like an optimisation and is
//! not: a cursor-only commit, issued when the CRTC is free and no frame is
//! going out. Without it a cursor waits for the client's next frame, and a
//! client repainting on a cursor blink leaves the pointer frozen for most of
//! a second. TLC refuses the model without it.
//!
//! It is issued only while the client is *quiet*, and that qualifier is the
//! whole of t120. The commit blocks until the kernel applies it at the next
//! vblank, so one issued between a frame retiring and the next arriving holds
//! the owner loop through the very vblank that frame needed. A client
//! rendering continuously never needs it -- its next frame is a refresh away
//! and carries the cursor for free -- and a measured run paid 234 of them for
//! a fifth of its frame rate. When the client stops, the gate opens within
//! two refreshes and the idle behaviour above is unchanged, which is what
//! keeps the cursor from freezing on a desktop nobody is drawing to.

use crate::{HardwareCursorPath, LegacyHardwareCursorAdmission, LibdrmNativeCursorPlacement};

/// Where the cursor should be on one head.
///
/// The outer `Option` is whether anything is waiting; the inner one is
/// whether the pointer is on this head at all. A head the pointer left is
/// told to hide, which is a change to commit rather than nothing to say.
pub type PendingCursor = Option<Option<LibdrmNativeCursorPlacement>>;

/// Settle the cursor position carried by one accepted KMS commit.
///
/// A newer desired position may replace the pending cell while an older
/// primary commit is being prepared. Settling that older commit must advance
/// the committed position without erasing the newer work.
pub fn settle_pending_cursor(
    pending: PendingCursor,
    settled: Option<LibdrmNativeCursorPlacement>,
) -> PendingCursor {
    match pending {
        Some(latest) if latest == settled => None,
        pending => pending,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorCommitPlan {
    /// Nothing to do: nothing pending, or the plane already shows it.
    Idle,
    /// A frame is going out; the cursor rides its commit.
    RideNextPrimary,
    /// The CRTC is free and no frame is waiting: commit the cursor alone.
    CommitCursorOnly,
    /// The CRTC is busy with something not ours to add to. Hold the position.
    Wait,
}

/// Decide what to do with this head's pending cursor.
///
/// Ordered so the cheap answers come first: riding a frame that is going out
/// anyway costs nothing, and a redundant commit costs a commit.
pub fn plan_cursor_commit(
    path: HardwareCursorPath,
    admission: LegacyHardwareCursorAdmission,
    pending: PendingCursor,
    committed: Option<LibdrmNativeCursorPlacement>,
    primary_going_out: bool,
    client_quiet: bool,
) -> CursorCommitPlan {
    if path != HardwareCursorPath::AtomicPlane {
        return CursorCommitPlan::Idle;
    }
    let Some(placement) = pending else {
        return CursorCommitPlan::Idle;
    };
    // Already showing it. Superseding in place means the pending cell can
    // hold a position the plane reached by some other commit, and paying for
    // a commit to change nothing is exactly the unbounded work the model
    // forbids.
    if placement == committed {
        return CursorCommitPlan::Idle;
    }
    // A frame is going out: the cursor rides it. This is the cheap case, and
    // the reason the submit policy carries a cursor at all.
    if primary_going_out {
        return CursorCommitPlan::RideNextPrimary;
    }
    match admission {
        // The plane is not installed yet, or a flip is in flight. Either way
        // this head cannot commit now -- the position waits rather than being
        // dropped, which is what makes the cursor eventually arrive.
        LegacyHardwareCursorAdmission::DeferredUpdate
        | LegacyHardwareCursorAdmission::DeferredInitialization => CursorCommitPlan::Wait,
        // The CRTC is free and nothing else is going out. This is the commit
        // the model needs: without it a cursor waits on a client that may not
        // draw again for most of a second.
        //
        // Only while the client is quiet, though. This commit blocks until
        // the next vblank, so issuing one in the gap between a frame retiring
        // and the next arriving spends the frame's own vblank on the cursor.
        // A client still drawing has a frame along within a refresh, and that
        // frame carries the cursor for nothing; waiting for it is the same
        // waiting the busy-CRTC arm above already does, and holds the
        // position rather than dropping it.
        LegacyHardwareCursorAdmission::Update
        | LegacyHardwareCursorAdmission::InitializeThenUpdate => {
            if client_quiet {
                CursorCommitPlan::CommitCursorOnly
            } else {
                CursorCommitPlan::Wait
            }
        }
    }
}

/// Whether this head's client has stopped drawing, so a cursor-only commit
/// may block on its vblank without taking one a frame needed.
///
/// Quiet means no primary has retired here for two refreshes: one is the
/// interval a still-drawing client would beat, and the second is the margin
/// that keeps a client pacing exactly on the refresh from flickering between
/// the two answers. `0` is "nothing has ever retired", which is startup --
/// quiet, so a session installs its cursor before the first frame exactly as
/// it always did.
///
/// A head reporting no refresh is read as 60Hz rather than as infinitely
/// quiet; the seeded value has been real since `310cf886`, and treating a
/// zero as "commit freely" would restore the cost this gate exists to remove.
pub fn cursor_only_quiet(
    last_retirement_ust_usec: u64,
    now_ust_usec: u64,
    refresh_millihz: u32,
) -> bool {
    if last_retirement_ust_usec == 0 {
        return true;
    }
    let refresh_millihz = if refresh_millihz == 0 {
        60_000
    } else {
        refresh_millihz
    };
    let interval_usec = 1_000_000_000_u64 / u64::from(refresh_millihz);
    now_ust_usec.saturating_sub(last_retirement_ust_usec) >= interval_usec.saturating_mul(2)
}

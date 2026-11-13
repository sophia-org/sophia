//! G3-5d d-2b3c: two labelled stub Full-debt controls (MD1, MD2). The
//! renderer is G3-4's stub, so nothing here is running-worker slot evidence.
//! Both start as R1b does: A's one result is served and held, A stops, the
//! queue is filled, and the next hooked turn collects the result and its
//! ended-grant Release finds the queue actually Full.
//!
//! Three Release facts stay apart. `release_attempts` and
//! `release_transfers` count the row's facade calls and their Transferred
//! answers; `renderer_releases` is the stub's drain of the command queue,
//! and draining is a labelled operation of its own. There is no worker ACK:
//! the receipt is the facade's owner-local evidence of an enqueue.
//!
//! Every turn is a hooked `lane.turn()` at `Instant::now()`. A's grant has
//! ended, so the walk's Debt arm alone handles the job and no deadline is
//! read; the admitted deadline is never replaced.
use super::*;
use sophia_backend_live::CombinedDeclaredHead;

/// A's job held, A stopped and the queue filled; one hooked turn collects
/// it and the walk's Release is Full. The job and its issuing identity
/// (native owner, head, facade ticket) as the row recorded it.
fn full_debt(lane: &mut Lane) -> (u64, (u64, u64, u64)) {
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let job = lane.until(&mut a, "in flight");
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    lane.h.owner.stop(a.key).unwrap();
    drop(a);
    lane.native.fill_renderer_queue();
    assert!(lane.turn().is_none());
    let row = lane.h.owner.processes.combined_row(0);
    assert_eq!(
        (row.states(), row.debt(job)),
        (vec![(job, "debt")], Some(CombinedRelease::Pending)),
        "the first flush found the queue Full"
    );
    let [collection] = row.collections() else {
        panic!("one collection: {:?}", row.collections());
    };
    let [binding] = row.bindings() else {
        panic!("one binding: {:?}", row.bindings());
    };
    assert_eq!(
        (collection.job, collection.head),
        (job, binding.head),
        "the collected job carries its submission's issuing identity"
    );
    assert_eq!(counts(&lane.native), (1, 0));
    assert_eq!(lane.native.combined_receipt(output().id), None);
    (job, collection.head)
}

/// The row's facade calls and their Transferred answers, as
/// (`release_attempts`, `release_transfers`).
fn counts(native: &CombinedNativeFixture) -> (usize, usize) {
    (native.release_attempts(), native.release_transfers())
}

/// The successor guard, probed outside any Session turn: one reservation
/// through the native's own entry, cancelled at once when it is granted.
fn successor(lane: &mut Lane) -> std::result::Result<(), std::io::ErrorKind> {
    let (head, _inbox) = ShellContentNative::reserve_combined_frame(&mut lane.native, output().id)
        .map_err(|error| error.kind())?;
    assert_eq!(
        ShellContentNative::cancel_combined_frame(&mut lane.native, &head).ok(),
        Some(true),
        "the probe's reservation was freed"
    );
    Ok(())
}

/// MD1: the issuing head is disabled between owner visits (declared, as a
/// topology change leaves it). The maintenance walk still visits the debt
/// through the stored issuing head, with no replacement issuer: still Full,
/// it stays Pending. After the labelled drain the next visit enqueues one
/// Release and the same call takes its receipt, so the row closes; the
/// receipt is absent before and after that one call. A further visit sends
/// nothing.
///
/// Calls: setup turn (Full); declare Disabled; visit 2 (Full, disabled);
/// drain 1; visit 3 (enqueue and Transferred); drain 2; visit 4.
#[test]
fn a_full_first_flush_retries_and_a_disabled_head_still_releases() {
    let mut lane = Lane::new();
    let (job, issuer) = full_debt(&mut lane);
    lane.native
        .declare_head(output().id, CombinedDeclaredHead::Disabled);
    assert!(!lane.native.head_enabled(output().id));
    // Visit 2, still Full, the head disabled.
    assert!(lane.turn().is_none());
    assert_eq!(
        lane.h.owner.processes.combined_row(0).debt(job),
        Some(CombinedRelease::Pending),
        "MD1: the disabled head's debt was visited through its issuer"
    );
    assert_eq!(counts(&lane.native), (2, 0), "one more maintenance attempt");
    assert_eq!(lane.states(), [(job, "debt")]);
    assert_eq!(lane.native.combined_receipt(output().id), None);
    // Drain 1: the queue held only filler.
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    // Visit 3, capacity available, the head still disabled.
    assert!(lane.turn().is_none());
    assert!(!lane.native.head_enabled(output().id));
    assert_eq!(
        counts(&lane.native),
        (3, 1),
        "one enqueue answered Transferred"
    );
    assert!(lane.states().is_empty(), "the row closed");
    assert_eq!(
        lane.native.combined_receipt(output().id),
        None,
        "the maintenance call took its own receipt"
    );
    // Drain 2: the one Release the visit enqueued.
    assert_eq!(lane.native.renderer_releases(), 1);
    // Visit 4: nothing left to send or transfer.
    assert!(lane.turn().is_none());
    assert_eq!(counts(&lane.native), (3, 1), "no second attempt");
    assert_eq!(lane.native.renderer_releases(), 1, "no second Release");
    let row = lane.h.owner.processes.combined_row(0);
    assert_eq!(
        row.collections().iter().map(|c| c.head).collect::<Vec<_>>(),
        [issuer],
        "no other issuer was recorded"
    );
}

/// MD2: an intervening native-only service. After the labelled drain, one
/// ordinary `poll()` of the worker, with no Session call, enqueues the
/// retained Release and leaves the revision-5 receipt for that admission;
/// the row still owes its maintenance step and the successor guard holds.
/// The labelled drain then sees the one Release. The next walk takes
/// Transferred from that receipt, closes the row once and frees the guard;
/// it sends nothing.
///
/// Calls: setup turn (Full); successor probe 1; drain 1; ordinary poll;
/// successor probe 2; drain 2; visit 2 (Transferred); successor probe 3;
/// drain 3; visit 3.
#[test]
fn an_ordinary_poll_flush_closes_the_debt_as_transferred_once() {
    let mut lane = Lane::new();
    let (job, (_, _, ticket)) = full_debt(&mut lane);
    assert_eq!(
        successor(&mut lane),
        Err(std::io::ErrorKind::WouldBlock),
        "probe 1: the cell retains the Release"
    );
    // Drain 1: the queue held only filler.
    assert_eq!(lane.native.renderer_releases(), 0, "only filler was queued");
    // The labelled native-only service.
    assert!(lane.native.ordinary_poll(output().id), "nothing in flight");
    assert_eq!(
        lane.native.combined_receipt(output().id),
        Some((ticket, job)),
        "the poll's enqueue left this admission's receipt"
    );
    assert_eq!(
        counts(&lane.native),
        (1, 0),
        "the poll is no row attempt and no transfer"
    );
    assert_eq!(
        lane.h.owner.processes.combined_row(0).debt(job),
        Some(CombinedRelease::Pending),
        "the row still owes its maintenance step"
    );
    assert_eq!(
        successor(&mut lane),
        Err(std::io::ErrorKind::WouldBlock),
        "probe 2: the receipt guards the successor"
    );
    // Drain 2: the poll's one Release reached the queue.
    assert_eq!(lane.native.renderer_releases(), 1);
    assert_eq!(
        lane.native.combined_receipt(output().id),
        Some((ticket, job)),
        "draining is not acknowledgement"
    );
    // Visit 2: the walk's flush reads the receipt.
    assert!(lane.turn().is_none());
    assert_eq!(
        lane.native.combined_receipt(output().id),
        None,
        "MD2: the walk consumed the admission's receipt"
    );
    assert!(lane.states().is_empty(), "the row closed");
    assert_eq!(
        counts(&lane.native),
        (2, 1),
        "one maintenance attempt, answered Transferred"
    );
    assert_eq!(successor(&mut lane), Ok(()), "probe 3: the guard is free");
    // Drain 3: the walk sent no second Release.
    assert_eq!(lane.native.renderer_releases(), 1);
    // Visit 3: nothing left.
    assert!(lane.turn().is_none());
    assert_eq!(counts(&lane.native), (2, 1), "transferred once");
    assert_eq!(lane.native.renderer_releases(), 1);
}

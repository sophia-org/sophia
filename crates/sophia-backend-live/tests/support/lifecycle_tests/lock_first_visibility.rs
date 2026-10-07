//! t291: a first Present parked while the session is locked stays parked,
//! even when the condition that parked it no longer holds, and unlocking
//! releases it. The lock draws no client and no preview, so releasing on that
//! condition would re-park the candidate every pass (the t246 shape).
use super::*;

fn cover(epoch: u64) -> sophia_engine::SessionLockCover {
    sophia_engine::SessionLockCover::fill(
        sophia_engine::SessionLockEpoch::from_raw(epoch).unwrap(),
        sophia_engine::CompositorRgb8 {
            red: 0x10,
            green: 0x20,
            blue: 0x30,
        },
    )
}

#[test]
fn a_first_present_parked_while_locked_waits_for_the_unlock() {
    let mut scene = present_scene();
    let now = Instant::now();
    let (_, candidate) = scene.queue_present(950, now);
    assert!(scene.runtime.present_scheduler.defer_first_visibility(
        candidate,
        crate::LiveProductionFirstVisibilityReason::OutsidePresentationOrder,
        now,
    ));
    // The application is in the presentation order, so unlocked this
    // reason no longer holds and the candidate would be released.
    scene
        .runtime
        .set_session_lock_on::<MirroredTarget>(Some(cover(7)), &scene.scene, None)
        .unwrap();
    scene
        .runtime
        .service_first_visibility_presentations(now + Duration::from_millis(10));
    assert_eq!(
        scene
            .runtime
            .present_scheduler
            .awaiting_first_visibility()
            .count(),
        1,
        "locked: the first Present stays parked"
    );
    scene
        .runtime
        .set_session_lock_on::<MirroredTarget>(None, &scene.scene, None)
        .unwrap();
    scene
        .runtime
        .service_first_visibility_presentations(now + Duration::from_millis(20));
    assert_eq!(
        scene
            .runtime
            .present_scheduler
            .awaiting_first_visibility()
            .count(),
        0,
        "unlocking releases it"
    );
}

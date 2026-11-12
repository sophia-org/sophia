//! G3-2 through the real route: the job's stop flag at dequeue and retry,
//! the owner's view of a mismatch, the ring job's own description, and a
//! parked plan's drop.

use super::super::super::super::combined::{CombinedDisposition, CombinedSettleRefused, attempt};
use super::super::super::correlated_facade;
use super::super::{Validator, WAIT, WorkerCommand, forward, route, settle};
use super::{
    BYTES, RenderAttempt, Slots, description, occurrence, plan, render_and_record, sealed,
};
use crate::LiveRendererFrameSlotAcquire;
use crate::LiveRendererFrameSlotRelease;
use sophia_protocol::{ContentGrant, Size};
use std::time::Instant;

#[test]
fn stop_after_forward_before_the_first_dequeue_renders_nothing() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut route = route(&mut facade, &commands);
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let reservation = forward(&mut validator, &mut facade, 1, Instant::now() + WAIT);
    assert!(validator.cancel(reservation.job()));
    let mut slots = Slots::new();
    route.serve(Instant::now(), |plan, pixels, described| {
        attempt(&mut slots, plan, pixels, described, |_, _, _| {
            panic!("a stopped job renders nothing")
        })
    });
    let result = facade.take_combined_result().unwrap();
    assert_eq!(result.disposition(), CombinedDisposition::Stopped);
    assert!(slots.log.is_empty(), "a stopped job takes no slot");
    settle(&mut validator, reservation, result);
}

#[test]
fn stop_while_parked_is_seen_at_the_retry_and_renders_nothing() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut route = route(&mut facade, &commands);
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let reservation = forward(&mut validator, &mut facade, 2, Instant::now() + WAIT);
    let mut slots = Slots::new();
    let busy = (0..3)
        .map(|_| match slots.pool.try_acquire() {
            LiveRendererFrameSlotAcquire::Acquired(token) => token,
            _ => panic!("an empty pool leases three slots"),
        })
        .collect::<Vec<_>>();
    route.serve(Instant::now(), |plan, pixels, described| {
        attempt(&mut slots, plan, pixels, described, |_, _, _| {
            panic!("no slot, no render")
        })
    });
    assert!(route.deadline().is_some(), "the job parked");
    assert!(validator.cancel(reservation.job()));
    assert_eq!(
        slots.pool.release(busy[0]),
        LiveRendererFrameSlotRelease::Released
    );
    route.serve(Instant::now(), |plan, pixels, described| {
        attempt(&mut slots, plan, pixels, described, |_, _, _| {
            panic!("a stopped job renders nothing")
        })
    });
    let result = facade.take_combined_result().unwrap();
    assert_eq!(result.disposition(), CombinedDisposition::Stopped);
    assert_eq!(
        slots.log,
        ["acquire"],
        "the retry read the stop before taking the freed slot"
    );
    settle(&mut validator, reservation, result);
}

#[test]
fn a_mismatch_reaches_the_owner_and_cancels_only_its_own_release() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut validator = Validator::for_combined_plans(2, 64).unwrap();
    let (issued, inbox) = facade
        .reserve_combined_frame()
        .expect("an idle facade reserves");
    let WorkerCommand::RegisterCombined(registration) = commands.recv().unwrap() else {
        panic!("first combined use registers the route");
    };
    let mut route = registration
        .route_for(facade.output(), &facade.claim)
        .expect("route for the live claim");
    let reservation = validator
        .reserve_combined(32, Instant::now() + WAIT)
        .unwrap();
    let _owner_plan = validator
        .submit_combined(
            &reservation,
            sealed(&BYTES),
            plan(&[100], vec![occurrence(0, 1, 0)]),
            inbox,
        )
        .ok()
        .expect("combined submission");
    let end = Instant::now() + WAIT;
    while !reservation.was_forwarded() {
        assert!(Instant::now() < end, "the validator did not forward");
        std::thread::yield_now();
    }
    let mut slots = Slots::new();
    route.serve(Instant::now(), |plan, pixels, described| {
        attempt(&mut slots, plan, pixels, described, |_, _, _| {
            panic!("a sealed job with a placeholder renders nothing")
        })
    });
    let result = facade.take_combined_result().unwrap();
    assert_eq!(
        result.disposition(),
        CombinedDisposition::Mismatch,
        "the owner reads the mismatch before settling consumes it"
    );

    let foreign = validator
        .reserve_combined(8, Instant::now() + WAIT)
        .unwrap();
    let Err(CombinedSettleRefused::Refused(foreign, result)) =
        facade.settle_combined_frame(&issued, &mut validator, foreign, result)
    else {
        panic!("a foreign reservation cannot settle this result");
    };
    assert_eq!(
        result.disposition(),
        CombinedDisposition::Mismatch,
        "a refused settlement keeps the disposition"
    );
    assert!(validator.release_reserved(foreign.job()));

    let pixels = facade
        .settle_combined_frame(&issued, &mut validator, reservation, *result)
        .ok()
        .expect("the mismatch settles its own credit");
    assert_eq!(pixels.bytes(), BYTES.as_slice(), "the pixels come home");
    assert_eq!(validator.pending(), 0);
    let (successor, _inbox) = facade
        .reserve_combined_frame()
        .expect("the mismatch cancelled its own Release reservation");
    assert!(
        !facade.cancel_combined_frame(&issued),
        "the settled admission cannot cancel its successor"
    );
    assert!(facade.cancel_combined_frame(&successor));
}

/// Narrower than the owned-copy row path: this submits through
/// `submit_ring_combined` with the description in hand. The row-derived
/// provenance of that description is G3-1's (combined_frame_join M1).
#[test]
fn a_ring_job_is_checked_against_the_description_it_was_admitted_with() {
    use sophia_runtime::{ContentEpochRegistry, OwnedRing, OwnedRingGeometry, OwnedSlotIdentity};
    use std::os::unix::fs::FileExt;
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    registry
        .admit(sophia_protocol::ContentLimits::prototype(grant))
        .unwrap();
    let geometry = OwnedRingGeometry::new(2, 32).unwrap();
    let held = registry.reserve_owned_ring_source(grant, geometry).unwrap();
    let file = std::fs::File::from(
        rustix::fs::memfd_create("combined-ring", rustix::fs::MemfdFlags::CLOEXEC).unwrap(),
    );
    file.set_len(geometry.source_bytes() as u64).unwrap();
    let mut ring = OwnedRing::admit(file.try_clone().unwrap(), held).unwrap();

    let (mut facade, commands, _reply) = correlated_facade();
    let mut route = route(&mut facade, &commands);
    let mut validator = Validator::for_combined_plans(1, 64).unwrap();
    let mut tall = occurrence(0, 1, 0);
    tall.size = Size {
        width: 4,
        height: 2,
    };
    tall.stride = 16;
    for (slot, placeholder, expected) in [
        (0, occurrence(0, 1, 0), CombinedDisposition::Rendered),
        (1, tall, CombinedDisposition::Mismatch),
    ] {
        let identity = OwnedSlotIdentity {
            offer: slot as u64 + 1,
            slot,
            generation: 2,
        };
        let offset = (slot * geometry.slot_extent()) as u64;
        file.write_all_at(&BYTES, offset + 4096).unwrap();
        file.write_all_at(&identity.generation.to_le_bytes(), offset)
            .unwrap();
        let read = ring.offer(identity, 32).unwrap();
        let reservation = validator
            .reserve_combined(32, Instant::now() + WAIT)
            .unwrap();
        let inbox = facade.combined_inbox().expect("facade takes combined work");
        let _owner_plan = validator
            .submit_ring_combined(
                &reservation,
                read,
                description(2, 4, 32),
                plan(&[100], vec![placeholder]),
                inbox,
            )
            .ok()
            .expect("ring submission");
        let end = Instant::now() + WAIT;
        while !reservation.was_forwarded() {
            assert!(Instant::now() < end, "the validator did not forward");
            std::thread::yield_now();
        }
        let mut handed = None;
        let mut slots = Slots::new();
        route.serve(Instant::now(), |plan, pixels, described| {
            handed = described.cloned();
            attempt(
                &mut slots,
                plan,
                pixels,
                described,
                render_and_record(&mut Vec::new(), &mut Vec::new()),
            )
        });
        assert_eq!(
            handed,
            Some(description(2, 4, 32)),
            "the renderer is handed the job's own description"
        );
        let result = facade.take_combined_result().unwrap();
        assert_eq!(result.disposition(), expected, "slot {slot}");
        assert_eq!(result.pixels.bytes(), BYTES.as_slice());
        assert!(
            validator
                .settle_combined_token(reservation, result.token)
                .is_ok()
        );
    }
}

#[test]
fn dropping_a_parked_plan_settles_nothing() {
    let (mut facade, commands, _reply) = correlated_facade();
    let mut route = route(&mut facade, &commands);
    let mut validator = Validator::for_combined_plans(1, 8).unwrap();
    let _reservation = forward(&mut validator, &mut facade, 3, Instant::now() + WAIT);
    route.serve(Instant::now(), |plan, pixels, _| {
        RenderAttempt::NoSlot(plan, pixels)
    });
    assert!(route.deadline().is_some(), "the job parked");
    let unwound = std::thread::spawn(move || {
        let _held = route;
        panic!("the renderer unwinds with a parked plan");
    })
    .join();
    assert!(unwound.is_err());
    assert_eq!(
        validator.pending(),
        1,
        "dropping the plan frees memory only; the credit stays with the owner"
    );
}

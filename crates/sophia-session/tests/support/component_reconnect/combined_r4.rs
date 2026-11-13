//! G3-5b R4 Session controls, on the combined lane's harness: the
//! reserve-time projection against the bundle its admission produced, and a
//! compose fault beside an unrelated job in the facade's cell.
use super::*;
use sophia_engine::CompositorImageSource;
use sophia_shell_client::RingOfferState;

/// R4: an independent projection comparison through the real boundary. The
/// reserve hook projects a real queued frame with the host's description,
/// before admission. An unrelated reservation holds the facade's cell, so
/// that frame is refused a renderer and goes on today's path, where the
/// same admission later produces a bundle. Both occurrences of the one
/// resource and the allocation, scale and geometry facts agree; only the
/// image source differs. Nothing is reconstructed from the bundle.
#[test]
fn a_queued_frame_projects_as_the_bundle_its_admission_produced() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    let (unrelated, _inbox) =
        ShellContentNative::reserve_combined_frame(&mut lane.native, output().id)
            .expect("the unrelated reservation holds the cell");
    lane.submit_at(&mut a, 1, PIXEL, &[0, 5]);
    lane.drive_until(&mut a, "the reserve-time projection", |lane, _| {
        !lane
            .h
            .owner
            .processes
            .combined_row(0)
            .projections()
            .is_empty()
    });
    lane.finish_ordinary(&mut a, 1);
    let provisional = lane
        .h
        .owner
        .processes
        .combined_row(0)
        .projections()
        .to_vec();
    assert_eq!(provisional.len(), 1, "one proposal of the one frame");
    let provisional = &provisional[0];
    assert_eq!(a.bundles.len(), 1, "one bundle from that admission");
    let mut ordinary = a.bundles[0].clone();
    assert_eq!(ordinary.images.len(), 2, "both occurrences");
    for (image, admitted) in ordinary.images.iter_mut().zip(&provisional.images) {
        assert!(matches!(
            &admitted.resource,
            CompositorImageSource::Provisional(_)
        ));
        assert_eq!(
            image.resource.identity(),
            admitted.resource.identity(),
            "the placeholder names the copy the lease holds"
        );
        image.resource = admitted.resource.clone();
    }
    assert_eq!(
        &ordinary, provisional,
        "one placement algorithm over the same facts"
    );
    assert_eq!(
        provisional.images[1].geometry_px.x,
        provisional.images[0].geometry_px.x + 5
    );
    assert_ne!(provisional.images[0].node, provisional.images[1].node);
    assert!(matches!(
        ShellContentNative::cancel_combined_frame(&mut lane.native, &unrelated),
        Ok(true)
    ));
}

/// R4: a declared compose fault while an unrelated real job occupies the
/// facade's cell. A's J1 is parked in the renderer, still the row's, when A
/// stops. B's frame composes once and fails: no renderer reservation is
/// attempted, no copy is admitted or charged, the frame stays queued and
/// unanswered, and J1 and its owners are untouched. B's real close then
/// leaves J1 to finish on its own.
#[test]
fn a_compose_fault_beside_an_unrelated_job_reserves_and_admits_nothing() {
    let mut lane = Lane::new();
    let mut a = lane.attempt();
    lane.submit(&mut a, 1, PIXEL);
    let j1 = lane.until(&mut a, "in flight");
    assert!(lane.native.park_combined(), "the validator forwarded J1");
    lane.h.owner.stop(a.key).unwrap();
    drop(a);
    let mut b = lane.attempt();
    let composes = lane.native.composes();
    let reserves = lane.native.reserve_attempts();
    let images = lane.h.owner.processes.image_pending(0);
    let usage = lane.usage(&b);
    lane.native.declare_compose_fault("declared compose fault");
    lane.submit(&mut b, 1, PIXEL);
    let error = lane.until_failure(&mut b);
    assert!(
        matches!(&error, ShellTransportError::CombinedComposition(reason)
            if reason == "declared compose fault"),
        "the first error survives: {error:?}"
    );
    assert_eq!(lane.native.composes(), composes + 1, "one composition");
    assert_eq!(
        lane.native.reserve_attempts(),
        reserves,
        "no renderer reservation was attempted"
    );
    assert_eq!(
        lane.h.owner.processes.image_pending(0),
        images,
        "no copy admitted"
    );
    assert_eq!(lane.usage(&b), usage, "nothing charged");
    assert!(!lane.resident(&b, 1));
    assert!(b.terminals(1).is_empty(), "the frame is unanswered");
    let offers: Vec<_> = b
        .peer
        .image_adapter()
        .unwrap()
        .ring_offers()
        .into_iter()
        .filter(|offer| offer.resource == RESOURCE.id && offer.resource_generation == 1)
        .map(|offer| offer.state)
        .collect();
    assert_eq!(
        offers,
        [RingOfferState::Pending],
        "the one frame, still queued"
    );
    assert_eq!(lane.states(), [(j1, "in flight")], "J1 untouched");
    // The real close: B stops; J1 finishes on its own.
    lane.h.owner.stop(b.key).unwrap();
    drop(b);
    assert!(lane.turn().is_none());
    lane.native
        .serve_combined(output().id, Instant::now(), exported());
    assert!(lane.turn().is_none());
    assert!(
        lane.states().is_empty(),
        "J1 settled Stopped and retired with its ended grant"
    );
    assert_eq!(lane.native.renderer_releases(), 0, "J1 was never held");
    assert_eq!(lane.native.reserve_attempts(), reserves);
}

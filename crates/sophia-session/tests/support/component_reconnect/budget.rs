//! Repeated and saturated grants at the actual component service/transport join.
use super::*;

#[test]
fn repeated_reduced_successors_keep_every_predecessor_until_its_consumer_ends() {
    let mut h = Harness::new();
    let (neighbor, _neighbor_peer, _neighbor_pixels) = h.connect(1);
    let mut pins = Vec::new();
    let mut previous = None;
    for round in 0..4 {
        let (key, mut peer, pixels) = h.connect_extent(0, 1 + round);
        let limits = h
            .owner
            .with_service(key, |_, transport| {
                transport.content_limits().unwrap().clone()
            })
            .unwrap();
        assert_eq!(
            limits.max_retiring_bytes,
            16 * 1024 * 1024 - 4 * u64::from(round)
        );
        assert_eq!(limits.max_resource_bytes, 4 * 1024 * 1024);
        if let Some(old) = previous {
            assert!(h.owner.with_service(old, |_, _| ()).is_err());
        }
        let bands = h.owner.work_area_bands();
        h.candidate(key, &mut peer, 1);
        assert_eq!(h.owner.work_area_bands(), bands);
        h.backend.simulate_submit(output().id);
        h.complete(key, &mut peer, 1);
        assert_ne!(h.owner.work_area_bands(), bands);
        h.owner.retry_at[0] = Some(Instant::now() + Duration::from_secs(60));
        drop(peer);
        h.service();
        pins.push(pixels);
        assert_eq!(h.owner.collect().retired_epochs, pins.len());
        assert_eq!(
            h.owner.phase(neighbor).unwrap(),
            ComponentConnectionPhase::Connected
        );
        previous = Some(key);
    }
    // The backend retains the final displayed image; it no longer needs the
    // three replaced source images. Only their independent pins keep them alive.
    let last = pins.pop().unwrap();
    drop(pins);
    assert_eq!(h.owner.collect().retired_epochs, 1);
    assert_eq!(last.bytes(), &[1, 2, 3, 255]);
}

#[test]
fn component_budget_refusal_is_named_and_recovers_after_real_collection() {
    let mut h = Harness::new();
    let (old, mut peer, retained) = h.connect(0);
    let (neighbor, _neighbor_peer, _neighbor_pixels) = h.connect(1);
    h.candidate(old, &mut peer, 1);
    h.backend.simulate_submit(output().id);
    h.complete(old, &mut peer, 1);
    let bands = h.owner.work_area_bands();
    let mut pins = Vec::new();
    for id in 2..=7 {
        pins.push(
            h.owner
                .with_service(old, |_, transport| {
                    let pin = wire::upload_maximum(transport, &mut peer, id);
                    if id <= 4 {
                        wire::send(
                            &mut peer,
                            ShellContentRecord::ResourceRetire(ContentResourceRetire {
                                grant: old.grant,
                                resource: pin.description().resource,
                            }),
                        );
                        transport.service_content_resources(0).unwrap();
                        transport.poll_io().unwrap();
                    }
                    pin
                })
                .unwrap(),
        );
    }
    h.owner.retry_at[0] = Some(Instant::now() + Duration::from_secs(60));
    drop(peer);
    h.service();
    let before = h.owner.collect();
    assert_eq!(
        before.memory.resident + before.memory.retiring,
        24 * 1024 * 1024 + 8
    );
    let error = h.owner.start(0).unwrap_err();
    assert_eq!(
        crate::component_start_cause::classify(&error.to_string()).as_str(),
        "content_budget"
    );
    assert_eq!(h.owner.attempt(0), Some(old));
    assert_eq!(h.owner.collect(), before);
    assert_eq!(h.owner.work_area_bands(), bands);
    drop(pins.pop());
    h.owner.collect();
    let (fresh, _fresh_peer, _fresh_pixels) = h.connect(0);
    assert_ne!(fresh.grant, old.grant);
    assert!(h.owner.with_service(old, |_, _| ()).is_err());
    assert_eq!(
        h.owner.phase(neighbor).unwrap(),
        ComponentConnectionPhase::Connected
    );
    assert_eq!(h.owner.work_area_bands(), bands);
    assert_eq!(retained.bytes(), &[1, 2, 3, 255]);
    assert!(h.owner.accounting().reserved_bytes <= 64 * 1024 * 1024);
}

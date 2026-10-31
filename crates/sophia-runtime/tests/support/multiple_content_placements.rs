use super::*;

fn assemble_three(
    candidates: &mut ContentCandidateStore,
    resources: &ContentResourceStore,
    generation: u64,
    ids: [ContentResourceId; 3],
    last_x: i32,
) -> Result<(), ContentCandidateError> {
    let now = generation * 10;
    candidates.grant_permit(tx(10), output(), generation, generation, now)?;
    let mut start = begin(generation);
    start.placement_count = 3;
    candidates.begin(tx(11), start, now + 1)?;
    let mut rows = chunk(generation, ids[0]);
    rows.placements = ids
        .into_iter()
        .zip([3, 5, last_x])
        .map(|(resource, x)| ContentPlacement {
            resource,
            surface_index: 0,
            destination_x_px: x,
            destination_y_px: 4,
        })
        .collect();
    candidates.chunk(tx(12), rows, now + 2)?;
    let mut finish = end(generation);
    finish.placement_count = 3;
    candidates.end(tx(13), finish, context(&[allocation()]), resources, now + 3)
}

fn submit(candidates: &mut ContentCandidateStore, generation: u64) -> ContentRenderBundle {
    let bundle = candidates
        .begin_submission(output(), generation, generation * 10 + 4)
        .unwrap();
    candidates
        .prepared(output(), generation, 20, 30, generation * 10 + 5)
        .unwrap();
    candidates
        .presented(output(), generation, 40, 20, 30)
        .unwrap();
    bundle
}

fn retire(resources: &mut ContentResourceStore, id: u64) {
    resources
        .retire(
            tx(20),
            &ContentResourceRetire {
                grant: grant(),
                resource: resource_id(id),
            },
        )
        .unwrap();
}

#[test]
fn replacing_one_placement_keeps_other_resources_and_waits_for_old_consumers() {
    for replacement in [
        resource_id(4),
        ContentResourceId {
            id: 3,
            generation: 2,
        },
    ] {
        replace_one(replacement);
    }
}

fn replace_one(replacement: ContentResourceId) {
    let (mut candidates, mut resources) = stores();
    for id in 1..=3 {
        upload(&mut resources, id);
    }
    assemble_three(
        &mut candidates,
        &resources,
        1,
        [1, 2, 3].map(resource_id),
        7,
    )
    .unwrap();
    let old = submit(&mut candidates, 1);
    isb::accept(
        &mut resources,
        tx(2),
        ContentResourceBegin {
            resource: replacement,
            ..resource_begin(replacement.id)
        },
        &[4, 0, 0, 255, 0, 128, 0, 128],
        0,
    );
    while resources.take_event().is_some() {}
    let next_ids = [resource_id(1), resource_id(2), replacement];
    assemble_three(&mut candidates, &resources, 2, next_ids, 7).unwrap();
    let current = submit(&mut candidates, 2);
    assert_eq!(
        current
            .placements
            .iter()
            .map(|p| p.resource)
            .collect::<Vec<_>>(),
        next_ids
    );
    for id in [1, 2] {
        let before = old.resource(resource_id(id)).unwrap().cpu_bytes().unwrap();
        let after = current
            .resource(resource_id(id))
            .unwrap()
            .cpu_bytes()
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(
            before.as_ptr(),
            after.as_ptr(),
            "retained placement keeps its accepted storage"
        );
    }
    assert!(current.resource(resource_id(3)).is_none());
    assert!(old.resource(replacement).is_none());
    assert_eq!(
        current.resource(replacement).unwrap().cpu_bytes().unwrap()[0],
        4
    );

    // A second frame consumer outlives the old candidate bundle.
    let consumer = old.resource(resource_id(3)).unwrap().clone();
    retire(&mut resources, 3);
    resources.collect();
    assert!(resources.take_event().is_none());
    drop(old);
    resources.collect();
    assert!(resources.take_event().is_none());
    assert_eq!(consumer.cpu_bytes().unwrap().len(), 8);
    drop(consumer);
    resources.collect();
    let released = resources.take_event().unwrap();
    assert!(
        matches!(released.record, ShellContentRecord::ResourceReleased(row) if row.resource == resource_id(3))
    );
    resources.confirm_isb_release(resource_id(3)).unwrap();
    resources.collect();
    assert!(resources.take_event().is_none());
    assert_eq!(resources.usage().retiring, 0);
    for id in next_ids {
        assert_eq!(current.resource(id).unwrap().cpu_bytes().unwrap().len(), 8);
    }
    // The unchanged identities remain admissible in another candidate.
    assemble_three(&mut candidates, &resources, 3, next_ids, 7).unwrap();
}

#[test]
fn one_invalid_placement_refuses_the_whole_candidate() {
    for stale in [false, true] {
        let (mut candidates, mut resources) = stores();
        for id in 1..=3 {
            upload(&mut resources, id);
        }
        if stale {
            retire(&mut resources, 2);
        }
        assert_eq!(
            assemble_three(
                &mut candidates,
                &resources,
                1,
                [1, 2, 3].map(resource_id),
                if stale { 7 } else { 63 }
            ),
            Err(if stale {
                ContentCandidateError::Stale
            } else {
                ContentCandidateError::Malformed
            })
        );
        assert_eq!(candidates.pending_candidate_count(), 0);
        assert_eq!(candidates.submitted_candidate_count(), 0);
        assert!(candidates.begin_submission(output(), 1, 14).is_err());
        // Refusal must release any lease obtained before the invalid row.
        while resources.take_event().is_some() {}
        retire(&mut resources, 1);
        resources.collect();
        assert!(std::iter::from_fn(|| resources.take_event()).any(|event|
            matches!(event.record, ShellContentRecord::ResourceReleased(row) if row.resource == resource_id(1))));
        resources.confirm_isb_release(resource_id(1)).unwrap();
    }
}

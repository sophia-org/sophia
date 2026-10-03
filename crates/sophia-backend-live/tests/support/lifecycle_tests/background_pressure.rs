use super::*;

fn full_registry(now: Instant) -> PresentScene {
    let mut scene = present_scene();
    for id in 1000..1064 {
        let (_, key) = scene.queue_present(id, now);
        assert!(scene.runtime.present_scheduler.defer_to_frame_tick(
            key,
            now,
            Duration::from_secs(1),
            false
        ));
    }
    // Software/in-flight/first-visibility occupancy shares the same registry.
    // Model non-reclaimable owners using real software resource entries.
    for id in 2000..2192 {
        scene
            .runtime
            .presentation_feedback
            .resources_mut()
            .begin_software(TransactionId::from_raw(id), None, None)
            .unwrap();
    }
    assert_eq!(
        scene
            .runtime
            .presentation_feedback
            .resources()
            .presentation_count(),
        256
    );
    scene
}

fn staged_batch(scene: &mut PresentScene, count: u32) -> crate::LiveProductionAuthorityBatch {
    let groups = (0..count)
        .map(|index| {
            let id = 3000 + u64::from(index);
            let handle = BufferHandle::from_raw(id);
            scene
                .runtime
                .presentation_feedback
                .resources_mut()
                .register_source(
                    present_descriptor(handle),
                    vec![std::fs::File::open("/dev/null").unwrap().into()],
                )
                .unwrap();
            let mut group = present_group(
                TransactionId::from_raw(id),
                SurfaceId::new(3000 + index, 1),
                handle,
            );
            group.present_submissions[0].layout_disposition =
                crate::LiveProductionPresentDisposition::StageLayout {
                    epoch: TransactionId::from_raw(99),
                };
            group
        })
        .collect();
    crate::LiveProductionAuthorityBatch {
        groups,
        dma_buf_registrations: vec![],
        fence_registrations: vec![],
        released_dma_bufs: vec![],
        released_fences: vec![],
    }
}

#[test]
fn batch_intake_reclaims_background_before_any_of_sixty_four_begins() {
    let mut scene = full_registry(Instant::now());
    let batch = staged_batch(&mut scene, 64);
    scene
        .runtime
        .run_batch(&batch, &[], None, None, &scene.scene, vec![], None)
        .unwrap();
    assert_eq!(scene.runtime.present_scheduler.frame_tick_parked(), 0);
    assert_eq!(
        scene
            .runtime
            .presentation_feedback
            .resources()
            .presentation_count(),
        256
    );
    // Non-background owners were not sacrificed for headroom.
    for id in 2000..2192 {
        assert!(
            scene
                .runtime
                .presentation_feedback
                .resources_mut()
                .poll_acquire_fence(TransactionId::from_raw(id))
                .unwrap()
        );
    }
}

#[test]
fn software_intake_also_reclaims_total_occupancy() {
    let mut scene = full_registry(Instant::now());
    let mut batch = staged_batch(&mut scene, 1);
    let group = &mut batch.groups[0];
    group.present_submissions.clear();
    group
        .software_present_submissions
        .push(crate::LiveProductionSoftwarePresentSubmission {
            candidate: group.transactions[0].key(),
            source_size: Size {
                width: 16,
                height: 16,
            },
            transaction: group.transaction,
            surface: group.transactions[0].surface,
            acquire_fence: None,
            idle_fence: None,
        });
    let transaction = group.transaction;
    scene
        .runtime
        .enqueue_software_presents(&batch.groups)
        .unwrap();
    assert_eq!(scene.runtime.present_scheduler.frame_tick_parked(), 63);
    assert_eq!(
        scene
            .runtime
            .presentation_feedback
            .resources()
            .presentation_count(),
        256
    );
    assert!(
        scene
            .runtime
            .presentation_feedback
            .resources_mut()
            .poll_acquire_fence(transaction)
            .unwrap()
    );
}

#[test]
fn timer_settlement_frees_resources_before_a_full_reissue_batch() {
    let now = Instant::now();
    let mut scene = full_registry(now);
    // Execute the same service as the owner tick. Visibility stays hidden.
    scene.replace_with(vec![]);
    scene
        .runtime
        .service_first_visibility_presentations(now + Duration::from_secs(1));
    assert_eq!(
        scene
            .runtime
            .presentation_feedback
            .resources()
            .presentation_count(),
        192
    );
    let batch = staged_batch(&mut scene, 64);
    scene
        .runtime
        .run_batch(&batch, &[], None, None, &scene.scene, vec![], None)
        .unwrap();
    assert_eq!(
        scene
            .runtime
            .presentation_feedback
            .resources()
            .presentation_count(),
        256
    );
    assert_eq!(scene.runtime.present_scheduler.frame_tick_parked(), 0);
}

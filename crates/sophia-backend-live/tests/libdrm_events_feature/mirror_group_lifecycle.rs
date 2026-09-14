fn head_id(raw: u64) -> sophia_engine::RenderHeadId {
    sophia_engine::RenderHeadId::from_raw(raw)
}

#[test]
fn primary_flip_completes_without_waiting_for_the_secondary() {
    let output = OutputId::from_raw(7);
    let frame = LiveProductionNativeFrameId::from_raw(41);
    let mut group =
        LiveProductionMirrorGroupLifecycle::new(output, [head_id(11), head_id(12)]).unwrap();

    assert_eq!(group.primary_head(), head_id(11));
    assert_eq!(group.begin(frame), LiveProductionMirrorGroupBegin::Started);
    assert_eq!(
        group.mark_submitted(head_id(11), frame),
        LiveProductionMirrorHeadTransition::GroupReady
    );
    assert_eq!(
        group.mark_submitted(head_id(12), frame),
        LiveProductionMirrorHeadTransition::Accepted
    );
    assert!(group.observe_flip_timing(head_id(11), frame, 90, 8_000));
    assert_eq!(
        group.mark_flipped(head_id(11), frame),
        LiveProductionMirrorHeadTransition::GroupReady
    );
    assert_eq!(group.completed_frame(), Some(frame));
    assert_eq!(group.flip_timing(), Some((frame.raw(), 8_000)));
    assert!(group.awaiting_flips());
    assert_eq!(group.displayed_frame(head_id(11)), Some(frame));
    assert_eq!(group.displayed_frame(head_id(12)), None);
}

#[test]
fn primary_advances_while_secondary_remains_in_flight() {
    let output = OutputId::from_raw(7);
    let first = LiveProductionNativeFrameId::from_raw(41);
    let next = LiveProductionNativeFrameId::from_raw(42);
    let mut group =
        LiveProductionMirrorGroupLifecycle::new(output, [head_id(11), head_id(12)]).unwrap();

    group.begin(first);
    group.mark_submitted(head_id(11), first);
    group.mark_submitted(head_id(12), first);
    group.observe_flip_timing(head_id(11), first, 90, 8_000);
    group.mark_flipped(head_id(11), first);

    assert_eq!(group.begin(next), LiveProductionMirrorGroupBegin::Started);
    assert!(group.head_may_submit_frame(head_id(11), next));
    assert!(!group.head_may_submit_frame(head_id(12), next));
    assert_eq!(
        group.mark_submitted(head_id(11), next),
        LiveProductionMirrorHeadTransition::GroupReady
    );
    assert!(group.generation_is_scanned(first));

    group.observe_flip_timing(head_id(12), first, 81, 9_000);
    assert_eq!(
        group.mark_flipped(head_id(12), first),
        LiveProductionMirrorHeadTransition::Accepted
    );
    assert!(group.head_may_submit_frame(head_id(12), next));
}

#[test]
fn lagging_head_coalesces_to_the_newest_generation() {
    let output = OutputId::from_raw(7);
    let first = LiveProductionNativeFrameId::from_raw(41);
    let skipped = LiveProductionNativeFrameId::from_raw(42);
    let latest = LiveProductionNativeFrameId::from_raw(43);
    let mut group =
        LiveProductionMirrorGroupLifecycle::new(output, [head_id(11), head_id(12)]).unwrap();

    group.begin(first);
    group.mark_submitted(head_id(12), first);
    group.begin(skipped);
    group.begin(latest);
    group.observe_flip_timing(head_id(12), first, 81, 9_000);
    group.mark_flipped(head_id(12), first);

    assert!(!group.head_may_submit_frame(head_id(12), skipped));
    assert!(group.head_may_submit_frame(head_id(12), latest));
}

#[test]
fn ordinary_successor_waits_until_the_primary_owns_a_present_generation() {
    let present = LiveProductionNativeFrameId::from_raw(77);
    let transaction = TransactionId::from_raw(574);
    let content = LiveProductionScanoutContent::MixedPresent {
        frame: present,
        transaction,
        nonzero_rgb_pixels: 0,
    };

    assert_eq!(
        reduce_live_production_mirror_generation_queue(Some(present), None, Some(content)),
        LiveProductionMirrorGenerationQueue::DeferUntilPrimarySubmission
    );
    assert_eq!(
        reduce_live_production_mirror_generation_queue(
            Some(present),
            Some(present),
            Some(content)
        ),
        LiveProductionMirrorGenerationQueue::Install
    );
    assert_eq!(
        reduce_live_production_mirror_generation_queue(
            Some(present),
            None,
            Some(LiveProductionScanoutContent::RetainedMixed {
                frame: present,
                nonzero_rgb_pixels: 0,
            })
        ),
        LiveProductionMirrorGenerationQueue::Install
    );
}

#[test]
fn mirror_group_rejects_unknown_stale_and_duplicate_work() {
    let output = OutputId::from_raw(7);
    let first = LiveProductionNativeFrameId::from_raw(41);
    let latest = LiveProductionNativeFrameId::from_raw(42);
    let mut group =
        LiveProductionMirrorGroupLifecycle::new(output, [head_id(11), head_id(12)]).unwrap();

    assert_eq!(group.begin(first), LiveProductionMirrorGroupBegin::Started);
    assert_eq!(group.begin(latest), LiveProductionMirrorGroupBegin::Started);
    assert_eq!(
        group.begin(first),
        LiveProductionMirrorGroupBegin::GenerationInFlight
    );
    assert_eq!(
        group.mark_submitted(head_id(99), latest),
        LiveProductionMirrorHeadTransition::UnknownHead
    );
    assert_eq!(
        group.mark_submitted(head_id(11), first),
        LiveProductionMirrorHeadTransition::WrongGeneration
    );
    assert_eq!(
        group.mark_flipped(head_id(11), latest),
        LiveProductionMirrorHeadTransition::NotSubmitted
    );
}

#[test]
fn mirror_group_initialization_is_head_scoped() {
    let output = OutputId::from_raw(7);
    let mut group =
        LiveProductionMirrorGroupLifecycle::new(output, [head_id(11), head_id(12)]).unwrap();

    assert!(!group.initialized());
    assert_eq!(
        group.mark_initialized(head_id(11)),
        LiveProductionMirrorHeadTransition::Accepted
    );
    assert_eq!(
        group.mark_initialized(head_id(12)),
        LiveProductionMirrorHeadTransition::GroupReady
    );
    assert!(group.initialized());
}

#[test]
fn aborted_mirror_group_cannot_admit_a_new_generation() {
    let output = OutputId::from_raw(7);
    let first = LiveProductionNativeFrameId::from_raw(41);
    let second = LiveProductionNativeFrameId::from_raw(42);
    let mut group =
        LiveProductionMirrorGroupLifecycle::new(output, [head_id(11), head_id(12)]).unwrap();

    group.begin(first);
    assert_eq!(
        group.mark_submitted(head_id(11), first),
        LiveProductionMirrorHeadTransition::GroupReady
    );
    assert!(group.abort(first));
    assert!(group.failed());
    assert_eq!(group.begin(second), LiveProductionMirrorGroupBegin::Poisoned);
}

#[test]
fn renderer_work_keeps_its_generation_identity_during_coalescing() {
    let current = LiveProductionNativeFrameId::from_raw(41);
    let next = LiveProductionNativeFrameId::from_raw(42);
    let current_content = LiveProductionScanoutContent::RetainedMixed {
        frame: current,
        nonzero_rgb_pixels: 10,
    };
    let next_content = LiveProductionScanoutContent::RetainedMixed {
        frame: next,
        nonzero_rgb_pixels: 11,
    };

    assert_eq!(
        live_production_mirror_head_work_frame(
            true,
            Some(current_content),
            Some(next_content)
        ),
        Some(current)
    );
    assert_eq!(
        live_production_mirror_head_work_frame(false, None, Some(next_content)),
        Some(next)
    );
}

#[test]
fn renderer_start_captures_content_even_without_a_submit_report() {
    let frame = LiveProductionNativeFrameId::from_raw(41);
    let content = LiveProductionScanoutContent::HeadComposition {
        frame,
        logical_content_checksum: 73,
        nonzero_rgb_pixels: 0,
    };
    let mut pending = Some(content);
    let mut rendering = None;

    assert_eq!(
        advance_live_production_renderer_content(false, true, &mut pending, &mut rendering),
        Ok(true)
    );
    assert_eq!(pending, None);
    assert_eq!(rendering, Some(content));
    assert_eq!(
        advance_live_production_renderer_content(true, false, &mut pending, &mut rendering),
        Ok(false)
    );
    assert_eq!(rendering, Some(content));
}

#[test]
fn renderer_start_refuses_missing_or_competing_content_identity() {
    let frame = LiveProductionNativeFrameId::from_raw(41);
    let content = LiveProductionScanoutContent::RetainedMixed {
        frame,
        nonzero_rgb_pixels: 0,
    };

    assert!(
        advance_live_production_renderer_content(false, true, &mut None, &mut None).is_err()
    );
    assert!(
        advance_live_production_renderer_content(
            false,
            true,
            &mut Some(content),
            &mut Some(content),
        )
        .is_err()
    );
}

#[test]
fn native_renderer_ownership_transition_is_not_gated_by_a_submit_report() {
    let source = include_str!("../../src/production_session/native_scanout.rs");
    let singleton = source
        .split_once("            self.observe_callbacks(index, report.page_flip_callbacks.clone());\n")
        .expect("singleton scanout observes callbacks")
        .1
        .split_once("            if let Some(submit) = report.rendered_primary_plane_scanout_submit")
        .expect("singleton scanout later handles its optional submit report")
        .0;

    assert!(singleton.contains("advance_live_production_renderer_content("));
}

#[test]
fn normal_mirror_retirement_cannot_reenter_scene_projection() {
    let source = include_str!("../../src/production_session/native_scanout.rs");
    let retirement = source
        .split_once("        pub fn retire_ready(\n")
        .expect("native scanout retains the normal retirement entry point")
        .1
        .split_once("        pub(crate) fn retire_ready_for_drain(\n")
        .expect("normal and drain retirement remain separate")
        .0;

    assert!(!retirement.contains("run_mirror_group_scene_tick"));
    assert!(!retirement.contains("CompositorBackendTickInput::default()"));
    assert!(retirement.contains("service_mirror_group_retirement"));
    assert!(!retirement.contains("promote_queued_mirror_generation"));
}

#[test]
fn native_head_identity_is_wired_from_sessions_to_engine_registry() {
    // Production reachability for the opaque head boundary: the constructor
    // must build the backend head table from the session head records, admit
    // reduced head targets into the Engine registry, and route callbacks by
    // head. Deleting any of the three ends unwires the boundary and fails
    // this test rather than leaving a dead record type behind.
    let source = include_str!("../../src/production_session/native_scanout.rs");
    assert!(source.contains("LiveProductionNativeHeadTable::from_records(sessions.head_records"));
    assert!(source.contains("presentation_outputs.admit(target)"));
    assert!(source.contains("head.head == callback.head"));

    let session_source =
        include_str!("../../src/hardware_validation/atomic_scanout_card/session.rs");
    assert!(session_source.contains("allocator.mint()"));
    assert!(session_source.contains("head_records.push(crate::LiveNativeHeadRecord {"));
}

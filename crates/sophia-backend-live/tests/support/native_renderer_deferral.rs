#![cfg(test)]

use super::*;

fn content(frame: u64) -> LiveProductionScanoutContent {
    LiveProductionScanoutContent::HeadComposition {
        frame: LiveProductionNativeFrameId::from_raw(frame),
        logical_content_checksum: frame,
        nonzero_rgb_pixels: 0,
    }
}

fn snapshot(
    head: &LiveProductionNativeHead,
    cursor_x: i32,
) -> sophia_engine::OutputFrameDamageSnapshot {
    sophia_engine::OutputFrameDamageSnapshot {
        damage_history: Default::default(),
        output: head.output,
        surfaces: Vec::new(),
        compositor_display_list: sophia_engine::CompositorDamageList::empty(head.output.id),
        software_cursor: Some(sophia_protocol::Rect {
            x: cursor_x,
            y: 0,
            width: 8,
            height: 8,
        }),
    }
}

fn rendering() -> LiveProductionNativeHead {
    let mut head = native_head_fixture::head(
        1,
        1,
        crate::LibdrmNativePlaneFormatCapabilities::parse(201, 0, &[]),
    );
    head.pending_content = Some(content(31));
    head.output_frames.queue(snapshot(&head, 0)).unwrap();
    assert_eq!(
        advance_live_production_renderer_content(
            false,
            true,
            &mut head.pending_content,
            &mut head.rendering_content
        ),
        Ok(true)
    );
    head.output_frames.mark_rendering().unwrap();
    head
}

fn restart(head: &mut LiveProductionNativeHead, frame: u64) {
    assert_eq!(
        advance_live_production_renderer_content(
            false,
            true,
            &mut head.pending_content,
            &mut head.rendering_content
        ),
        Ok(true),
        "a deferred render must be restartable without stale renderer ownership"
    );
    assert_eq!(head.rendering_content, Some(content(frame)));
    assert_eq!(head.pending_content, None);
    head.output_frames.mark_rendering().unwrap();
}

#[test]
fn deferred_frame_can_retry_without_a_new_generation() {
    let mut head = rendering();
    let old_snapshot = head.output_frames.rendering().unwrap().snapshot.clone();
    for _ in 0..2 {
        head.defer_renderer_content(content(31).frame(), None)
            .unwrap();
        restart(&mut head, 31);
        assert_eq!(
            head.output_frames.rendering().unwrap().snapshot,
            old_snapshot
        );
        assert!(head.output_frames.submitted().is_none());
        assert!(head.output_frames.presented().is_none());
    }
}

#[test]
fn deferred_frame_yields_to_the_newer_queued_identity_and_damage() {
    let mut head = rendering();
    let next = snapshot(&head, 8);
    head.pending_content = Some(content(32));
    head.output_frames.queue(next.clone()).unwrap();
    head.defer_renderer_content(content(32).frame(), None)
        .unwrap();
    restart(&mut head, 32);
    assert_eq!(head.output_frames.rendering().unwrap().snapshot, next);
}

#[test]
fn foreign_deferred_identity_is_refused_without_losing_either_frame() {
    let mut head = rendering();
    head.pending_content = Some(content(32));
    head.output_frames.queue(snapshot(&head, 8)).unwrap();
    let frames = head.output_frames.clone();
    assert!(
        head.defer_renderer_content(content(99).frame(), None)
            .is_err()
    );
    assert_eq!(head.rendering_content, Some(content(31)));
    assert_eq!(head.pending_content, Some(content(32)));
    assert_eq!(head.output_frames, frames);
    assert!(
        head.defer_renderer_content(content(31).frame(), None)
            .is_err(),
        "the exporter must retain the newest queued identity"
    );
}

#[test]
fn superseded_deferral_releases_its_old_cohort_after_the_sibling_settles() {
    let mut head = rendering();
    let sibling = sophia_engine::RenderHeadId::from_raw(2);
    let mut cohort = sophia_engine::OutputPresentationCohort::new(
        head.output.id,
        31,
        head.head,
        [head.head, sibling],
    )
    .unwrap();
    cohort.mark_skipped(sibling);
    // Retrying the same frame keeps this head's obligation.
    head.defer_renderer_content(content(31).frame(), Some(&mut cohort))
        .unwrap();
    assert!(!cohort.generation_releasable());
    restart(&mut head, 31);
    // A newer frame supersedes a second deferral. No old renderer owner remains.
    head.pending_content = Some(content(32));
    head.output_frames.queue(snapshot(&head, 8)).unwrap();
    head.defer_renderer_content(content(32).frame(), Some(&mut cohort))
        .unwrap();
    assert!(
        cohort.generation_releasable(),
        "superseded deferred head must settle its old cohort"
    );
    restart(&mut head, 32);
}

#[test]
fn mirror_and_singleton_settle_only_pending_worker_completion() {
    for source in [
        include_str!(
            "../../src/production_session/native_scanout/persistent_native_scanout/mirror_scene_tick.rs"
        ),
        include_str!(
            "../../src/production_session/native_scanout/persistent_native_scanout/singleton_tick.rs"
        ),
    ] {
        let pending = source
            .split_once("Status::ScanoutExportPending => {")
            .unwrap()
            .1
            .split_once("Status::AlreadyInFlight")
            .unwrap()
            .0;
        assert!(pending.contains("if worker_was_in_flight && !worker_is_in_flight"));
        assert!(pending.contains("self.settle_deferred_renderer_content("));
    }
}

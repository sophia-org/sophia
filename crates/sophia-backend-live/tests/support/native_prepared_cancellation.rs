#![cfg(test)]

use super::*;

fn head(id: u32) -> LiveProductionNativeHead {
    native_head_fixture::head(
        id,
        1,
        crate::LibdrmNativePlaneFormatCapabilities::parse(id + 200, 0, &[]),
    )
}

fn content(frame: u64) -> LiveProductionScanoutContent {
    LiveProductionScanoutContent::HeadComposition {
        frame: LiveProductionNativeFrameId::from_raw(frame),
        logical_content_checksum: frame,
        nonzero_rgb_pixels: 0,
    }
}

fn snapshot(head: &LiveProductionNativeHead) -> sophia_engine::OutputFrameDamageSnapshot {
    sophia_engine::OutputFrameDamageSnapshot {
        damage_history: Default::default(),
        output: head.output,
        surfaces: Vec::new(),
        compositor_display_list: sophia_engine::CompositorDamageList::empty(head.output.id),
        software_cursor: None,
    }
}

fn prepared_worker(head: &mut LiveProductionNativeHead, frame: u64) {
    head.rendering_content = Some(content(frame));
    head.prepared_group_frame = Some(content(frame).frame());
    head.prepared_worker_was_in_flight = true;
    head.output_frames.queue(snapshot(head)).unwrap();
    head.output_frames.mark_rendering().unwrap();
}

// Exercise the production cancellation settlement after supplied successful
// resource disposition. This does not run a renderer, DRM cancellation, or the
// full mirror tick. The custody tests cover resource destruction and refusal.
#[test]
fn coalescing_a_prepared_worker_frame_allows_the_successor_to_start() {
    let mut head = head(2);
    prepared_worker(&mut head, 31);
    head.pending_content = Some(content(32));
    head.output_frames.queue(snapshot(&head)).unwrap();

    head.finish_prepared_cancellation();
    assert_eq!(
        advance_live_production_renderer_content(
            false,
            true,
            &mut head.pending_content,
            &mut head.rendering_content,
        ),
        Ok(true),
        "a cancelled prepared frame must not retain renderer ownership"
    );
    assert_eq!(head.rendering_content, Some(content(32)));
    assert_eq!(head.pending_content, None);
    assert!(head.output_frames.mark_rendering().is_ok());
    assert_eq!(head.prepared_group_frame, None);
    assert!(!head.prepared_worker_was_in_flight);
}

#[test]
fn cancellation_discards_rendering_damage_without_publishing_or_losing_pending() {
    let mut head = head(1);
    // A displayed predecessor and a newer pending frame have separate custody.
    head.output_frames.queue(snapshot(&head)).unwrap();
    head.output_frames.mark_submitted().unwrap();
    head.output_frames.mark_presented().unwrap();
    let presented = head.output_frames.presented().cloned();
    prepared_worker(&mut head, 31);
    let mut next = snapshot(&head);
    next.software_cursor = Some(sophia_protocol::Rect {
        x: 8,
        y: 0,
        width: 8,
        height: 8,
    });
    head.output_frames.queue(next.clone()).unwrap();
    head.pending_content = Some(content(32));

    head.finish_prepared_cancellation();
    assert!(
        head.output_frames.rendering().is_none(),
        "cancelled damage is not rendering"
    );
    assert_eq!(head.output_frames.pending().unwrap().snapshot, next);
    assert_eq!(head.output_frames.presented(), presented.as_ref());
    assert!(head.output_frames.submitted().is_none());
    assert_eq!(head.pending_content, Some(content(32)));
}

#[test]
fn cancelling_inline_preparation_does_not_take_renderer_or_pending_content() {
    let mut head = head(1);
    prepared_worker(&mut head, 31);
    head.prepared_worker_was_in_flight = false;
    head.pending_content = Some(content(32));
    head.finish_prepared_cancellation();
    assert_eq!(head.rendering_content, Some(content(31)));
    assert!(head.output_frames.rendering().is_some());
    assert_eq!(head.pending_content, Some(content(32)));
}

#[test]
fn settling_one_heads_cancellation_does_not_release_its_sibling() {
    let mut first = head(1);
    let mut second = head(2);
    prepared_worker(&mut first, 31);
    prepared_worker(&mut second, 31);
    first.finish_prepared_cancellation();
    assert!(first.rendering_content.is_none());
    assert_eq!(second.rendering_content, Some(content(31)));
    assert!(second.output_frames.rendering().is_some());
    assert!(second.prepared_worker_was_in_flight);
}

#[test]
fn all_stored_prepared_cancellation_uses_the_successful_custody_boundary() {
    let source = include_str!(
        "../../src/production_session/native_scanout/persistent_native_scanout/frame_retirement.rs"
    );
    let cancellation = source
        .split_once("fn cancel_prepared_head_owner(")
        .unwrap()
        .1
        .split_once("pub fn release_displayed_output(")
        .unwrap()
        .0;
    let (accepted, refused) = cancellation.split_once("Err(prepared) =>").unwrap();
    assert!(accepted.contains(".cancel_prepared("));
    assert!(accepted.contains(".finish_prepared_cancellation();"));
    assert!(!refused.contains("finish_prepared_cancellation"));
    assert!(refused.contains(".prepared_scanout = Some(prepared)"));
    let installer = include_str!(
        "../../src/production_session/native_scanout/persistent_native_scanout/composition_installation.rs"
    );
    assert!(installer.contains("self.cancel_prepared_head_owner(queued.head_index, prepared)"));
}

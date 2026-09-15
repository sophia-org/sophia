#![cfg(feature = "gbm-probe")]

use sophia_engine::{
    ChromeDescriptorTable, CompositorDisplayCommand, CompositorDisplayList, CompositorNodeId,
    CompositorRgb8, DescriptorOverlayCandidate, DescriptorOverlayEntry, DescriptorOverlayNodeRole,
    HeadCompositorCommand, HeadCompositorText, HeadRenderTarget, RenderHeadId,
    ToplevelActionCapabilityRef, build_output_head_plans, descriptor_overlay_projection,
    output_scene_snapshot_from_committed_in_view,
};
use sophia_protocol::{
    AttentionState, ChromeDescriptor, DisplayLabel, OutputHeadMapping, OutputId, OutputTransform,
    Rect, Size, SurfaceId, TrustLevel,
};
use sophia_renderer_live::{
    COMPOSITOR_TEXT_CACHE_MAX_ENTRIES, CompositorTextRasterCache, IndicatorStripRasterCache,
    LiveOwnedMixedCompositionLayer, lower_head_composition_plan_with_caches,
};

const OUTPUT: OutputId = OutputId::from_raw(1);

fn action(slot: u16, generation: u64) -> ToplevelActionCapabilityRef {
    ToplevelActionCapabilityRef {
        token: 1_000 + u64::from(slot),
        issuer_epoch: 3,
        issuer_revocation_epoch: 4,
        recipient_epoch: 5,
        target_slot: slot,
        target_generation: generation,
    }
}

fn projection(entry_count: u16) -> sophia_engine::DescriptorOverlayProjection {
    let mut descriptors = ChromeDescriptorTable::default();
    let mut entries = Vec::new();
    for slot in 1..=entry_count {
        let surface = SurfaceId::new(u32::from(slot), 1);
        let generation = 20 + u64::from(slot);
        descriptors.upsert(ChromeDescriptor {
            surface,
            label: Some(DisplayLabel {
                text: format!("Window {slot}"),
                redacted: true,
            }),
            icon: None,
            trust_level: if slot % 2 == 0 {
                TrustLevel::Trusted
            } else {
                TrustLevel::Isolated
            },
            attention: if slot == entry_count {
                AttentionState::Notice
            } else {
                AttentionState::None
            },
            generation,
        });
        entries.push(DescriptorOverlayEntry {
            slot,
            surface,
            descriptor_generation: generation,
            action: action(slot, generation),
        });
    }
    descriptor_overlay_projection(
        &DescriptorOverlayCandidate {
            projection: 7,
            generation: 11,
            output: OUTPUT,
            broker_epoch: 3,
            broker_revocation_epoch: 4,
            shell_session_epoch: 5,
            selected_slot: Some(1),
            entries,
        },
        &descriptors,
        Rect {
            x: 0,
            y: 0,
            width: 1_920,
            height: 1_080,
        },
    )
    .unwrap()
}

fn target(head: u64, width: i32, height: i32) -> HeadRenderTarget {
    HeadRenderTarget {
        head: RenderHeadId::from_raw(head),
        output: OUTPUT,
        target_generation: 2,
        native_size: Size { width, height },
        scale: 1,
        refresh_millihz: 60_000,
        transform: OutputTransform::Normal,
        mapping: OutputHeadMapping::Fit,
    }
}

#[test]
fn one_logical_projection_lowers_independently_for_unequal_heads() {
    let projection = projection(2);
    let snapshot = output_scene_snapshot_from_committed_in_view(
        OUTPUT,
        9,
        Rect {
            x: 0,
            y: 0,
            width: 1_920,
            height: 1_080,
        },
        &[],
        CompositorDisplayList {
            output: OUTPUT,
            commands: projection.commands,
        },
        None,
    )
    .unwrap();
    let plans =
        build_output_head_plans(&snapshot, &[target(1, 1_920, 1_080), target(2, 1_280, 720)])
            .unwrap();
    let large_text = plans[0]
        .compositor
        .iter()
        .find_map(|command| match command {
            HeadCompositorCommand::Text(text) => Some(text),
            _ => None,
        })
        .unwrap();
    let small_text = plans[1]
        .compositor
        .iter()
        .find_map(|command| match command {
            HeadCompositorCommand::Text(text) => Some(text),
            _ => None,
        })
        .unwrap();
    assert_eq!(large_text.font_size_millis, 12_000);
    assert_eq!(small_text.font_size_millis, 7_992);
    assert!(
        (large_text.geometry.width * 2).abs_diff(small_text.geometry.width * 3) <= 1,
        "head-native projection may differ by at most one rounding pixel"
    );

    let mut indicator_cache = IndicatorStripRasterCache::default();
    let mut text_cache = CompositorTextRasterCache::default();
    let large = lower_head_composition_plan_with_caches(
        &plans[0],
        &[],
        &mut indicator_cache,
        &mut text_cache,
    )
    .unwrap();
    let small = lower_head_composition_plan_with_caches(
        &plans[1],
        &[],
        &mut indicator_cache,
        &mut text_cache,
    )
    .unwrap();

    assert_eq!(
        large
            .layers
            .iter()
            .filter(|layer| matches!(layer, LiveOwnedMixedCompositionLayer::Cpu { .. }))
            .count(),
        2
    );
    assert_eq!(
        small
            .layers
            .iter()
            .filter(|layer| matches!(layer, LiveOwnedMixedCompositionLayer::Cpu { .. }))
            .count(),
        2
    );
    let large_damage = large.output_damage_snapshot.unwrap();
    let small_damage = small.output_damage_snapshot.unwrap();
    assert_eq!(large_damage.output.size.width, 1_920);
    assert_eq!(small_damage.output.size.width, 1_280);
    assert_eq!(large_damage.compositor_display_list.texts().count(), 2);
    assert_eq!(small_damage.compositor_display_list.texts().count(), 2);
    assert_eq!(text_cache.stats().misses, 4);

    lower_head_composition_plan_with_caches(&plans[0], &[], &mut indicator_cache, &mut text_cache)
        .unwrap();
    assert_eq!(text_cache.stats().hits, 2);
}

fn text(text: String) -> HeadCompositorText {
    HeadCompositorText {
        node: CompositorNodeId::DescriptorOverlay {
            projection: 1,
            slot: 1,
            role: DescriptorOverlayNodeRole::Label,
        },
        generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 96,
            height: 32,
        },
        text,
        font_size_millis: 12_000,
        color: CompositorRgb8 {
            red: 0xee,
            green: 0xee,
            blue: 0xee,
        },
    }
}

#[test]
fn bundled_text_raster_is_deterministic_bounded_and_safe_to_evict() {
    let request = text("Browser".to_owned());
    let mut first_cache = CompositorTextRasterCache::default();
    let first = first_cache.raster_for(&request).unwrap();
    let retained = first_cache.raster_for(&request).unwrap();
    assert_eq!(first.handle, retained.handle);
    assert!(std::ptr::eq(
        first.bytes.as_slice(),
        retained.bytes.as_slice()
    ));
    assert!(first.bytes.chunks_exact(4).any(|pixel| pixel[3] != 0));

    let mut second_cache = CompositorTextRasterCache::default();
    let second = second_cache.raster_for(&request).unwrap();
    assert_eq!(first.bytes, second.bytes);

    for index in 0..=COMPOSITOR_TEXT_CACHE_MAX_ENTRIES {
        first_cache
            .raster_for(&text(format!("Entry {index}")))
            .unwrap();
    }
    let stats = first_cache.stats();
    assert_eq!(stats.entries, COMPOSITOR_TEXT_CACHE_MAX_ENTRIES);
    assert!(stats.evictions >= 2);
    assert!(stats.bytes <= sophia_renderer_live::COMPOSITOR_TEXT_CACHE_MAX_BYTES);
    assert!(first.bytes.chunks_exact(4).any(|pixel| pixel[3] != 0));

    assert!(
        projection(1)
            .commands
            .iter()
            .any(|command| matches!(command, CompositorDisplayCommand::Text(_)))
    );
}

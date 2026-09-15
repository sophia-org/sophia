#![cfg(test)]

use super::*;
use crate::{
    LibdrmNativePlaneFormatCapabilities as Capabilities,
    LibdrmNativePlaneFormatSnapshot as Snapshot, LibdrmNativePlaneFormatSnapshotUnknown as Unknown,
};
use sophia_protocol::{DRM_FORMAT_ARGB8888 as AR24, DRM_FORMAT_XRGB8888 as XR24};

fn blob(modifiers: &[(u64, u64)]) -> Vec<u8> {
    let mut bytes = vec![0; 32 + modifiers.len() * 24];
    for (offset, value) in [
        (0, 1u32),
        (8, 2),
        (12, 24),
        (16, modifiers.len() as u32),
        (20, 32),
        (24, XR24),
        (28, AR24),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
    }
    for (index, (mask, modifier)) in modifiers.iter().enumerate() {
        let offset = 32 + index * 24;
        bytes[offset..offset + 8].copy_from_slice(&mask.to_ne_bytes());
        bytes[offset + 16..offset + 24].copy_from_slice(&modifier.to_ne_bytes());
    }
    bytes
}

fn preference(format: u32, modifiers: Vec<u64>) -> LiveOutputAllocationFormatPreference {
    LiveOutputAllocationFormatPreference { format, modifiers }
}

#[test]
fn exact_format_preferences_preserve_distinct_modifier_order() {
    let xrgb_tiled = 0x0200_0000_28a0_1f04;
    let argb_tiled = 0x0200_0000_0040_1b03;
    let bytes = blob(&[(1, xrgb_tiled), (2, argb_tiled), (3, 0), (1, xrgb_tiled)]);
    let capabilities = Capabilities::parse(13, 77, &bytes);
    let legacy = capabilities.preferred_xrgb8888_modifiers.clone();
    let rows = allocation_format_preferences(&capabilities.snapshot);
    assert_eq!(
        rows,
        vec![
            preference(XR24, vec![xrgb_tiled, 0]),
            preference(AR24, vec![argb_tiled, 0]),
        ]
    );
    assert_eq!(legacy, rows[0].modifiers);
    assert_eq!(capabilities.preferred_xrgb8888_modifiers, legacy);
}

#[test]
fn a_format_without_explicit_layouts_has_no_preference_row() {
    for (mask, format) in [(1, XR24), (2, AR24)] {
        let snapshot = Snapshot::parse(13, 77, &blob(&[(mask, 0)]));
        assert_eq!(
            allocation_format_preferences(&snapshot),
            vec![preference(format, vec![0])]
        );
    }
    let snapshot = Snapshot::parse(13, 77, &blob(&[]));
    assert!(allocation_format_preferences(&snapshot).is_empty());
}

#[test]
fn unknown_preferences_do_not_change_legacy_renderer_admission() {
    for reason in [
        Unknown::Unavailable,
        Unknown::ReadFailed,
        Unknown::Malformed,
        Unknown::UnsupportedVersion,
        Unknown::CapacityExceeded,
        Unknown::ImplicitModifier,
    ] {
        assert!(allocation_format_preferences(&Snapshot::unavailable(13, reason)).is_empty());
    }

    // Reserved flags invalidate strict evidence without changing the legacy parser.
    let mut bytes = blob(&[(3, 0)]);
    bytes[4..8].copy_from_slice(&1u32.to_ne_bytes());
    let capabilities = Capabilities::parse(13, 77, &bytes);
    assert_eq!(
        capabilities.snapshot.unknown_reason(),
        Some(Unknown::Malformed)
    );
    assert!(allocation_format_preferences(&capabilities.snapshot).is_empty());
    assert_eq!(capabilities.preferred_xrgb8888_modifiers, vec![0]);
}

#[test]
fn invalid_sentinels_never_become_allocation_preferences() {
    let snapshot = Snapshot::parse(13, 77, &blob(&[(3, u64::MAX), (3, 0)]));
    assert_eq!(
        allocation_format_preferences(&snapshot),
        vec![preference(XR24, vec![0]), preference(AR24, vec![0])]
    );
    let snapshot = Snapshot::parse(13, 77, &blob(&[(3, u64::MAX)]));
    assert!(allocation_format_preferences(&snapshot).is_empty());

    let snapshot = Snapshot::parse(
        13,
        77,
        &blob(&[(3, sophia_protocol::DRM_FORMAT_MOD_INVALID), (3, 0)]),
    );
    assert_eq!(snapshot.unknown_reason(), Some(Unknown::ImplicitModifier));
    assert!(allocation_format_preferences(&snapshot).is_empty());
}

fn device() -> LiveRenderDeviceNodeIdentity {
    LiveRenderDeviceNodeIdentity {
        device: 1,
        inode: 2,
        device_number: 3,
    }
}

fn devices() -> LiveRenderDeviceState {
    let mut state = LiveRenderDeviceState::new();
    state.group_devices = vec![Some(device())];
    state
}

fn head(id: u32, output: u64, modifier: u64) -> LiveProductionNativeHead {
    let size = sophia_protocol::Size {
        width: 640,
        height: 480,
    };
    let output = sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(output),
        size,
        scale: 1,
    };
    LiveProductionNativeHead {
        head: sophia_engine::RenderHeadId::from_raw(u64::from(id)),
        enabled: true,
        group: 0,
        selection: crate::LibdrmNativePrimaryPlaneSelection::new(
            ::drm::control::from_u32(id).unwrap(),
            ::drm::control::from_u32(id + 100).unwrap(),
            ::drm::control::from_u32(id + 200).unwrap(),
            size,
            None,
        ),
        format_capabilities: Capabilities::parse(id + 200, u64::from(id), &blob(&[(3, modifier)])),
        scale: 1,
        refresh_millihz: 60_000,
        transform: sophia_protocol::OutputTransform::Normal,
        mapping: sophia_protocol::OutputHeadMapping::Fit,
        vrr: sophia_protocol::OutputVrrPolicy::Disabled,
        pending_callback: None,
        completion_mode: LiveProductionKmsCompletionMode::PageFlipPreferred,
        completion_fence_status: crate::LibdrmNativeCompletionFenceStatus::Unsupported,
        out_fence_retirements: 0,
        late_page_flip_events: 0,
        completion_fence_errors: 0,
        output,
        target_generation: 1,
        submitted_at: None,
        submitted_ust_usec: None,
        pending_nonzero_pixel_bytes: 0,
        last_checksum: 0,
        submitted_checksum: None,
        submitted_sequence: None,
        pending_content: None,
        rendering_content: None,
        submitted_content: None,
        submitted_direct: false,
        layout_witness: layout_retirement::NativeLayoutWitnessState::default(),
        presented_direct: false,
        presented_content: None,
        presented_logical_checksum: 0,
        presented_submissions: 0,
        service_skew_baseline: None,
        presented_submission_ust_usec: 0,
        presented_page_flip_ust_usec: 0,
        presented_submit_to_page_flip: Duration::ZERO,
        submissions: 0,
        retirements: 0,
        callback_accepted: 0,
        initial_modeset_submission: None,
        nonzero_exports: 0,
        last_submit_report: None,
        pending_cursor: None,
        pending_cursor_since: None,
        committed_cursor: None,
        cursor_properties: None,
        prepared_cursor_ride: None,
        scanout_custody: crate::PersistentScanoutCustody::default(),
        displayed_group_frame: None,
        prepared_scanout: None,
        prepared_group_frame: None,
        prepared_worker_was_in_flight: false,
        scanout_in_flight_ticks: 0,
        last_callback_serial: None,
        submitted_group_frame: None,
        output_frames: OutputFramePresentationState::new(output).unwrap(),
    }
}

#[test]
fn reordering_physical_heads_keeps_each_planes_preference() {
    let state = devices();
    let mut heads = [head(1, 20, 17), head(2, 10, 29)];
    for _ in 0..2 {
        for (output, id, modifier) in [(20, 1, 17), (10, 2, 29)] {
            let row = state
                .output_preference(&heads, OutputId::from_raw(output), false)
                .unwrap();
            assert_eq!(row.context.unwrap().head.raw(), id);
            assert_eq!(
                row.formats,
                vec![
                    preference(XR24, vec![modifier]),
                    preference(AR24, vec![modifier])
                ]
            );
            let physical = heads.iter().find(|head| head.head.raw() == id).unwrap();
            assert_eq!(
                physical.format_capabilities.snapshot.plane,
                physical.selection.plane_id()
            );
        }
        heads.sort_by_key(|head| head.output.id);
    }
}

#[test]
fn scalar_queries_and_rows_share_stable_context_without_minting() {
    let state = devices();
    let heads = [head(1, 10, 0)];
    let output = heads[0].output.id;
    let expected = state.output_context(&heads, output, false).unwrap();
    assert_eq!(expected.1, device());
    for _ in 0..8 {
        assert_eq!(state.output_context(&heads, output, false), Some(expected));
        assert_eq!(
            state
                .output_preference(&heads, output, false)
                .unwrap()
                .context,
            Some(expected.0)
        );
    }
}

#[test]
fn reconstruction_and_invalidation_never_reuse_restored_context_values() {
    let mut state = devices();
    let mut heads = [head(1, 10, 0)];
    let output = heads[0].output.id;
    let original = state.output_context(&heads, output, false).unwrap().0;
    heads[0].target_generation = 2;
    state.invalidate_context();
    let candidate = state.output_context(&heads, output, false).unwrap().0;
    heads[0].target_generation = original.target_generation;
    state.invalidate_context();
    let rollback = state.output_context(&heads, output, false).unwrap().0;
    state.invalidate_context();
    let same_values = state.output_context(&heads, output, false).unwrap().0;
    let replacement = devices().output_context(&heads, output, false).unwrap().0;
    let generations = [original, candidate, rollback, same_values, replacement]
        .map(|context| context.generation)
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert_eq!(generations.len(), 5);
    assert_eq!(rollback.head, original.head);
    assert_eq!(rollback.target_generation, original.target_generation);
    assert_eq!(same_values.head, rollback.head);
    assert_eq!(same_values.target_generation, rollback.target_generation);
}

#[test]
fn preparation_mirrors_disabled_heads_and_unknown_devices_have_no_context() {
    let mut state = devices();
    let mut heads = vec![head(1, 10, 0)];
    let output = heads[0].output.id;
    assert!(state.output_context(&heads, output, false).is_some());
    assert!(state.output_context(&heads, output, true).is_none());
    assert!(
        state
            .output_context(&heads, OutputId::from_raw(99), false)
            .is_none()
    );
    heads[0].enabled = false;
    assert!(state.output_context(&heads, output, false).is_none());
    heads[0].enabled = true;
    heads.push(head(2, 10, 0));
    assert!(state.output_context(&heads, output, false).is_none());
    heads[1].enabled = false;
    assert!(state.output_context(&heads, output, false).is_none());
    assert!(
        state
            .output_preference(&heads, output, false)
            .unwrap()
            .context
            .is_none()
    );
    heads.pop();
    state.group_devices[0] = None;
    assert!(state.output_context(&heads, output, false).is_none());
}

#[test]
fn exhausted_context_identity_suppresses_evidence_without_losing_format_rows() {
    let counter = AtomicU64::new(u64::MAX - 1);
    assert_eq!(next_allocation_context(&counter), Some(u64::MAX - 1));
    assert_eq!(next_allocation_context(&counter), None);
    assert_eq!(next_allocation_context(&counter), None);
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
    let mut state = devices();
    let heads = [head(1, 10, 0)];
    let output = heads[0].output.id;
    state.context_generation = next_allocation_context(&counter);
    assert!(state.output_context(&heads, output, false).is_none());
    let row = state.output_preference(&heads, output, false).unwrap();
    assert_eq!(row.context, None);
    assert_eq!(row.identity, Some(device()));
    assert_eq!(
        row.formats,
        vec![preference(XR24, vec![0]), preference(AR24, vec![0])]
    );
}

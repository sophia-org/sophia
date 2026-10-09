use sophia_renderer_live::LiveRendererScanoutBufferExportDetail as Detail;

// Only compiler-owned error variants and exact internal invariant messages
// cross into ordinary records. Never copy arbitrary Display or Debug text.
const RENDERER_CODES: &[(Detail, &str)] = &[
    (Detail::Exported, "renderer_exported"),
    (Detail::WorkerPending, "renderer_worker_pending"),
    (Detail::WorkerQueueFull, "renderer_worker_queue_full"),
    (Detail::WorkerDisconnected, "renderer_worker_disconnected"),
    (Detail::WorkerStalled, "renderer_worker_stalled"),
    (Detail::InvalidTarget, "renderer_invalid_target"),
    (Detail::ComposeRefused, "renderer_compose_refused"),
    (
        Detail::BackendDeviceUnavailable,
        "renderer_backend_device_unavailable",
    ),
    (
        Detail::GbmDeviceUnavailable,
        "renderer_gbm_device_unavailable",
    ),
    (Detail::EglUnavailable, "renderer_egl_unavailable"),
    (
        Detail::EglDisplayUnavailable,
        "renderer_egl_display_unavailable",
    ),
    (
        Detail::EglInitializeFailed,
        "renderer_egl_initialize_failed",
    ),
    (Detail::EglBindApiFailed, "renderer_egl_bind_api_failed"),
    (
        Detail::EglConfigUnavailable,
        "renderer_egl_config_unavailable",
    ),
    (
        Detail::GbmSurfaceUnavailable,
        "renderer_gbm_surface_unavailable",
    ),
    (
        Detail::EglSurfaceUnavailable,
        "renderer_egl_surface_unavailable",
    ),
    (
        Detail::EglContextUnavailable,
        "renderer_egl_context_unavailable",
    ),
    (
        Detail::EglMakeCurrentFailed,
        "renderer_egl_make_current_failed",
    ),
    (Detail::GlSmokeFailed, "renderer_gl_smoke_failed"),
    (
        Detail::CpuLayerUploadFailed,
        "renderer_cpu_layer_upload_failed",
    ),
    (
        Detail::DmaBufImageCreateFailed,
        "renderer_dma_buf_image_create_failed",
    ),
    (
        Detail::DmaBufImageBindFailed,
        "renderer_dma_buf_image_bind_failed",
    ),
    (
        Detail::CompositionDrawFailed,
        "renderer_composition_draw_failed",
    ),
    (
        Detail::CompositionFinishFailed,
        "renderer_composition_finish_failed",
    ),
    (
        Detail::EglImageDestroyFailed,
        "renderer_egl_image_destroy_failed",
    ),
    (Detail::DmaBufImportFailed, "renderer_dma_buf_import_failed"),
    (
        Detail::EglSwapBuffersFailed,
        "renderer_egl_swap_buffers_failed",
    ),
    (
        Detail::FrontBufferLockFailed,
        "renderer_front_buffer_lock_failed",
    ),
    (
        Detail::InvalidBufferDescriptor,
        "renderer_invalid_buffer_descriptor",
    ),
    (
        Detail::InvalidRendererImageId,
        "renderer_invalid_renderer_image_id",
    ),
    (
        Detail::DmaBufDescriptorMismatch,
        "renderer_dma_buf_descriptor_mismatch",
    ),
    (
        Detail::DmaBufImportCacheFull,
        "renderer_dma_buf_import_cache_full",
    ),
    (
        Detail::RendererImageStoreFull,
        "renderer_renderer_image_store_full",
    ),
    (
        Detail::RetainedBufferMissing,
        "renderer_retained_buffer_missing",
    ),
    (
        Detail::PendingFrameMissing,
        "renderer_pending_frame_missing",
    ),
    (
        Detail::ExportedDescriptorMissing,
        "renderer_exported_descriptor_missing",
    ),
    (
        Detail::ExportedOwnerMissing,
        "renderer_exported_owner_missing",
    ),
    (
        Detail::WorkerLeaseIdExhausted,
        "renderer_worker_lease_id_exhausted",
    ),
    (
        Detail::FrameSlotIncarnationExhausted,
        "renderer_frame_slot_incarnation_exhausted",
    ),
];
const PREVIEW_REFUSAL_CODES: &[&str] = &[
    "preview_image_pending",
    "preview_image_missing",
    "preview_image_cross_device",
];

const INVARIANT_CODES: &[(&str, &str)] = &[
    (
        "native completion descriptor failed",
        "native_completion_descriptor_failed",
    ),
    (
        "topology composition display list invalid",
        "topology_composition_invariant",
    ),
    (
        "preview retry found a non-local source",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery found a conflicting shell retirement claim",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery display list invalid",
        "preview_recovery_invariant",
    ),
    (
        "single output composition was deferred",
        "native_composition_deferred",
    ),
    (
        "native frame service export failed without detail",
        "native_frame_service_failed",
    ),
    (
        "native frame service target not ready",
        "native_frame_service_failed",
    ),
    (
        "native frame service frame target unavailable",
        "native_frame_service_failed",
    ),
    (
        "native frame service plane submit failed",
        "native_frame_service_failed",
    ),
    (
        "independent native submit lost its content",
        "native_frame_service_failed",
    ),
    (
        "preview recovery already owns this output",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery cannot skip a partially submitted Present",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery cannot withdraw a submitted frame",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery could not transfer its unsubmitted frame mapping",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery did not admit its replacement",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery lost its Present image",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery lost its Present transaction",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery lost its exact failure owner",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery lost its frozen sources",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery lost its prepared candidate",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery lost its unsubmitted Present",
        "preview_recovery_invariant",
    ),
    (
        "preview recovery tried to replace a submitted Present frame",
        "preview_recovery_invariant",
    ),
    (
        "preview replacement does not own the withdrawn output",
        "preview_recovery_invariant",
    ),
    (
        "software preview recovery admitted no frame",
        "preview_recovery_invariant",
    ),
    (
        "software preview recovery cannot replace a submitted frame",
        "preview_recovery_invariant",
    ),
    (
        "software preview recovery does not own an unsubmitted frame",
        "preview_recovery_invariant",
    ),
    (
        "software preview recovery lost its binding",
        "preview_recovery_invariant",
    ),
    (
        "software preview recovery lost its frame mapping",
        "preview_recovery_invariant",
    ),
    (
        "software preview recovery lost its source set",
        "preview_recovery_invariant",
    ),
    (
        "software preview recovery reused a native frame identity",
        "preview_recovery_invariant",
    ),
    (
        "native renderer registry replaced with live readers or evictions",
        "renderer_image_registry_custody",
    ),
    (
        "native preview recovery admission bypassed readiness",
        "preview_recovery_admission",
    ),
    (
        "public WM projection has no reconciled content placement",
        "wm_missing_reconciled_content",
    ),
    (
        "public WM retained output has no committed content placement",
        "wm_missing_retained_content",
    ),
    (
        "persistent live session received no composable X pixels",
        "session_no_composable_pixels",
    ),
    (
        "persistent Present resources did not retire exactly once",
        "session_present_retirement",
    ),
    (
        "persistent session controls did not drain cleanly",
        "session_control_drain",
    ),
    (
        "persistent client key state did not drain cleanly",
        "session_key_drain",
    ),
    (
        "renderer-image handoff targets an unknown output",
        "handoff_unknown_output",
    ),
    (
        "retained scene refers to an unavailable promoted renderer image",
        "handoff_missing_image",
    ),
    (
        "renderer-image handoff does not cover the retained scene",
        "handoff_coverage_mismatch",
    ),
    (
        "renderer-image handoff is unexpectedly missing",
        "handoff_missing",
    ),
    (
        "renderer-image handoff head coverage changed during replacement",
        "handoff_head_coverage_changed",
    ),
    (
        "renderer-image handoff names an unavailable connector",
        "handoff_connector_unavailable",
    ),
];

pub fn failure_code(error: &(dyn std::error::Error + 'static)) -> &'static str {
    if let Some(code) = super::recovery::failure_code(error) {
        return code;
    }
    if let Some(code) = super::output_profile::failure_code(error) {
        return code;
    }
    if let Some(detail) = error.downcast_ref::<Detail>() {
        return RENDERER_CODES
            .iter()
            .find(|(candidate, _)| candidate == detail)
            .map_or("unclassified", |(_, code)| *code);
    }
    #[cfg(feature = "native-session")]
    if let Some(refusal) = error.downcast_ref::<sophia_backend_live::LivePreviewImageRefusal>() {
        use sophia_backend_live::LivePreviewImageRefusal as R;
        return match refusal {
            R::Pending { .. } => "preview_image_pending",
            R::Missing { .. } => "preview_image_missing",
            R::CrossDevice { .. } => "preview_image_cross_device",
            R::Renderer { detail, .. } => failure_code(detail),
        };
    }
    let message = error.to_string();
    INVARIANT_CODES
        .iter()
        .find(|(candidate, _)| *candidate == message)
        .map_or("unclassified", |(_, code)| *code)
}

pub(super) fn approved_failure_code(value: &str) -> bool {
    PREVIEW_REFUSAL_CODES.contains(&value)
        || super::recovery::CODES.contains(&value)
        || super::output_profile::CODES.contains(&value)
        || value == "unclassified"
        || RENDERER_CODES.iter().any(|(_, code)| *code == value)
        || INVARIANT_CODES.iter().any(|(_, code)| *code == value)
}

mod refresh;
pub use refresh::head_refresh_interval;

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
mod persistent_native_scanout;

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
pub use persistent_native_scanout::{
    LIVE_PRODUCTION_PAGE_FLIP_HARD_STALL, LiveOutputAllocationContext,
    LiveOutputAllocationFormatPreference, LiveOutputAllocationPreference,
    LivePersistentRenderMetrics, LiveProductionCompletionTimestamp,
    LiveProductionCpuFrameQueueStatus, LiveProductionDirectScanoutTotals,
    LiveProductionHeadCompositionFrame, LiveProductionKmsCompletionSource,
    LiveProductionMirrorGenerationQueue, LiveProductionMirrorGroupBegin,
    LiveProductionMirrorGroupLifecycle, LiveProductionMirrorHeadTransition,
    LiveProductionNativeFrameRetirement, LiveProductionNativeHead, LiveProductionNativeScanout,
    LiveProductionNativeTopologyApplyCoordinator, LiveProductionNativeTopologyApplyPhase,
    LiveProductionNativeTopologyApplyTransition, LiveProductionNativeTopologyCandidateResource,
    LiveProductionNativeTopologyCurrentHead, LiveProductionNativeTopologyDisposition,
    LiveProductionNativeTopologyHeadPlan, LiveProductionNativeTopologyPlan,
    LiveProductionNativeTopologyPlanError, LiveProductionNativeTopologyPreparationPhase,
    LiveProductionNativeTopologyPreparationReport, LiveProductionNativeTopologyResourceCohort,
    LiveProductionNativeTopologyResourceRejection, LiveProductionNativeTopologyResourceTransition,
    LiveProductionPageFlipWatchdogStatus, LiveProductionRendererImageHandoff,
    LiveProductionRetainedFrameQueueRequirement, LiveProductionRetainedSceneQueueStatus,
    LiveProductionRetiredLayoutWitness, LiveProductionScanoutContent,
    LiveProductionSemanticStartupBarrier, LiveRenderDeviceNodeIdentity,
    advance_live_production_renderer_content, finish_live_production_native_initialization,
    live_production_mirror_head_work_frame, live_production_retained_frame_requirement,
    live_production_scanout_is_stable_present, live_topology_frame_renderer_image_requirements,
    plan_live_production_native_topology, project_live_production_published_topology,
    project_mirror_output_damage_snapshot, project_native_cursor_logical_viewport,
    reduce_live_production_completion_timestamp, reduce_live_production_cpu_frame_queue,
    reduce_live_production_head_render_target, reduce_live_production_mirror_generation_queue,
    reduce_live_production_page_flip_watchdog, reduce_live_production_retained_frame_queue,
    reduce_live_production_retained_scene_queue, reduce_live_production_semantic_startup_barrier,
    validate_live_head_composition_frame_batch, validate_live_production_rollback_topology,
    validate_live_production_topology_frames,
};

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
pub(crate) use persistent_native_scanout::LiveProductionNativeRetirementContent;

#[cfg(all(test, feature = "libdrm-events", feature = "gbm-probe"))]
pub(crate) use persistent_native_scanout::{
    CompositionInstallation, CompositionInstaller, DeferredNativeCompositions,
    LiveProductionHeadCompositionContent, LiveProductionQueuedMirrorHeadFrame,
    MirrorCompletionWitness, NativeCompositionInstallationHead, NativeCompositionOutput,
    PresentedTimingHead, SettledMirrorHead, complete_mirror_head, completed_timing,
    install_composition_generation, prepare_native_composition_batch,
    reserve_composition_lifecycle, settled_mirror_checksum, validate_composition_installation,
};

#[derive(Debug)]
pub struct LiveNativeMixedDiagnosticComplete {
    pub status: crate::LiveRendererScanoutBufferExportStatus,
    pub detail: crate::LiveRendererScanoutBufferExportDetail,
    pub cpu_layers: usize,
    pub dmabuf_layers: usize,
    pub live_sources: usize,
    pub live_fences: usize,
    pub live_transactions: usize,
}

impl LiveNativeMixedDiagnosticComplete {
    pub fn reduced_log_line(&self, child_outcome: &str) -> String {
        format!(
            "sophia_native_egl_mixed schema=1 case=mixed status={:?} stage={:?} cpu_layers={} dmabuf_layers={} child_outcome={} live_sources={} live_fences={} live_transactions={}",
            self.status,
            self.detail,
            self.cpu_layers,
            self.dmabuf_layers,
            child_outcome,
            self.live_sources,
            self.live_fences,
            self.live_transactions,
        )
    }
}

impl std::fmt::Display for LiveNativeMixedDiagnosticComplete {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.reduced_log_line("completed"))
    }
}

impl std::error::Error for LiveNativeMixedDiagnosticComplete {}

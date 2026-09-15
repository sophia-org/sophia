#[cfg(feature = "libdrm-events")]
use super::*;
#[cfg(feature = "libdrm-events")]
use crate::prelude::*;
#[cfg(feature = "libdrm-events")]
use std::{any::Any, collections::VecDeque};

#[cfg(feature = "libdrm-events")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn track_rendered_primary_plane_scanout_submit_from_target_and_selection_with<D, E>(
    scanout_target: LiveKmsScanoutTargetStatus,
    output_size: Option<Size>,
    target: Option<LiveGbmEglFrameTargetRecord>,
    custody: &mut crate::PersistentScanoutCustody,
    rendered_primary_plane_runtime_scanout_state: &mut Option<RuntimeScanoutState>,
    rendered_primary_plane_scanout_in_flight_ticks: &mut u64,
    submitted_after_page_flip_serial: Option<u64>,
    pending_runtime_scanout_states: Option<&mut VecDeque<RuntimeScanoutState>>,
    selection: LibdrmNativePrimaryPlaneSelectionResult,
    vrr_enabled: Option<bool>,
    cursor_ride: Option<crate::LibdrmNativeAtomicCursor>,
    device: &D,
    exporter: &mut E,
) -> LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport
where
    D: LibdrmNativePropertyLookupDevice
        + LibdrmNativePrimaryPlaneResourceDevice
        + LibdrmNativeAtomicCommitDevice,
    E: LiveRenderedScanoutBufferExporter,
    E::Owner: LiveRenderedScanoutBufferPrimeSource + 'static,
{
    if custody.submitted().is_some() {
        *rendered_primary_plane_runtime_scanout_state = Some(RuntimeScanoutState::Deferred);
        push_pending_runtime_scanout_state(
            pending_runtime_scanout_states,
            RuntimeScanoutState::Deferred,
        );
        return LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
            status: LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::AlreadyInFlight,
            scanout_target,
            output_size,
            target: target.map(|target| target.status),
            target_size: target.map(|target| target.size),
            export: None,
            scanout_buffer: None,
            buffer_format: None,
            buffer_modifier: None,
            buffer_planes: None,
            properties: None,
            format_table: None,
            resources: None,
            framebuffer: None,
            request: None,
            submit: None,
            request_scope: None,
            commit_flags: None,
            commit_submit: None,
            atomic_test: None,
            layout_witness: None,
            runtime_scanout_state: Some(RuntimeScanoutState::Deferred),
            in_flight: true,
            in_flight_ticks: *rendered_primary_plane_scanout_in_flight_ticks,
            cleanup_pending: custody.cleanup_pending(),
            cursor_dropped: false,
        };
    }

    if custody.cleanup_pending() {
        *rendered_primary_plane_runtime_scanout_state = Some(RuntimeScanoutState::Deferred);
        push_pending_runtime_scanout_state(
            pending_runtime_scanout_states,
            RuntimeScanoutState::Deferred,
        );
        return LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
            status: LiveTrackedRenderedPrimaryPlaneScanoutSubmitStatus::CleanupPending,
            scanout_target,
            output_size,
            target: target.map(|target| target.status),
            target_size: target.map(|target| target.size),
            export: None,
            scanout_buffer: None,
            buffer_format: None,
            buffer_modifier: None,
            buffer_planes: None,
            properties: None,
            format_table: None,
            resources: None,
            framebuffer: None,
            request: None,
            submit: None,
            request_scope: None,
            commit_flags: None,
            commit_submit: None,
            atomic_test: None,
            layout_witness: None,
            runtime_scanout_state: Some(RuntimeScanoutState::Deferred),
            in_flight: false,
            in_flight_ticks: *rendered_primary_plane_scanout_in_flight_ticks,
            cleanup_pending: true,
            cursor_dropped: false,
        };
    }

    let mut result = submit_rendered_primary_plane_scanout_from_scanout_target_and_selection_with(
        scanout_target,
        target,
        selection,
        vrr_enabled,
        cursor_ride,
        device,
        exporter,
    );
    let runtime_scanout_state = Some(result.runtime_scanout_state());

    if let Some(submission) = result.submission.take() {
        custody
            .accept_submission(
                submission
                    .with_submitted_after_page_flip_serial(submitted_after_page_flip_serial)
                    .map_scanout_buffer(|owner| Box::new(owner) as Box<dyn Any>),
            )
            .expect("submission capacity checked before device call");
    }
    let cleanup_pending = result.cleanup.is_some();
    if let Some(cleanup) = result.cleanup.take() {
        custody
            .accept_cleanup(cleanup.map_scanout_buffer(|owner| Box::new(owner) as Box<dyn Any>))
            .expect("cleanup capacity checked before device call");
    }
    *rendered_primary_plane_scanout_in_flight_ticks = 0;
    *rendered_primary_plane_runtime_scanout_state = runtime_scanout_state;
    if let Some(state @ (RuntimeScanoutState::Rejected | RuntimeScanoutState::Deferred)) =
        runtime_scanout_state
    {
        push_pending_runtime_scanout_state(pending_runtime_scanout_states, state);
    }

    LiveTrackedRenderedPrimaryPlaneScanoutSubmitReport {
        status: result.status.into(),
        scanout_target: result.scanout_target,
        output_size,
        target: result.target,
        target_size: target.map(|target| target.size),
        export: result.export,
        scanout_buffer: result.scanout_buffer,
        buffer_format: result.buffer_format,
        buffer_modifier: result.buffer_modifier,
        buffer_planes: result.buffer_planes,
        properties: result.properties,
        format_table: result.format_table,
        resources: result.resources,
        framebuffer: result.framebuffer,
        request: result.request,
        submit: result.submit,
        request_scope: result.request_scope,
        commit_flags: result.commit_flags,
        commit_submit: result.commit_submit,
        atomic_test: result.atomic_test,
        layout_witness: result.layout_witness,
        runtime_scanout_state,
        in_flight: custody.submitted().is_some(),
        in_flight_ticks: *rendered_primary_plane_scanout_in_flight_ticks,
        cleanup_pending,
        cursor_dropped: result.cursor_dropped,
    }
}

fn push_pending_runtime_scanout_state(
    pending_runtime_scanout_states: Option<&mut VecDeque<RuntimeScanoutState>>,
    state: RuntimeScanoutState,
) {
    if let Some(pending_runtime_scanout_states) = pending_runtime_scanout_states {
        pending_runtime_scanout_states.push_back(state);
    }
}

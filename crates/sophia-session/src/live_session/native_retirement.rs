use super::*;

/// Records each retired Present the Engine refused to apply. The screen shows
/// that candidate while the committed set does not, which is the state a
/// reader needs named when the next frame looks wrong.
pub(super) fn record_discarded_presents(
    discarded: &[sophia_backend_live::LiveProductionDiscardedPresent],
) {
    for present in discarded {
        let outcome = match present.outcome {
            sophia_protocol::TransactionOutcome::RejectedStaleSurface => "stale_surface",
            sophia_protocol::TransactionOutcome::RejectedInvalidSurface => "invalid_surface",
            sophia_protocol::TransactionOutcome::TimedOut => "timed_out",
            sophia_protocol::TransactionOutcome::Committed => "committed",
        };
        crate::session_println!(
            "sophia_live_session_present schema=5 status=discarded transaction={} surface={} outcome={outcome} baseline_generation={} current_generation={}",
            present.transaction.raw(),
            present.surface.index(),
            present.baseline_generation,
            present.current_generation,
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct NativePresentRetirementObservation {
    pub surface: SurfaceId,
    pub stable: bool,
    pub ust_usec: u64,
    pub msc: u64,
}

pub(super) fn record_native_present_retirement(
    layout: &mut PersistentLiveLayout,
    runtime: &LiveProductionVisualRuntime,
    native_scanout: &LiveProductionNativeScanout,
    retired: LiveProductionRetiredPresent,
    retired_present_surfaces: &mut BTreeMap<SurfaceId, TransactionId>,
    startup_surface_presentations: &mut StartupSurfacePresentationEvidence,
    startup_readiness: &mut SessionStartupReadiness,
) -> NativePresentRetirementObservation {
    let _ = layout.complete_visual_commit(retired.candidate, retired.source_size);
    layout.complete_admission_retirement(retired.candidate);
    let stable = runtime.stable_present(native_scanout, retired.transaction, &retired.outputs);
    let nonzero_rgb_pixels = native_scanout.presented_mixed_nonzero_rgb_pixels(retired.transaction);
    retired_present_surfaces.insert(retired.surface, retired.transaction);
    if stable {
        startup_surface_presentations.observe_stable(retired.surface, nonzero_rgb_pixels);
        let _ = reduce_session_startup(
            startup_readiness,
            SessionStartupEvent::StablePresented(retired.surface),
        );
    }

    let clip = retired.clip.map_or_else(
        || "none".to_owned(),
        |clip| format!("{}x{}_{}_{}", clip.width, clip.height, clip.x, clip.y),
    );
    crate::session_println!(
        "sophia_live_session_present schema=2 status=retired transaction={} surface={} source={}x{} target={}x{}_{}_{} clip={} unit_scale={}",
        retired.transaction.raw(),
        retired.surface.index(),
        retired.source_size.width,
        retired.source_size.height,
        retired.target.width,
        retired.target.height,
        retired.target.x,
        retired.target.y,
        clip,
        retired.source_size.width == retired.target.width
            && retired.source_size.height == retired.target.height,
    );
    // `pending_primary` was `!stable` restated, and stability no longer has
    // anything to do with what is queued behind this flip. The pixel count is
    // what a reader actually needs to tell "shown but blank" from "not shown".
    crate::session_println!(
        "sophia_live_session_scanout schema=2 status={} kind=mixed transaction={} nonzero_rgb_pixels={nonzero_rgb_pixels}",
        if stable { "stable" } else { "superseded" },
        retired.transaction.raw(),
    );

    if let Some(layout) = retired.layout_witness
        && let Some(trace) = layout.witness.alternative.trace
    {
        let witness = layout.witness;
        crate::session_println!(
            "sophia_live_layout_probe schema=2 status=RetiredCopy transaction={} output={} scene_generation={} source_image={} native_generation={} format={} original_modifier={} alternative_modifier={}",
            retired.transaction.raw(),
            trace.output.raw(),
            trace.scene_generation,
            witness.source_image.raw(),
            layout.context_generation,
            witness.format,
            witness.original_modifier,
            witness.alternative_modifier,
        );
    }

    NativePresentRetirementObservation {
        surface: retired.surface,
        stable,
        ust_usec: retired.ust_usec,
        msc: retired.msc,
    }
}

pub(super) fn record_native_software_present_retirement(
    layout: &mut PersistentLiveLayout,
    retired: sophia_backend_live::LiveProductionRetiredSoftwarePresent,
) {
    let _ = layout.complete_visual_commit(retired.candidate, retired.source_size);
    layout.complete_admission_retirement(retired.candidate);
    crate::session_println!(
        "sophia_live_session_present schema=4 status=retired transaction={} surface={} source={}x{} kind=software frame={} native_submission={} ust={} msc={}",
        retired.candidate.transaction.raw(),
        retired.candidate.surface.index(),
        retired.source_size.width,
        retired.source_size.height,
        retired.frame.raw(),
        retired.native_submission,
        retired.ust_usec,
        retired.msc,
    );
}

pub(super) fn correlate_physical_input_page_flip(
    input_delivery_complete: bool,
    input_pixel_change: bool,
    input_raw_ingress_msec: Option<u64>,
    input_change_submission_baseline: Option<usize>,
    input_change_frame_baseline: Option<u64>,
    native_scanout: &LiveProductionNativeScanout,
    input_presented_ust_usec: &mut Option<u64>,
    input_submit_to_page_flip: &mut Option<Duration>,
) {
    if input_presented_ust_usec.is_some() {
        return;
    }
    let (Some(ingress_ust_usec), Some(baseline_submission), Some(baseline_frame), Some(head)) = (
        input_raw_ingress_msec.and_then(|msec| msec.checked_mul(1_000)),
        input_change_submission_baseline,
        input_change_frame_baseline,
        native_scanout.heads.first(),
    ) else {
        return;
    };
    if !physical_input_page_flip_correlates(
        input_delivery_complete,
        input_pixel_change,
        ingress_ust_usec,
        baseline_submission,
        head.presented_submissions,
        baseline_frame,
        head.presented_content
            .map_or(0, |content| content.frame().raw()),
        head.presented_submission_ust_usec,
        head.presented_page_flip_ust_usec,
    ) {
        return;
    }
    *input_presented_ust_usec = Some(head.presented_page_flip_ust_usec);
    *input_submit_to_page_flip = Some(head.presented_submit_to_page_flip);
}

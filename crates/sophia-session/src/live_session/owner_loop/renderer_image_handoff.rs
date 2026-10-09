fn capture_renderer_image_handoff(
    runtime: &mut LiveProductionVisualRuntime,
    native_scanout: &mut LiveProductionNativeScanout,
) -> Result<sophia_backend_live::LiveProductionRendererImageHandoff, Box<dyn std::error::Error>> {
    // Images no store held at the last restore travel as snapshots the
    // runtime kept; only the rest are exported from the stores.
    let residual = runtime.take_pending_renderer_handoff();
    let pending = residual
        .as_ref()
        .map(|handoff| handoff.image_ids().iter().copied().collect::<std::collections::BTreeSet<_>>())
        .unwrap_or_default();
    let in_stores = runtime
        .retained_renderer_image_ids()
        .into_iter()
        .filter(|image| !pending.contains(image))
        .collect::<Vec<_>>();
    match native_scanout.export_renderer_image_handoff(&in_stores) {
        Ok(mut handoff) => {
            if let Some(residual) = residual {
                handoff.absorb(residual);
            }
            Ok(handoff)
        }
        Err(error) => {
            runtime.return_pending_renderer_handoff(residual);
            crate::session_eprintln!(
                "sophia_live_renderer_handoff schema=1 status=failed phase=export_images failure_code={} retained_count={}",
                crate::diagnostics::failure_code(error.as_ref()),
                in_stores.len(),
            );
            Err(error)
        }
    }
}

/// Resumes onto a replacement owner. The handoff is restored into it by
/// device and demand, whatever heads the replacement has; the runtime keeps
/// the snapshots of images no store took, and the handoff is consumed only
/// once the replacement has published.
fn resume_native_scanout_from_scene(
    runtime: &mut LiveProductionVisualRuntime,
    native: &mut LiveProductionNativeScanout,
    outputs: &[sophia_engine::HeadlessOutput],
    scene: &mut LiveProductionCpuScene,
    handoff: &mut Option<sophia_backend_live::LiveProductionRendererImageHandoff>,
) -> Result<usize, Box<dyn std::error::Error>> {
    resume_native_scanout_from_scene_with(runtime, native, outputs, scene, handoff, None)
}

/// The same resume with every logical output at a resolved layout's
/// root-space viewport, installed before restore and the first presentation.
fn resume_native_scanout_from_scene_at(
    runtime: &mut LiveProductionVisualRuntime,
    native: &mut LiveProductionNativeScanout,
    outputs: &[sophia_engine::HeadlessOutput],
    scene: &mut LiveProductionCpuScene,
    handoff: &mut Option<sophia_backend_live::LiveProductionRendererImageHandoff>,
    logical_viewports: &[(sophia_protocol::OutputId, sophia_protocol::Rect)],
) -> Result<usize, Box<dyn std::error::Error>> {
    resume_native_scanout_from_scene_with(
        runtime,
        native,
        outputs,
        scene,
        handoff,
        Some(logical_viewports),
    )
}

fn resume_native_scanout_from_scene_with(
    runtime: &mut LiveProductionVisualRuntime,
    native: &mut LiveProductionNativeScanout,
    outputs: &[sophia_engine::HeadlessOutput],
    scene: &mut LiveProductionCpuScene,
    handoff: &mut Option<sophia_backend_live::LiveProductionRendererImageHandoff>,
    logical_viewports: Option<&[(sophia_protocol::OutputId, sophia_protocol::Rect)]>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let (restore, taken) =
        native_owner_retirement::restore_retained_handoff(
            handoff,
            |handoff| match logical_viewports {
                Some(logical_viewports) => runtime.resume_native_scanout_at(
                    native,
                    outputs,
                    scene,
                    handoff,
                    logical_viewports,
                ),
                None => runtime.resume_native_scanout(native, outputs, scene, handoff),
            },
        )?;
    if let Some(mut taken) = taken {
        taken.retain_only(&restore.pending);
        runtime.keep_pending_renderer_handoff(
            taken,
            native.renderer_storage_progress(),
            restore.busy,
        );
    }
    Ok(restore.restored.len())
}

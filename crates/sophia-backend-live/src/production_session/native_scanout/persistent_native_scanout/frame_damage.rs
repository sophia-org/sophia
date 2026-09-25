use super::*;

/// Projects one logical mirror scene's immutable damage state into a physical
/// head's coordinate space.
///
/// Rendering and damage must use the same target rectangle. Keeping this
/// transformation pure lets every mirror queue path prepare all heads before
/// it reserves or mutates a logical generation.
pub fn project_mirror_output_damage_snapshot(
    snapshot: &sophia_engine::OutputFrameDamageSnapshot,
    source: sophia_protocol::Size,
    destination: sophia_engine::HeadlessOutput,
    fit: sophia_protocol::OutputHeadMapping,
) -> Result<sophia_engine::OutputFrameDamageSnapshot, &'static str> {
    if snapshot.output.id != destination.id
        || snapshot.compositor_display_list.output != destination.id
    {
        return Err("mirror damage snapshot targets a different logical output");
    }
    if snapshot.output.size != source {
        return Err("mirror damage snapshot source size does not match the projected scene");
    }
    if source.width <= 0
        || source.height <= 0
        || destination.size.width <= 0
        || destination.size.height <= 0
        || destination.scale == 0
    {
        return Err("mirror damage projection has an invalid output shape");
    }
    let target = crate::project_mirror_rect(source, destination.size, fit);
    if target.width <= 0 || target.height <= 0 {
        return Err("mirror damage projection is empty");
    }

    let mut projected = snapshot.clone();
    projected.output = destination;
    for surface in &mut projected.surfaces {
        surface.geometry = crate::project_mirror_child_rect(surface.geometry, source, target);
    }
    let child = |rect| crate::project_mirror_child_rect(rect, source, target);
    // WM policy targets round outward with the Engine's one projection,
    // exactly as the head plan draws them (t244).
    let policy_transform = sophia_engine::HeadLogicalTransform {
        source,
        projected_scene: target,
    };
    let policy_child = |rect| policy_transform.project_local_rect_outward(rect);
    for command in &mut projected.compositor_display_list.commands {
        match command {
            // A WM region's stroke damages where the head plan draws it,
            // with the same shared geometry; other borders keep theirs.
            sophia_engine::CompositorDisplayCommand::Border(border)
                if matches!(
                    border.node,
                    sophia_engine::CompositorNodeId::PolicyRegion { .. }
                ) =>
            {
                (border.outer, border.inner) =
                    policy_transform.project_local_policy_border(border.outer, border.inner);
            }
            sophia_engine::CompositorDisplayCommand::Border(border) => {
                border.outer = child(border.outer);
                border.inner = child(border.inner);
            }
            // Region backdrops and other rectangles, surface instances and
            // the presentation stamp's coverage damage the mirror head where
            // it draws them, not where the primary head does (t244).
            sophia_engine::CompositorDisplayCommand::Rect(rect) => {
                rect.geometry = if matches!(
                    rect.node,
                    sophia_engine::CompositorNodeId::PolicyRegion { .. }
                ) {
                    policy_child(rect.geometry)
                } else {
                    child(rect.geometry)
                };
            }
            sophia_engine::CompositorDisplayCommand::SurfaceInstance(instance) => {
                instance.destination = policy_child(instance.destination);
                instance.clip = policy_child(instance.clip);
            }
            sophia_engine::CompositorDisplayCommand::PresentationStamp(stamp) => {
                stamp.coverage = policy_child(stamp.coverage);
            }
            _ => {}
        }
    }
    // What projects to nothing on this head draws and damages nothing there;
    // it cannot stay as a record the damage ledger would refuse. A WM policy
    // target this head crops away entirely is not drawn here either, so it
    // is not listed as drawn (t244).
    let screen = sophia_protocol::Rect {
        x: 0,
        y: 0,
        width: destination.size.width,
        height: destination.size.height,
    };
    let on_screen = |rect: sophia_protocol::Rect| {
        rect.x < screen.width
            && rect.y < screen.height
            && rect.x.saturating_add(rect.width) > 0
            && rect.y.saturating_add(rect.height) > 0
            && !rect.is_empty()
    };
    projected
        .compositor_display_list
        .commands
        .retain(|command| match command {
            sophia_engine::CompositorDisplayCommand::Rect(rect) => {
                !rect.geometry.is_empty()
                    && (!matches!(
                        rect.node,
                        sophia_engine::CompositorNodeId::PolicyRegion { .. }
                    ) || on_screen(rect.geometry))
            }
            sophia_engine::CompositorDisplayCommand::SurfaceInstance(instance) => {
                on_screen(instance.visible())
            }
            sophia_engine::CompositorDisplayCommand::Border(border)
                if matches!(
                    border.node,
                    sophia_engine::CompositorNodeId::PolicyRegion { .. }
                ) =>
            {
                sophia_engine::compositor_border_bands(*border)
                    .iter()
                    .any(|band| on_screen(band.geometry))
            }
            _ => true,
        });
    projected.software_cursor = projected
        .software_cursor
        .map(|cursor| crate::project_mirror_child_rect(cursor, source, target));
    Ok(projected)
}

impl LiveProductionNativeHead {
    pub(super) fn queue_output_damage_snapshot(
        &mut self,
        snapshot: Option<sophia_engine::OutputFrameDamageSnapshot>,
    ) {
        let Some(snapshot) = snapshot else {
            self.output_frames.discard_pending();
            return;
        };
        if let Err(error) = self.output_frames.queue(snapshot) {
            tracing::warn!(
                "sophia_live_output_damage schema=1 status=queue_rejected output={} reason={error}",
                self.output.id.raw(),
            );
            self.output_frames.discard_pending();
        }
    }
}

pub(super) fn trace_presented_output_damage(
    status: &'static str,
    output: OutputId,
    presented: &sophia_engine::OutputFramePresentation,
) {
    tracing::trace!(
        "sophia_live_compositor_damage schema=1 status={} output={} rects={}",
        status,
        output.raw(),
        presented.compositor_damage.rects.len(),
    );
    tracing::trace!(
        "sophia_live_output_damage schema=1 status={} output={} rects={}",
        status,
        output.raw(),
        presented.damage.rects.len(),
    );
    tracing::trace!(
        "sophia_live_output_repaint schema=1 status={} output={} mode={} rects={} pixels={}",
        status,
        output.raw(),
        presented.repaint.reduced_name(),
        presented
            .repaint
            .damage()
            .map_or(0, |damage| damage.rects.len()),
        presented.repaint.damaged_pixels(),
    );
}

pub(super) fn trace_presented_mirror_head_damage(
    output: OutputId,
    head: sophia_engine::RenderHeadId,
    frame: LiveProductionNativeFrameId,
    presented: &sophia_engine::OutputFramePresentation,
) {
    tracing::trace!(
        "sophia_live_mirror_head_damage schema=2 status=presented output={} head={} frame={} width={} height={} mode={} rects={} pixels={}",
        output.raw(),
        head.raw(),
        frame.raw(),
        presented.snapshot.output.size.width,
        presented.snapshot.output.size.height,
        presented.repaint.reduced_name(),
        presented
            .repaint
            .damage()
            .map_or(0, |damage| damage.rects.len()),
        presented.repaint.damaged_pixels(),
    );
}
/// Pixel proof needs a retirement identity at the same log level as readback.
/// Ordinary sessions keep this per-flip record at trace level.
pub(super) fn trace_native_head_retirement(output: u64, head: u64, submission: usize, frame: u64) {
    if std::env::var_os("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE").is_some() {
        tracing::info!(
            "sophia_live_native_head_page_flip schema=2 status=retired output={output} head={head} submission={submission} frame={frame}"
        );
    } else {
        tracing::trace!(
            "sophia_live_native_head_page_flip schema=2 status=retired output={output} head={head} submission={submission} frame={frame}"
        );
    }
}

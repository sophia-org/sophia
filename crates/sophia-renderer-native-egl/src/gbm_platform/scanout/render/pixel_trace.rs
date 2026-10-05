fn trace_composition_pixels(
    pipeline: &PersistentXrgb8888GlPipeline,
    stage: &str,
    layer: usize,
    target: NativeCompositionRect,
    format: u32,
    modifier: u64,
    stride: u32,
) {
    let region_metrics = pipeline.read_composition_region_pixels(target.into());
    match (pipeline.read_composition_pixels(), region_metrics) {
        (Ok(metrics), Ok(region)) => tracing::info!(
            "sophia_native_composition_pixels schema=3 status=read stage={stage} layer={layer} target={}x{}_{}_{} format={format:#x} modifier={modifier:#x} stride={stride} pixels={} nonzero_rgb_pixels={} alpha_zero_pixels={} alpha_partial_pixels={} alpha_opaque_pixels={} luminance_sum={} luminance_mean_millis={} checksum={} region_pixels={} region_nonzero_rgb_pixels={} region_red_pixels={} region_green_pixels={} region_blue_pixels={} region_yellow_pixels={} region_cyan_pixels={} region_magenta_pixels={} region_gray_pixels={} region_other_pixels={} region_luminance_sum={} region_luminance_mean_millis={} region_luminance_histogram={} region_checksum={}",
            target.width,
            target.height,
            target.x,
            target.y,
            metrics.pixels,
            metrics.nonzero_rgb_pixels,
            metrics.alpha_zero_pixels,
            metrics.alpha_partial_pixels,
            metrics.alpha_opaque_pixels,
            metrics.luminance_sum,
            metrics.luminance_mean_millis(),
            metrics.checksum,
            region.pixels,
            region.nonzero_rgb_pixels,
            region.red_pixels,
            region.green_pixels,
            region.blue_pixels,
            region.yellow_pixels,
            region.cyan_pixels,
            region.magenta_pixels,
            region.gray_pixels,
            region.other_pixels,
            region.luminance_sum,
            region.luminance_mean_millis(),
            region.luminance_histogram_field(),
            region.checksum,
        ),
        _ => tracing::warn!(
            "sophia_native_composition_pixels schema=3 status=unavailable stage={stage} layer={layer} target={}x{}_{}_{} format={format:#x} modifier={modifier:#x} stride={stride}",
            target.width,
            target.height,
            target.x,
            target.y,
        ),
    }
}

fn trace_final_composition_region(
    pipeline: &PersistentXrgb8888GlPipeline,
    source_stage: &str,
    layer: usize,
    target: NativeCompositionRect,
    output: (u32, u32),
    trace: Option<NativeCompositionTrace>,
) -> Option<usize> {
    match pipeline.read_composition_region_pixels(target.into()) {
        Ok(region) => {
            // Keep the existing region schema for historical gates. This
            // additive identity record ties the same readback to an opaque
            // head scene, which the backend maps to an exact retired frame.
            if let Some(trace) = trace {
                tracing::info!(
                    "sophia_native_composition_region_frame schema=1 status=read output={} head={} scene_generation={} layer={layer} source_stage={source_stage} target={}x{}_{}_{} region_pixels={} nonzero_rgb_pixels={} checksum={}",
                    trace.output,
                    trace.head,
                    trace.scene_generation,
                    target.width,
                    target.height,
                    target.x,
                    target.y,
                    region.pixels,
                    region.nonzero_rgb_pixels,
                    region.checksum,
                );
            }
            tracing::info!(
                "sophia_native_composition_region schema=3 status=read composition=final source_stage={source_stage} layer={layer} output={}x{} target={}x{}_{}_{} region_pixels={} region_nonzero_rgb_pixels={} region_red_pixels={} region_green_pixels={} region_blue_pixels={} region_yellow_pixels={} region_cyan_pixels={} region_magenta_pixels={} region_gray_pixels={} region_other_pixels={} region_luminance_sum={} region_luminance_mean_millis={} region_luminance_histogram={} region_checksum={}",
                output.0,
                output.1,
                target.width,
                target.height,
                target.x,
                target.y,
                region.pixels,
                region.nonzero_rgb_pixels,
                region.red_pixels,
                region.green_pixels,
                region.blue_pixels,
                region.yellow_pixels,
                region.cyan_pixels,
                region.magenta_pixels,
                region.gray_pixels,
                region.other_pixels,
                region.luminance_sum,
                region.luminance_mean_millis(),
                region.luminance_histogram_field(),
                region.checksum,
            );
            Some(region.nonzero_rgb_pixels)
        }
        Err(_) => {
            tracing::warn!(
                "sophia_native_composition_region schema=3 status=unavailable composition=final source_stage={source_stage} layer={layer} output={}x{} target={}x{}_{}_{}",
                output.0,
                output.1,
                target.width,
                target.height,
                target.x,
                target.y,
            );
            None
        }
    }
}

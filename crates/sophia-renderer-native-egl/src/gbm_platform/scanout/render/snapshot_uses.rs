struct NativeSnapshotCompositionResources<'a> {
    images: &'a std::collections::BTreeMap<NativeRendererImageId, NativeRendererImage>,
    fences: &'a mut Vec<NativeSnapshotFence>,
    quarantine: &'a mut Vec<std::rc::Rc<NativeCaptureAllocation>>,
    import_reuse: bool,
}

fn collect_snapshot_uses(
    frame: NativeCompositionFrame<'_>,
    renderer_images: &std::collections::BTreeMap<NativeRendererImageId, NativeRendererImage>,
) -> Vec<NativeSnapshotGpuUse<NativeCaptureAllocation>> {
    let mut uses = Vec::new();
    for layer in frame.layers {
        if let NativeCompositionLayer::RendererImage(layer) = layer
            && let Some(image) = renderer_images.get(&layer.image_id)
            && let Some(captured) = &image.pooled
            && !uses
                .iter()
                .any(|gpu_use: &NativeSnapshotGpuUse<NativeCaptureAllocation>| {
                    std::rc::Rc::ptr_eq(gpu_use.generation(), &captured.generation)
                })
        {
            uses.push(NativeSnapshotGpuUse {
                allocation: captured.allocation.clone(),
                use_guard: captured.generation.acquire_use(),
            });
        }
    }
    uses
}

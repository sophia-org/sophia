// Completion belongs to the consuming GL command stream, including a draw
// whose output is subsequently rejected by KMS. Idle imports are not readers.
struct NativeSnapshotFence {
    sync: khronos_egl::Sync,
    uses: Vec<NativeSnapshotGpuUse<NativeCaptureAllocation>>,
}

fn finish_snapshot_batch(
    egl: &khronos_egl::DynamicInstance<khronos_egl::EGL1_5>,
    display: khronos_egl::Display,
    uses: Vec<NativeSnapshotGpuUse<NativeCaptureAllocation>>,
    fences: &mut Vec<NativeSnapshotFence>,
    quarantine: &mut Vec<std::rc::Rc<NativeCaptureAllocation>>,
) {
    if uses.is_empty() {
        return;
    }
    if fences.len() >= DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY {
        quarantine_snapshot_uses(uses, quarantine);
        return;
    }
    let completion = unsafe {
        egl.create_sync(
            display,
            khronos_egl::SYNC_FENCE as u32,
            &[khronos_egl::ATTRIB_NONE],
        )
    };
    if let Ok(sync) = completion {
        let submitted =
            unsafe { egl.client_wait_sync(display, sync, khronos_egl::SYNC_FLUSH_COMMANDS_BIT, 0) };
        if submitted.is_ok() {
            fences.push(NativeSnapshotFence { sync, uses });
            return;
        }
        let _ = unsafe { egl.destroy_sync(display, sync) };
    }
    // The frame may remain presentable, but its storage cannot be reused or
    // removed from the allocation budget while completion is uncertain.
    quarantine_snapshot_uses(uses, quarantine);
}

impl<T: AsFd> NativeGbmRenderedScanoutContext<T> {
    fn poll_snapshot_fences(&mut self) {
        let mut index = 0;
        while index < self.snapshot_fences.len() {
            let sync = self.snapshot_fences[index].sync;
            match unsafe { self.egl.client_wait_sync(self.display, sync, 0, 0) } {
                Ok(khronos_egl::TIMEOUT_EXPIRED) => index += 1,
                result => {
                    let fence = self.snapshot_fences.swap_remove(index);
                    if result != Ok(khronos_egl::CONDITION_SATISFIED) {
                        quarantine_snapshot_uses(fence.uses, &mut self.snapshot_quarantine);
                    }
                    let _ = unsafe { self.egl.destroy_sync(self.display, fence.sync) };
                    // Dropping the guards releases exactly their generations.
                }
            }
        }
    }

    fn abandon_snapshot_fences(&mut self) {
        for fence in self.snapshot_fences.drain(..) {
            quarantine_snapshot_uses(fence.uses, &mut self.snapshot_quarantine);
            let _ = unsafe { self.egl.destroy_sync(self.display, fence.sync) };
        }
    }
}

#[derive(Default)]
struct NativeCaptureTimingTotals {
    cpu: [std::cell::Cell<std::time::Duration>; 5],
    wall: [std::cell::Cell<std::time::Duration>; 5],
}

#[derive(Clone, Copy)]
enum NativeCaptureTimingKind {
    Capture = 0,
    Setup = 1,
    Copy = 2,
    Cleanup = 3,
    Reclaim = 4,
}

struct NativeCaptureTimingSpan {
    accounting: std::rc::Rc<NativeCaptureAllocationAccounting>,
    kind: NativeCaptureTimingKind,
    started: RenderStageTimer,
    cleanup_cpu: std::time::Duration,
    cleanup_wall: std::time::Duration,
}

impl NativeCaptureTimingSpan {
    fn start(
        accounting: &std::rc::Rc<NativeCaptureAllocationAccounting>,
        kind: NativeCaptureTimingKind,
    ) -> Option<Self> {
        accounting.timing_enabled.get().then(|| Self {
            accounting: accounting.clone(),
            kind,
            started: RenderStageTimer::start(),
            cleanup_cpu: accounting.timing.cpu[NativeCaptureTimingKind::Cleanup as usize].get(),
            cleanup_wall: accounting.timing.wall[NativeCaptureTimingKind::Cleanup as usize].get(),
        })
    }
}

impl Drop for NativeCaptureTimingSpan {
    fn drop(&mut self) {
        let totals = &self.accounting.timing;
        let mut cpu = self.started.cpu_elapsed();
        let mut wall = self.started.elapsed();
        if matches!(self.kind, NativeCaptureTimingKind::Copy) {
            // Cleanup is nested in draw, so remove its measured interval from
            // copy. Setup + copy + cleanup form the capture partition. Reclaim
            // may run inside setup under pressure; it is a separate attribution
            // bracket and must never be added to that partition.
            cpu = cpu.saturating_sub(totals.cpu[3].get().saturating_sub(self.cleanup_cpu));
            wall = wall.saturating_sub(totals.wall[3].get().saturating_sub(self.cleanup_wall));
        }
        let index = self.kind as usize;
        totals.cpu[index].set(totals.cpu[index].get().saturating_add(cpu));
        totals.wall[index].set(totals.wall[index].get().saturating_add(wall));
    }
}

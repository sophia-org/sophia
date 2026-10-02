use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct CpuSceneTiming {
    pub enabled: bool,
    pub elapsed: Duration,
    pub cpu: Duration,
}
pub(super) struct CpuSceneTimer {
    wall: Instant,
    cpu: Duration,
}
impl CpuSceneTiming {
    pub fn start(&self) -> Option<CpuSceneTimer> {
        self.enabled.then(|| CpuSceneTimer {
            wall: Instant::now(),
            cpu: thread_cpu(),
        })
    }
    pub fn finish(&mut self, timer: Option<CpuSceneTimer>) {
        if let Some(timer) = timer {
            self.elapsed = self.elapsed.saturating_add(timer.wall.elapsed());
            self.cpu = self
                .cpu
                .saturating_add(thread_cpu().saturating_sub(timer.cpu));
        }
    }
}
fn thread_cpu() -> Duration {
    let value = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    Duration::new(value.tv_sec.max(0) as u64, value.tv_nsec.max(0) as u32)
}

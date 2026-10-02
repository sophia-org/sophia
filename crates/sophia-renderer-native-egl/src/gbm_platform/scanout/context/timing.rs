// Optional qualification clocks. Wall time includes driver/GPU waits; thread
// CPU time counts only this worker. Neither is sampled for normal rendering.
struct RenderStageTimer { wall: Instant, cpu: std::time::Duration }
impl RenderStageTimer {
    fn start() -> Self { Self { wall: Instant::now(), cpu: render_thread_cpu() } }
    fn elapsed(&self) -> std::time::Duration { self.wall.elapsed() }
    fn cpu_elapsed(&self) -> std::time::Duration { render_thread_cpu().saturating_sub(self.cpu) }
}
fn render_thread_cpu() -> std::time::Duration {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    std::time::Duration::new(time.tv_sec.max(0) as u64, time.tv_nsec.max(0) as u32)
}

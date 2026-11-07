//! Diagnostic-only owner CPU accounting; records are emitted after the loop exits.
#[derive(Clone, Copy)]
pub(super) enum Phase {
    Other = 0,
    Components = 1,
    Native = 2,
    Authority = 3,
}

#[cfg(not(feature = "diagnostic-owner-cost"))]
pub(super) struct Recorder;
#[cfg(not(feature = "diagnostic-owner-cost"))]
impl Recorder {
    pub(super) fn new(_: u64) -> Self {
        Self
    }
    pub(super) fn begin_pass(&mut self, _: [u64; 2], _: u8, _: [u64; 2]) {}
    pub(super) fn phase(&mut self, _: Phase) {}
    pub(super) fn measure<T>(&mut self, _: Phase, f: impl FnOnce() -> T) -> T {
        f()
    }
    pub(super) fn finish(self, _: bool, _: u64) {}
}

#[cfg(feature = "diagnostic-owner-cost")]
pub(super) use enabled::Recorder;
#[cfg(feature = "diagnostic-owner-cost")]
mod enabled {
    use super::Phase;
    const LIMIT: usize = 32768;
    #[derive(Clone, Copy)]
    struct Stamp {
        wall: u64,
        after: u64,
        cpu: u64,
    }
    fn nanos(clock: rustix::time::ClockId) -> u64 {
        let t = rustix::time::clock_gettime(clock);
        u64::try_from(t.tv_sec).expect("nonnegative clock") * 1_000_000_000
            + u64::try_from(t.tv_nsec).expect("nonnegative nanoseconds")
    }
    fn stamp() -> Stamp {
        Stamp {
            wall: nanos(rustix::time::ClockId::Monotonic),
            cpu: nanos(rustix::time::ClockId::ThreadCPUTime),
            after: nanos(rustix::time::ClockId::Monotonic),
        }
    }
    struct Row {
        start: Stamp,
        end: Stamp,
        cpu: [u64; 4],
        /// Cumulative render-worker and image-worker thread CPU at pass start.
        workers: [u64; 2],
        /// Cause bits of the wait that woke this pass.
        cause: u8,
        /// Cumulative component wire turns and ready turns at pass start.
        turns: [u64; 2],
    }
    pub(crate) struct Recorder {
        rows: Vec<Row>,
        start: Option<Stamp>,
        last_cpu: u64,
        phase: Phase,
        cpu: [u64; 4],
        workers: [u64; 2],
        cause: u8,
        turns: [u64; 2],
        overflow: u64,
        initial_passes: u64,
    }
    impl Recorder {
        pub(crate) fn new(initial_passes: u64) -> Self {
            Self {
                rows: Vec::with_capacity(LIMIT),
                start: None,
                last_cpu: 0,
                phase: Phase::Other,
                cpu: [0; 4],
                workers: [0; 2],
                cause: 0,
                turns: [0; 2],
                overflow: 0,
                initial_passes,
            }
        }
        fn charge(&mut self, cpu: u64) {
            if self.start.is_some() {
                self.cpu[self.phase as usize] += cpu
                    .checked_sub(self.last_cpu)
                    .expect("thread CPU clock regressed");
            }
            self.last_cpu = cpu;
        }
        fn close(&mut self, end: Stamp) {
            self.charge(end.cpu);
            if let Some(start) = self.start.take() {
                if self.rows.len() == LIMIT {
                    self.overflow += 1;
                } else {
                    self.rows.push(Row {
                        start,
                        end,
                        cpu: self.cpu,
                        workers: self.workers,
                        cause: self.cause,
                        turns: self.turns,
                    });
                }
            }
        }
        fn begin_at(&mut self, now: Stamp) {
            self.close(now);
            self.start = Some(now);
            self.phase = Phase::Other;
            self.cpu = [0; 4];
        }
        /// `workers`: cumulative render-worker and image-worker thread CPU
        /// read just before this pass, so a window's delta is theirs.
        /// `cause`: bits of the wait that woke this pass. `turns`: cumulative
        /// component wire turns and ready turns before this pass.
        pub(crate) fn begin_pass(&mut self, workers: [u64; 2], cause: u8, turns: [u64; 2]) {
            self.begin_at(stamp());
            self.workers = workers;
            self.cause = cause;
            self.turns = turns;
        }
        fn phase_at(&mut self, phase: Phase, cpu: u64) {
            self.charge(cpu);
            self.phase = phase;
        }
        pub(crate) fn phase(&mut self, phase: Phase) {
            self.phase_at(phase, nanos(rustix::time::ClockId::ThreadCPUTime));
        }
        pub(crate) fn measure<T>(&mut self, phase: Phase, f: impl FnOnce() -> T) -> T {
            let previous = self.phase;
            self.phase(phase);
            let result = f();
            self.phase(previous);
            result
        }
        pub(crate) fn finish(mut self, complete: bool, final_passes: u64) {
            self.close(stamp());
            for (i, row) in self.rows.iter().enumerate() {
                crate::session_println!(
                    "sophia_owner_cost schema=3 pass={} start_ns={} start_after_ns={} end_before_ns={} end_ns={} cpu_start_ns={} cpu_end_ns={} other_ns={} components_ns={} native_ns={} authority_ns={} render_cpu_ns={} image_cpu_ns={} cause={} component_turns={} component_ready_turns={}",
                    i + 1,
                    row.start.wall,
                    row.start.after,
                    row.end.wall,
                    row.end.after,
                    row.start.cpu,
                    row.end.cpu,
                    row.cpu[0],
                    row.cpu[1],
                    row.cpu[2],
                    row.cpu[3],
                    row.workers[0],
                    row.workers[1],
                    row.cause,
                    row.turns[0],
                    row.turns[1]
                );
            }
            crate::session_println!(
                "sophia_owner_cost_end schema=3 rows={} overflow={} complete={} pid={} tid={} initial_passes={} final_passes={}",
                self.rows.len(),
                self.overflow,
                complete,
                std::process::id(),
                rustix::thread::gettid().as_raw_nonzero(),
                self.initial_passes,
                final_passes
            );
        }
    }
    #[cfg(test)]
    mod tests {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/owner_cost.rs"
        ));
    }
}

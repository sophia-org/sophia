use super::{Snapshot, Stage};

impl Snapshot {
    /// One `kind=summary` line followed by one `kind=stage` row per stage.
    /// Values are numeric or fixed words; rows repeat the thread/generation
    /// keys so a reader can join them without state.
    pub fn records(&self) -> Vec<String> {
        let key = format!(
            "sophia_frame_cost schema=1 role={} tid={} thread_sequence={} generation={}",
            self.role.name(),
            self.tid,
            self.thread_sequence,
            self.generation
        );
        let mut lines = Vec::with_capacity(Stage::COUNT + 1);
        lines.push(format!(
            "{key} kind=summary reason={} cpu_first_nsec={} cpu_now_nsec={} mono_first_nsec={} \
             mono_now_nsec={} enabled_calls={} disabled_calls={} clock_regressions={} \
             saturations={} invalid={}",
            self.reason.name(),
            self.cpu_first_nsec,
            self.cpu_now_nsec,
            self.mono_first_nsec,
            self.mono_now_nsec,
            self.enabled_calls,
            self.disabled_calls,
            self.clock_regressions,
            self.saturations,
            u8::from(self.invalid),
        ));
        for stage in Stage::ALL {
            let cost = self.stages[stage as usize];
            lines.push(format!(
                "{key} kind=stage stage={} calls={} aborted={} cpu_nsec={} elapsed_nsec={} \
                 max_cpu_nsec={} max_elapsed_nsec={}",
                stage.name(),
                cost.calls,
                cost.aborted,
                cost.cpu_nsec,
                cost.elapsed_nsec,
                cost.max_cpu_nsec,
                cost.max_elapsed_nsec,
            ));
        }
        lines
    }
}

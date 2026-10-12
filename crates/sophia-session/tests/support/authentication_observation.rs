//! Process-wide observations for a serial fixture, not per-role latency.
use std::time::Instant;

pub(super) struct Observation {
    at: Instant,
    cpu_ticks: u64,
    rss_kib: u64,
    worker: sophia_runtime::lock_files::LockFileServiceStats,
}

fn process() -> (u64, u64) {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap();
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .collect();
    // Fields 14/15 follow the process name and state (field 3).
    let cpu = fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap();
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let rss = status
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:").map(|value| {
                value
                    .split_whitespace()
                    .next()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
            })
        })
        .unwrap();
    (cpu, rss)
}

impl Observation {
    pub(super) fn start(service: &sophia_runtime::lock_files::LockFileService) -> Self {
        let (cpu_ticks, rss_kib) = process();
        Self {
            at: Instant::now(),
            cpu_ticks,
            rss_kib,
            worker: service.stats(),
        }
    }

    pub(super) fn report(self, phase: &str, service: &sophia_runtime::lock_files::LockFileService) {
        let (cpu, rss) = process();
        let worker = service.stats();
        println!(
            "t317_auth_observation {}",
            serde_json::json!({
                "phase": phase,
                "scope": "test_process_including_driver_and_factotum_threads",
                "elapsed_usec": self.at.elapsed().as_micros(),
                "cpu_ticks": cpu - self.cpu_ticks,
                "ticks_per_second": rustix::param::clock_ticks_per_second(),
                "rss_before_kib": self.rss_kib,
                "rss_after_kib": rss,
                "worker_passes": worker.passes - self.worker.passes,
                "worker_waits": worker.waits - self.worker.waits,
                "worker_handoff_waits": worker.handoff_waits - self.worker.handoff_waits
            })
        );
    }
}

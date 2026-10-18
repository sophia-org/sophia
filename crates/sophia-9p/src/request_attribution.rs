//! Isolated diagnostics, never acceptance instrumentation. CPU is thread-local;
//! allocation intervals include all threads of this process. Nested intervals
//! overlap and MUST NOT be summed. The device-free fixture uses a separate peer
//! process; asynchronous worker allocations remain explicitly non-exclusive.
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
pub enum Site {
    Version,
    Auth,
    Attach,
    Walk,
    Open,
    Read,
    Write,
    Clunk,
    Flush,
    Other,
    EncodeReply,
    RetryReads,
    AdmitOutbound,
}

pub const NAMES: [&str; 13] = [
    "version",
    "auth",
    "attach",
    "walk",
    "open",
    "read",
    "write",
    "clunk",
    "flush",
    "other",
    "encode_reply",
    "retry_reads",
    "admit_outbound",
];

struct Totals {
    calls: AtomicU64,
    cpu: AtomicU64,
    allocations: AtomicU64,
    reallocations: AtomicU64,
    bytes: AtomicU64,
}
static TOTALS: [Totals; 13] = [const {
    Totals {
        calls: AtomicU64::new(0),
        cpu: AtomicU64::new(0),
        allocations: AtomicU64::new(0),
        reallocations: AtomicU64::new(0),
        bytes: AtomicU64::new(0),
    }
}; 13];

#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub calls: u64,
    pub thread_cpu_ns: u64,
    pub process_allocations: u64,
    pub process_reallocations: u64,
    pub process_bytes: u64,
}

fn cpu() -> u64 {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

pub struct Guard {
    site: Site,
    cpu: u64,
    allocations: stats_alloc::Stats,
    // A thread CPU interval cannot finish on a different thread.
    thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

pub fn enter(site: Site) -> Guard {
    Guard {
        site,
        cpu: cpu(),
        allocations: stats_alloc::INSTRUMENTED_SYSTEM.stats(),
        thread: std::marker::PhantomData,
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        let end_cpu = cpu();
        let end = stats_alloc::INSTRUMENTED_SYSTEM.stats();
        let total = &TOTALS[self.site as usize];
        total
            .cpu
            .fetch_add(end_cpu.saturating_sub(self.cpu), Ordering::Relaxed);
        total.allocations.fetch_add(
            end.allocations.saturating_sub(self.allocations.allocations) as u64,
            Ordering::Relaxed,
        );
        total.reallocations.fetch_add(
            end.reallocations
                .saturating_sub(self.allocations.reallocations) as u64,
            Ordering::Relaxed,
        );
        total.bytes.fetch_add(
            end.bytes_allocated
                .saturating_sub(self.allocations.bytes_allocated) as u64,
            Ordering::Relaxed,
        );
        total.calls.fetch_add(1, Ordering::Release);
    }
}

/// Each field is sampled separately. Record monotonic bounds around this call;
/// a concurrent completion may straddle those bounds.
pub fn snapshot() -> [Snapshot; 13] {
    std::array::from_fn(|index| {
        let total = &TOTALS[index];
        Snapshot {
            calls: total.calls.load(Ordering::Acquire),
            thread_cpu_ns: total.cpu.load(Ordering::Relaxed),
            process_allocations: total.allocations.load(Ordering::Relaxed),
            process_reallocations: total.reallocations.load(Ordering::Relaxed),
            process_bytes: total.bytes.load(Ordering::Relaxed),
        }
    })
}

pub(crate) fn request(request: &crate::records::Request<'_>) -> Guard {
    use crate::records::Request;
    enter(match request {
        Request::Version { .. } => Site::Version,
        Request::Auth { .. } => Site::Auth,
        Request::Attach { .. } => Site::Attach,
        Request::Walk { .. } => Site::Walk,
        Request::Lopen { .. } => Site::Open,
        Request::Read { .. } => Site::Read,
        Request::Write { .. } => Site::Write,
        Request::Clunk { .. } => Site::Clunk,
        Request::Flush { .. } => Site::Flush,
        _ => Site::Other,
    })
}

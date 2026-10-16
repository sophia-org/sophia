//! Isolated comparison instrumentation, never a production allocator policy.
//! Snapshots have clock bounds because counters are read separately while other
//! threads may allocate. Foreign allocators and other processes are excluded.
use std::{io::Write, sync::mpsc, thread::JoinHandle, time::Duration};

pub type Allocator = &'static stats_alloc::StatsAlloc<std::alloc::System>;
pub const ALLOCATOR: Allocator = &stats_alloc::INSTRUMENTED_SYSTEM;

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub before_usec: u64,
    pub after_usec: u64,
    pub cpu_ns: u64,
    pub allocations: usize,
    pub reallocations: usize,
    pub deallocations: usize,
    pub bytes_allocated: usize,
}

fn nanos(clock: rustix::time::ClockId) -> u64 {
    let value = rustix::time::clock_gettime(clock);
    value.tv_sec as u64 * 1_000_000_000 + value.tv_nsec as u64
}

pub fn snapshot() -> Snapshot {
    let before_usec = nanos(rustix::time::ClockId::Monotonic) / 1000;
    let counts = ALLOCATOR.stats();
    let cpu_ns = nanos(rustix::time::ClockId::ProcessCPUTime);
    let after_usec = nanos(rustix::time::ClockId::Monotonic) / 1000;
    Snapshot {
        before_usec,
        after_usec,
        cpu_ns,
        allocations: counts.allocations,
        reallocations: counts.reallocations,
        deallocations: counts.deallocations,
        bytes_allocated: counts.bytes_allocated,
    }
}

pub fn record(process: &str, stage: &str, sequence: u64) -> std::io::Result<()> {
    let s = snapshot();
    write_snapshot(&mut std::io::stderr().lock(), process, stage, sequence, s)
}

/// Format before writing so a blocking pipe receives one bounded record,
/// rather than fragments that another process can interrupt.
pub fn write_snapshot(
    output: &mut impl Write,
    process: &str,
    stage: &str,
    sequence: u64,
    s: Snapshot,
) -> std::io::Result<()> {
    let mut bytes = [0u8; 1024];
    let mut buffer = std::io::Cursor::new(bytes.as_mut_slice());
    writeln!(
        buffer,
        "sophia_comparator_counters schema=1 process={process} pid={} stage={stage} sequence={sequence} before_usec={} after_usec={} cpu_ns={} allocations={} reallocations={} deallocations={} bytes_allocated={}",
        std::process::id(),
        s.before_usec,
        s.after_usec,
        s.cpu_ns,
        s.allocations,
        s.reallocations,
        s.deallocations,
        s.bytes_allocated
    )?;
    let len = buffer.position() as usize;
    output.write_all(&bytes[..len])
}

/// Samples the whole Session process. The verifier must bracket each client
/// window conservatively; the requested interval is not a timing guarantee.
pub struct Sampler {
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<std::io::Result<()>>>,
}

impl Sampler {
    pub fn start() -> std::io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("comparison-counters".into())
            .spawn(move || {
                record("session", "start", 0)?;
                for sequence in 1..=60_000 {
                    match receiver.recv_timeout(Duration::from_millis(2)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            return record("session", "end", sequence);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            record("session", "sample", sequence)?
                        }
                    }
                }
                record("session", "exhausted", 60_001)?;
                Err(std::io::Error::other(
                    "comparison counter sample bound exhausted",
                ))
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        let _ = self.stop.send(());
        self.thread
            .take()
            .unwrap()
            .join()
            .map_err(|_| std::io::Error::other("counter sampler panicked"))?
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.stop.send(());
            let _ = thread.join();
        }
    }
}

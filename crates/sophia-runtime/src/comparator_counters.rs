//! Isolated comparison instrumentation, never a production allocator policy.
//! Snapshots have clock bounds because counters are read separately while other
//! threads may allocate. Foreign allocators and other processes are excluded.
use std::{io::Write, sync::mpsc, thread::JoinHandle, time::Duration};

#[cfg(feature = "comparator-profiling")]
pub mod phases;

#[cfg(all(
    feature = "comparator-cpu-counting",
    feature = "comparator-allocation-counting"
))]
compile_error!("CPU-only comparison must use the ordinary allocator");

#[cfg(feature = "comparator-allocation-counting")]
pub type Allocator = &'static stats_alloc::StatsAlloc<std::alloc::System>;
#[cfg(feature = "comparator-allocation-counting")]
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
    #[cfg(feature = "comparator-allocation-counting")]
    let counts = {
        let s = ALLOCATOR.stats();
        (
            s.allocations,
            s.reallocations,
            s.deallocations,
            s.bytes_allocated,
        )
    };
    #[cfg(not(feature = "comparator-allocation-counting"))]
    let counts = (0, 0, 0, 0);
    let cpu_ns = nanos(rustix::time::ClockId::ProcessCPUTime);
    let after_usec = nanos(rustix::time::ClockId::Monotonic) / 1000;
    Snapshot {
        before_usec,
        after_usec,
        cpu_ns,
        allocations: counts.0,
        reallocations: counts.1,
        deallocations: counts.2,
        bytes_allocated: counts.3,
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
    let schema = if cfg!(feature = "comparator-profiling") {
        "2 mode=profile"
    } else if cfg!(feature = "comparator-allocation-counting") {
        "1"
    } else {
        "2 mode=cpu-only"
    };
    writeln!(
        buffer,
        "sophia_comparator_counters schema={schema} process={process} pid={} stage={stage} sequence={sequence} before_usec={} after_usec={} cpu_ns={} allocations={} reallocations={} deallocations={} bytes_allocated={}",
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
        Self::start_with_output(std::io::stderr())
    }

    /// Keep the exact sampler available to isolated overhead checks. The sink
    /// changes where records go, never the counters or sampling cadence.
    pub fn start_with_output(mut output: impl Write + Send + 'static) -> std::io::Result<Self> {
        let (stop, receiver) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("comparison-counters".into())
            .spawn(move || {
                write_snapshot(&mut output, "session", "start", 0, snapshot())?;
                #[cfg(feature = "comparator-profiling")]
                phases::record(&mut output, 0)?;
                for sequence in 1..=60_000 {
                    match receiver.recv_timeout(Duration::from_millis(2)) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            #[cfg(feature = "comparator-profiling")]
                            phases::record(&mut output, sequence)?;
                            return write_snapshot(
                                &mut output,
                                "session",
                                "end",
                                sequence,
                                snapshot(),
                            );
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            write_snapshot(&mut output, "session", "sample", sequence, snapshot())?;
                            #[cfg(feature = "comparator-profiling")]
                            // Phase totals need less frequent publication than
                            // frame-cost counters. Keep detailed profiles within
                            // the same bounded capture without changing cost ticks.
                            if sequence % 8 == 0 {
                                phases::record(&mut output, sequence)?;
                            }
                        }
                    }
                }
                write_snapshot(&mut output, "session", "exhausted", 60_001, snapshot())?;
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

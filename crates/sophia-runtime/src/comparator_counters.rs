//! Isolated comparison instrumentation, never a production allocator policy.
//! Snapshots have clock bounds because counters are read separately while other
//! threads may allocate. Foreign allocators and other processes are excluded.
use std::{io::Write, sync::mpsc, thread::JoinHandle, time::Duration};

#[cfg(feature = "comparator-profiling")]
pub mod phases;

#[cfg(feature = "comparator-progress")]
pub use sophia_9p::progress;

#[cfg(all(
    feature = "comparator-progress",
    any(
        feature = "comparator-cpu-counting",
        feature = "comparator-allocation-counting"
    )
))]
compile_error!("progress diagnostics cannot be mixed with cost or phase instrumentation");

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
    #[cfg(not(feature = "comparator-progress"))]
    pub minor_faults: u64,
    #[cfg(not(feature = "comparator-progress"))]
    pub major_faults: u64,
}

/// Cumulative page faults of the whole process, exited threads included.
/// Page allocation and zeroing done by the kernel inside a write or socket
/// copy is not a fault and does not appear here.
#[cfg(not(feature = "comparator-progress"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Faults {
    pub minor: u64,
    pub major: u64,
}

fn nanos(clock: rustix::time::ClockId) -> u64 {
    let value = rustix::time::clock_gettime(clock);
    value.tv_sec as u64 * 1_000_000_000 + value.tv_nsec as u64
}

#[cfg(not(feature = "comparator-progress"))]
pub fn process_faults() -> std::io::Result<Faults> {
    use nix::sys::resource::{UsageWho, getrusage};
    let usage = getrusage(UsageWho::RUSAGE_SELF).map_err(std::io::Error::from)?;
    let count = |value: std::ffi::c_long| {
        u64::try_from(value)
            .map_err(|_| std::io::Error::other("getrusage returned a negative fault count"))
    };
    Ok(Faults {
        minor: count(usage.minor_page_faults())?,
        major: count(usage.major_page_faults())?,
    })
}

/// Progress records keep their original fields and make no fault read.
#[cfg(feature = "comparator-progress")]
pub fn snapshot() -> std::io::Result<Snapshot> {
    let before_usec = nanos(rustix::time::ClockId::Monotonic) / 1000;
    let cpu_ns = nanos(rustix::time::ClockId::ProcessCPUTime);
    let after_usec = nanos(rustix::time::ClockId::Monotonic) / 1000;
    Ok(Snapshot {
        before_usec,
        after_usec,
        cpu_ns,
        allocations: 0,
        reallocations: 0,
        deallocations: 0,
        bytes_allocated: 0,
    })
}

#[cfg(not(feature = "comparator-progress"))]
pub fn snapshot() -> std::io::Result<Snapshot> {
    snapshot_with_faults(process_faults)
}

/// A failed fault read fails the snapshot; no count is substituted.
#[cfg(not(feature = "comparator-progress"))]
pub fn snapshot_with_faults(
    read_faults: impl FnOnce() -> std::io::Result<Faults>,
) -> std::io::Result<Snapshot> {
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
    let faults = read_faults()?;
    let cpu_ns = nanos(rustix::time::ClockId::ProcessCPUTime);
    let after_usec = nanos(rustix::time::ClockId::Monotonic) / 1000;
    Ok(Snapshot {
        before_usec,
        after_usec,
        cpu_ns,
        allocations: counts.0,
        reallocations: counts.1,
        deallocations: counts.2,
        bytes_allocated: counts.3,
        minor_faults: faults.minor,
        major_faults: faults.major,
    })
}

pub fn record(process: &str, stage: &str, sequence: u64) -> std::io::Result<()> {
    let s = snapshot()?;
    write_snapshot(&mut std::io::stderr().lock(), process, stage, sequence, s)
}

/// The cost mode named in schema 3 records.
#[cfg(not(feature = "comparator-progress"))]
pub const MODE: &str = if cfg!(feature = "comparator-profiling") {
    "profile"
} else if cfg!(feature = "comparator-allocation-counting") {
    "rust-global"
} else {
    "cpu-only"
};

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
    #[cfg(feature = "comparator-progress")]
    writeln!(
        buffer,
        "sophia_comparator_counters schema=2 mode=progress process={process} pid={} stage={stage} sequence={sequence} before_usec={} after_usec={} cpu_ns={} allocations={} reallocations={} deallocations={} bytes_allocated={}",
        std::process::id(),
        s.before_usec,
        s.after_usec,
        s.cpu_ns,
        s.allocations,
        s.reallocations,
        s.deallocations,
        s.bytes_allocated
    )?;
    #[cfg(not(feature = "comparator-progress"))]
    writeln!(
        buffer,
        "sophia_comparator_counters schema=3 mode={MODE} process={process} pid={} stage={stage} sequence={sequence} before_usec={} after_usec={} cpu_ns={} allocations={} reallocations={} deallocations={} bytes_allocated={} minor_faults={} major_faults={}",
        std::process::id(),
        s.before_usec,
        s.after_usec,
        s.cpu_ns,
        s.allocations,
        s.reallocations,
        s.deallocations,
        s.bytes_allocated,
        s.minor_faults,
        s.major_faults
    )?;
    let len = buffer.position() as usize;
    output.write_all(&bytes[..len])?;
    #[cfg(feature = "comparator-progress")]
    if stage != "sample" || sequence.is_multiple_of(8) {
        write_progress(output, process, sequence)?;
    }
    Ok(())
}

#[cfg(feature = "comparator-progress")]
pub fn write_progress(
    output: &mut impl Write,
    process: &str,
    sequence: u64,
) -> std::io::Result<()> {
    // Timestamp the snapshot here, never at an instrumented service call.
    let before = nanos(rustix::time::ClockId::Monotonic) / 1000;
    let counts = progress::snapshot();
    let ledger = progress::service_ledger::snapshot();
    let after = nanos(rustix::time::ClockId::Monotonic) / 1000;
    let mut bytes = [0u8; 4096];
    let mut buffer = std::io::Cursor::new(bytes.as_mut_slice());
    write!(
        buffer,
        "sophia_comparator_progress schema=3 mode=progress process={process} pid={} sequence={sequence} before_usec={before} after_usec={after}",
        std::process::id()
    )?;
    for (name, count) in progress::NAMES.iter().zip(counts) {
        write!(buffer, " {name}={count}")?;
    }
    for (name, count) in progress::service_ledger::NAMES.iter().zip(ledger) {
        write!(buffer, " {name}={count}")?;
    }
    writeln!(buffer)?;
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
                write_snapshot(&mut output, "session", "start", 0, snapshot()?)?;
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
                                snapshot()?,
                            );
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            write_snapshot(
                                &mut output,
                                "session",
                                "sample",
                                sequence,
                                snapshot()?,
                            )?;
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
                write_snapshot(&mut output, "session", "exhausted", 60_001, snapshot()?)?;
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

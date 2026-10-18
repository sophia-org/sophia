//! Diagnostic scopes only. Scopes nest; their totals must not be added.
//! CPU belongs to the calling thread. Allocation deltas cover the process
//! while a scope is open, including other threads, not exclusive stack costs.
use std::{
    io::{self, Write},
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy)]
pub enum Phase {
    Owner,
    Readiness,
    Components,
    Wire,
    Diagnostics,
    Preparation,
    AfterComponents,
    Lifecycle,
    Wm,
    Authority,
    NativeRequest,
    NativeWait,
    AuthorityWait,
    LifecycleNative,
    WireAuthentication,
    WireServer,
    WireImage,
}
const NAMES: [&str; 17] = [
    "owner",
    "readiness",
    "components",
    "wire",
    "diagnostics",
    "preparation",
    "after_components",
    "lifecycle",
    "wm",
    "authority",
    "native_request",
    "native_wait",
    "authority_wait",
    "lifecycle_native",
    "wire_authentication",
    "wire_server",
    "wire_image",
];
struct Totals {
    calls: AtomicU64,
    cpu: AtomicU64,
    allocations: AtomicU64,
}
static TOTALS: [Totals; 17] = [const {
    Totals {
        calls: AtomicU64::new(0),
        cpu: AtomicU64::new(0),
        allocations: AtomicU64::new(0),
    }
}; 17];

pub struct Guard {
    phase: Phase,
    cpu: u64,
    allocations: u64,
    // A thread CPU interval must end on the thread that opened it.
    thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
fn allocations() -> u64 {
    let counts = super::ALLOCATOR.stats();
    (counts.allocations as u64).saturating_add(counts.reallocations as u64)
}
pub fn enter(phase: Phase) -> Guard {
    Guard {
        phase,
        cpu: super::nanos(rustix::time::ClockId::ThreadCPUTime),
        allocations: allocations(),
        thread: std::marker::PhantomData,
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        let cpu = super::nanos(rustix::time::ClockId::ThreadCPUTime).saturating_sub(self.cpu);
        let allocations = allocations().saturating_sub(self.allocations);
        let totals = &TOTALS[self.phase as usize];
        totals.cpu.fetch_add(cpu, Ordering::Relaxed);
        totals.allocations.fetch_add(allocations, Ordering::Relaxed);
        totals.calls.fetch_add(1, Ordering::Release);
    }
}

pub fn record(output: &mut impl Write, sequence: u64) -> io::Result<()> {
    #[cfg(feature = "comparator-request-attribution")]
    record_requests(output, sequence)?;
    for (name, totals) in NAMES.iter().zip(&TOTALS) {
        let before = super::nanos(rustix::time::ClockId::Monotonic) / 1000;
        let calls = totals.calls.load(Ordering::Acquire);
        let cpu = totals.cpu.load(Ordering::Relaxed);
        let allocations = totals.allocations.load(Ordering::Relaxed);
        let after = super::nanos(rustix::time::ClockId::Monotonic) / 1000;
        let mut bytes = [0u8; 1024];
        let mut buffer = io::Cursor::new(bytes.as_mut_slice());
        writeln!(
            buffer,
            "sophia_comparator_profile schema=1 phase={name} sequence={sequence} before_usec={before} after_usec={after} completed_scopes={calls} thread_cpu_ns={cpu} process_allocations_during_scopes={allocations}"
        )?;
        let len = buffer.position() as usize;
        output.write_all(&bytes[..len])?;
    }
    Ok(())
}

#[cfg(feature = "comparator-request-attribution")]
fn record_requests(output: &mut impl Write, sequence: u64) -> io::Result<()> {
    use sophia_9p::request_attribution::{NAMES, snapshot};
    let before = super::nanos(rustix::time::ClockId::Monotonic) / 1000;
    let counts = snapshot();
    let after = super::nanos(rustix::time::ClockId::Monotonic) / 1000;
    for (site, counts) in NAMES.into_iter().zip(counts) {
        let mut bytes = [0u8; 1024];
        let mut buffer = io::Cursor::new(bytes.as_mut_slice());
        writeln!(
            buffer,
            "sophia_comparator_request_profile schema=1 mode=profiling site={site} sequence={sequence} before_usec={before} after_usec={after} completed_scopes={} thread_cpu_ns={} process_allocations_during_scopes={} process_reallocations_during_scopes={} process_bytes_during_scopes={} nested=true",
            counts.calls,
            counts.thread_cpu_ns,
            counts.process_allocations,
            counts.process_reallocations,
            counts.process_bytes
        )?;
        let len = buffer.position() as usize;
        output.write_all(&bytes[..len])?;
    }
    Ok(())
}

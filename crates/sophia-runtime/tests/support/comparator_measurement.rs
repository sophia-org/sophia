//! Instrumentation preflight. No B9 verdict until the complete method is frozen.
use std::time::Instant;

#[cfg(feature = "comparator-allocation-counting")]
#[global_allocator]
static ALLOCATOR: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

fn allocations() -> Option<stats_alloc::Stats> {
    #[cfg(feature = "comparator-allocation-counting")]
    {
        Some(ALLOCATOR.stats())
    }
    #[cfg(not(feature = "comparator-allocation-counting"))]
    {
        None
    }
}

fn cpu_ns() -> u128 {
    let value = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    u128::try_from(value.tv_sec).unwrap() * 1_000_000_000 + u128::try_from(value.tv_nsec).unwrap()
}

pub struct Window {
    wall: Instant,
    cpu: u128,
    allocations: Option<stats_alloc::Stats>,
}

#[derive(Debug)]
pub struct Sample {
    wall_ns: u128,
    cpu_ns: u128,
    allocations: Option<stats_alloc::Stats>,
}

impl Window {
    pub fn begin() -> Self {
        let allocations = allocations();
        let cpu = cpu_ns();
        Self {
            wall: Instant::now(),
            cpu,
            allocations,
        }
    }

    pub fn end(self) -> Sample {
        let wall_ns = self.wall.elapsed().as_nanos();
        let cpu_ns = cpu_ns().checked_sub(self.cpu).unwrap();
        let allocations = allocations()
            .zip(self.allocations)
            .map(|(end, start)| end - start);
        Sample {
            wall_ns,
            cpu_ns,
            allocations,
        }
    }
}

fn count(name: &str, maximum: usize) -> usize {
    let value: usize = std::env::var(name).expect(name).parse().expect(name);
    assert!((1..=maximum).contains(&value), "invalid {name}");
    value
}

#[test]
#[ignore = "explicit serial diagnostic probe; not performance acceptance"]
fn resource_probe() {
    // Process-wide counters include the client and owner. Another test in
    // this process would contaminate them; reject an ambiguous invocation.
    let args: Vec<_> = std::env::args().collect();
    for required in [
        "measurement::resource_probe",
        "--exact",
        "--ignored",
        "--test-threads=1",
    ] {
        assert!(
            args.iter().any(|arg| arg == required),
            "required test argument: {required}"
        );
    }
    let samples = count("SOPHIA_COMPARATOR_SAMPLES", 10000);
    let warmup = count("SOPHIA_COMPARATOR_WARMUP", 1000);
    println!(
        "shell_comparator_method schema=1 verdict=diagnostic samples_per_wire_size={samples} warmup_per_wire_size={warmup} allocation_instrumentation={} cpu_scope=client_and_owner_process latency_start=before_resource_begin latency_end=decoded_accepted lifecycle=fresh_connection order=alternating wall_clock=monotonic cpu_clock=process_cpu",
        cfg!(feature = "comparator-allocation-counting")
    );
    for (width, height) in [(1920, 24), (1024, 1024)] {
        for iteration in 0..warmup + samples {
            let phase = if iteration < warmup {
                "warmup"
            } else {
                "sample"
            };
            for files in if iteration % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            } {
                let wire = if files { "files" } else { "ipc" };
                let bytes = width as u64 * height as u64 * 4;
                // An offered record without a result makes an aborted run
                // visible. Failed samples are never silently replaced.
                println!(
                    "shell_comparator_offer schema=1 phase={phase} iteration={iteration} wire={wire} bytes={bytes}"
                );
                let (_, sample) = super::measured_exchange(files, width, height);
                let (allocs, reallocs, deallocs, allocated_bytes) = sample.allocations.map_or(
                    (
                        "unmeasured".into(),
                        "unmeasured".into(),
                        "unmeasured".into(),
                        "unmeasured".into(),
                    ),
                    |s| {
                        (
                            s.allocations.to_string(),
                            s.reallocations.to_string(),
                            s.deallocations.to_string(),
                            s.bytes_allocated.to_string(),
                        )
                    },
                );
                println!(
                    "shell_comparator_result schema=1 phase={phase} iteration={iteration} wire={wire} bytes={bytes} status=accepted wall_ns={} cpu_ns={} allocations={allocs} reallocations={reallocs} deallocations={deallocs} allocated_bytes={allocated_bytes} transfer_bound_exceeded={}",
                    sample.wall_ns,
                    sample.cpu_ns,
                    sample.wall_ns >= 2_000_000_000
                );
            }
        }
    }
    println!(
        "shell_comparator_end schema=1 verdict=diagnostic offered={} accepted={} native_presentation=unmeasured allocations_per_presented_frame=unmeasured",
        4 * (warmup + samples),
        4 * (warmup + samples)
    );
}

//! Explicit timing discriminator; never an authority-cache experiment.
use super::{Probe, ProtectionDomainRole, ready_probe};
use std::hint::black_box;

fn thread_ns() -> u64 {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    u64::try_from(time.tv_sec).unwrap() * 1_000_000_000 + u64::try_from(time.tv_nsec).unwrap()
}

#[test]
#[ignore = "explicit optimized custody timing; requires nested user namespaces"]
#[allow(clippy::assertions_on_constants)]
fn measure_live_protected_launch_custody() {
    assert!(!cfg!(debug_assertions), "use --release for timing");
    let probe = Probe::build();
    let (mut supervisor, _owner, reservation) = ready_probe(&probe, 1);
    let custody = supervisor
        .launch_custody(&reservation, ProtectionDomainRole::MetadataShell)
        .unwrap()
        .unwrap();
    for _ in 0..1024 {
        assert!(black_box(&custody).is_live().unwrap());
    }
    const CALLS: u64 = 65_536;
    let mut samples = [0; 8];
    for (round, sample) in samples.iter_mut().enumerate() {
        let start = thread_ns();
        for _ in 0..CALLS {
            assert!(black_box(black_box(&custody).is_live().unwrap()));
        }
        *sample = thread_ns() - start;
        println!("custody_timing round={round} calls={CALLS} thread_cpu_ns={sample}");
    }
    samples.sort_unstable();
    println!(
        "custody_timing_summary calls={CALLS} median_thread_cpu_ns={} ns_per_call={:.2}",
        samples[samples.len() / 2],
        samples[samples.len() / 2] as f64 / CALLS as f64,
    );
    supervisor.terminate().unwrap();
    assert!(!custody.is_live().unwrap());
}

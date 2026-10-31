//! Explicit scan-only experiment. Mapping, page population and setup are untimed.
use std::{hint::black_box, os::fd::AsFd};

use sophia_linux_peer::SealedMapping;
use sophia_runtime::{ISB_PIXEL_VALIDATION_BYTES, IsbPixelValidation};

#[path = "support/isb_block_reference.rs"]
mod block_reference;
#[path = "support/isb_scan.rs"]
#[allow(dead_code)]
mod scan;

fn populate(mapping: &SealedMapping) {
    black_box(
        mapping
            .bytes()
            .iter()
            .step_by(4096)
            .fold(0u8, |acc, byte| acc ^ byte),
    );
}

fn thread_cpu_ns() -> u64 {
    let time = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    u64::try_from(time.tv_sec).unwrap() * 1_000_000_000 + u64::try_from(time.tv_nsec).unwrap()
}

fn measure(source: &std::os::fd::OwnedFd, len: usize, baseline: bool) -> u64 {
    let mapping = SealedMapping::map(source.as_fd().try_clone_to_owned().unwrap(), len).unwrap();
    populate(&mapping);
    if baseline {
        let mut work = block_reference::IsbPixelValidation::new(black_box(mapping)).unwrap();
        let start = thread_cpu_ns();
        while !black_box(
            work.step_with_budget(black_box(ISB_PIXEL_VALIDATION_BYTES))
                .unwrap(),
        ) {}
        let elapsed = thread_cpu_ns().checked_sub(start).unwrap();
        black_box(work.finish().unwrap());
        elapsed
    } else {
        let mut work = IsbPixelValidation::new(black_box(mapping)).unwrap();
        let start = thread_cpu_ns();
        while !black_box(
            work.step_with_budget(black_box(ISB_PIXEL_VALIDATION_BYTES))
                .unwrap(),
        ) {}
        let elapsed = thread_cpu_ns().checked_sub(start).unwrap();
        black_box(work.finish().unwrap());
        elapsed
    }
}

#[test]
#[ignore = "scan timing experiment; run explicitly in an optimized build"]
fn compare_fixed_blocks_and_opaque_shortcut_on_warm_images() {
    if cfg!(debug_assertions) {
        panic!("use --release for scan timing");
    }
    for size in [184_320, 3_932_160] {
        for pattern in [
            "opaque",
            "varying-alpha",
            "alternating-alpha",
            "one-translucent-per-unit",
        ] {
            let mut bytes = vec![0; size];
            for (index, pixel) in bytes.chunks_exact_mut(4).enumerate() {
                let alpha = match pattern {
                    "opaque" => 255,
                    "varying-alpha" => (index % 256) as u8,
                    "one-translucent-per-unit" => {
                        if (index * 4).is_multiple_of(ISB_PIXEL_VALIDATION_BYTES) {
                            128
                        } else {
                            255
                        }
                    }
                    "alternating-alpha" => {
                        if index % 2 == 0 {
                            255
                        } else {
                            128
                        }
                    }
                    _ => unreachable!(),
                };
                pixel.copy_from_slice(&[alpha / 3, alpha / 2, alpha, alpha]);
            }
            let source = scan::source(&bytes);
            for (block, baseline) in [true, false, false, true].into_iter().enumerate() {
                let mut times = Vec::new();
                for iteration in 0..320 {
                    let elapsed = measure(&source, bytes.len(), baseline);
                    if iteration >= 120 {
                        times.push(elapsed);
                    }
                }
                let mean = times.iter().sum::<u64>() / times.len() as u64;
                times.sort_unstable();
                println!(
                    "isb_scan_summary bytes={size} pattern={pattern} block={block} baseline={baseline} trials={} mean_thread_ns={mean} p50_thread_ns={} max_thread_ns={} scope=warm-mapped-scan-only",
                    times.len(),
                    times[times.len() / 2],
                    times.last().unwrap()
                );
            }
        }
    }
}

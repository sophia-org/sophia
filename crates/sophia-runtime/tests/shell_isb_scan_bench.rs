//! Explicit scan-only experiment. Mapping, page population and setup are untimed.
use std::{
    hint::black_box,
    os::fd::AsFd,
    time::{Duration, Instant},
};

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

fn measure(source: &std::os::fd::OwnedFd, len: usize, baseline: bool) -> Duration {
    let mapping = SealedMapping::map(source.as_fd().try_clone_to_owned().unwrap(), len).unwrap();
    populate(&mapping);
    if baseline {
        let mut work = block_reference::IsbPixelValidation::new(black_box(mapping)).unwrap();
        let start = Instant::now();
        while !black_box(
            work.step_with_budget(black_box(ISB_PIXEL_VALIDATION_BYTES))
                .unwrap(),
        ) {}
        let elapsed = start.elapsed();
        black_box(work.finish().unwrap());
        elapsed
    } else {
        let mut work = IsbPixelValidation::new(black_box(mapping)).unwrap();
        let start = Instant::now();
        while !black_box(
            work.step_with_budget(black_box(ISB_PIXEL_VALIDATION_BYTES))
                .unwrap(),
        ) {}
        let elapsed = start.elapsed();
        black_box(work.finish().unwrap());
        elapsed
    }
}

#[test]
#[ignore = "scan timing experiment; run explicitly in an optimized build"]
#[allow(clippy::assertions_on_constants)] // Compile in debug; refuse timing there at runtime.
fn compare_scalar_blocks_and_fixed_blocks_on_warm_images() {
    assert!(!cfg!(debug_assertions), "use --release for scan timing");
    for size in [184_320, 4 * 1024 * 1024] {
        for opaque in [false, true] {
            let mut bytes = vec![0; size];
            for (index, pixel) in bytes.chunks_exact_mut(4).enumerate() {
                let alpha = if opaque { 255 } else { (index % 256) as u8 };
                pixel.copy_from_slice(&[alpha / 3, alpha / 2, alpha, alpha]);
            }
            let source = scan::source(&bytes);
            for _ in 0..4 {
                black_box(measure(&source, bytes.len(), true));
                black_box(measure(&source, bytes.len(), false));
            }
            let mut old = Vec::new();
            let mut new = Vec::new();
            for round in 0..32 {
                // Alternate the first implementation to avoid a fixed order bias.
                for baseline in [round % 2 == 0, round % 2 != 0] {
                    let elapsed = measure(&source, bytes.len(), baseline).as_nanos();
                    if baseline {
                        old.push(elapsed);
                    } else {
                        new.push(elapsed);
                    }
                    println!(
                        "isb_scan_trial opaque={opaque} round={round} baseline={baseline} bytes={} ns={elapsed}",
                        bytes.len()
                    );
                }
            }
            old.sort_unstable();
            new.sort_unstable();
            println!(
                "isb_scan_summary bytes={size} opaque={opaque} trials={} baseline_median_ns={} candidate_median_ns={} baseline_min_ns={} candidate_min_ns={}",
                old.len(),
                old[old.len() / 2],
                new[new.len() / 2],
                old[0],
                new[0]
            );
        }
    }
}

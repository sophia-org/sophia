//! Differential controls for bounded validation; no native devices or worker timing.
use sophia_runtime::{ContentStoreError, ISB_PIXEL_VALIDATION_BYTES, IsbPixelValidation};

#[path = "support/isb_scan.rs"]
mod scan;

fn compare(bytes: &[u8], budgets: &[usize]) {
    let mut actual = IsbPixelValidation::new(scan::mapped(bytes)).unwrap();
    let mut expected = scan::ScalarValidation::new(bytes);
    for &budget in budgets {
        assert_eq!(
            actual.step_with_budget(budget),
            expected.step(budget),
            "budget={budget}"
        );
    }
    assert_eq!(actual.finish().is_ok(), expected.finished());
}

#[test]
fn units_split_pixels_and_blocks_without_checking_future_colours() {
    compare(&[200, 0, 0, 255], &[1, 1, 1, 1]);
    let budgets = [1, 2, 3, 5, 7, 15, 16, 17, 63, 64, 65];
    for budget in budgets {
        let schedule = vec![budget; 132usize.div_ceil(budget) + 1];
        for bad in 0..132 {
            if bad % 4 == 3 {
                continue;
            }
            let mut bytes = [0; 132];
            bytes[bad] = 1;
            compare(&bytes, &schedule);
        }
        compare(&[0; 132], &schedule);
        compare(&[255; 132], &schedule);
    }
}

#[test]
fn unit_cap_and_bad_boundary_pixels_match_scalar_scan() {
    let len = ISB_PIXEL_VALIDATION_BYTES * 2 + 4;
    let base = vec![0; len];
    for start in [0, 1, 2, 3, 63, 64, 65] {
        let schedule = [start, usize::MAX, 0, usize::MAX, usize::MAX, 1];
        compare(&base, &schedule);
        for boundary in [ISB_PIXEL_VALIDATION_BYTES, ISB_PIXEL_VALIDATION_BYTES * 2] {
            for bad in boundary - 4..boundary + 4 {
                if bad % 4 == 3 {
                    continue;
                }
                let mut bytes = base.clone();
                bytes[bad] = 1;
                compare(&bytes, &schedule);
            }
        }
    }
}

#[test]
fn failure_is_sticky_and_incomplete_or_malformed_scans_never_finish() {
    compare(&[0; 8], &[0, 1]);
    compare(&[0; 8], &[0, 1, 0, 7, 0, 1]);
    compare(&[1, 0, 0, 0, 0, 0, 0, 0], &[1, 0, 8, usize::MAX]);
    for len in [1, 2, 3, 5, 6, 7] {
        assert!(matches!(
            IsbPixelValidation::new(scan::mapped(&vec![0; len])),
            Err(ContentStoreError::Malformed)
        ));
    }
}

fn next(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

#[test]
fn deterministic_random_pixels_and_budget_sequences_match_scalar_scan() {
    let mut seed = 0x5ca1_ab1e_0137_9bdf;
    for case in 0..512 {
        let len = ((next(&mut seed) % 1024 + 1) * 4) as usize;
        let mut bytes = vec![0; len];
        for pixel in bytes.chunks_exact_mut(4) {
            let alpha = (next(&mut seed) & 255) as u8;
            for value in &mut pixel[..3] {
                *value = (next(&mut seed) % (u64::from(alpha) + 1)) as u8;
            }
            pixel[3] = alpha;
        }
        if case % 2 == 0 {
            let pixel = (next(&mut seed) as usize % (len / 4)) * 4;
            let component = next(&mut seed) as usize % 3;
            bytes[pixel + 3] = 254;
            bytes[pixel + component] = 255;
        }
        let mut schedule = Vec::new();
        let mut offered = 0;
        while offered < len {
            let budget = next(&mut seed) as usize % 130;
            schedule.push(budget);
            offered += budget;
        }
        schedule.extend([0, usize::MAX]);
        compare(&bytes, &schedule);
    }
}

//! Comparator fill experiment only; the ISB builder does not prescribe pixels.
use sophia_isb::MappedIsb;
use std::{hint::black_box, time::Instant};

type Fill = fn(&mut [u8], u64);

#[inline(never)]
fn byte_fill(bytes: &mut [u8], generation: u64) {
    let shade = (generation % 128) as u8;
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = if index % 4 == 3 { 255 } else { shade };
    }
}

#[inline(never)]
fn pixel_fill(bytes: &mut [u8], generation: u64) {
    let shade = (generation % 128) as u8;
    let (pixels, remainder) = bytes.as_chunks_mut::<4>();
    pixels.fill([shade, shade, shade, 255]);
    remainder.fill(shade);
}

#[test]
fn whole_pixel_fill_matches_bytes_at_block_edges_and_unaligned_offsets() {
    for pixels in (0..=65).chain([16383, 16384, 16385]) {
        for offset in 0..8 {
            for generation in (0..128).chain([128, 255, 256, u64::MAX]) {
                let mut original = vec![0xa5; pixels * 4 + 16];
                let mut candidate = original.clone();
                let range = offset..offset + pixels * 4;
                byte_fill(&mut original[range.clone()], generation);
                pixel_fill(&mut candidate[range.clone()], generation);
                assert_eq!(candidate, original);
                assert!(candidate[..offset].iter().all(|&b| b == 0xa5));
                assert!(candidate[range.end..].iter().all(|&b| b == 0xa5));
                let shade = (generation % 128) as u8;
                assert!(
                    candidate[range]
                        .chunks_exact(4)
                        .all(|pixel| pixel == [shade, shade, shade, 255])
                );
            }
        }
    }
}

#[test]
fn partial_pixel_prefixes_match_the_original_byte_loop() {
    for len in [1, 2, 3, 5, 6, 7, 63, 65] {
        let mut bytes = vec![0xa5; len];
        let mut expected = bytes.clone();
        byte_fill(&mut expected, 1);
        pixel_fill(&mut bytes, 1);
        assert_eq!(bytes, expected);
    }
}

#[inline(never)]
fn fresh_sample(width: u32, height: u32, generation: u64, fill: Fill) -> [u128; 2] {
    let start = Instant::now();
    let mut image =
        MappedIsb::create(width, height, u64::from(width) * u64::from(height) * 4).unwrap();
    let fill_start = Instant::now();
    fill(black_box(image.pixels_mut()), black_box(generation));
    let fill_end = Instant::now();
    let fd = image.finish().unwrap();
    drop(black_box(fd));
    let end = Instant::now();
    [(fill_end - fill_start).as_nanos(), (end - start).as_nanos()]
}

#[test]
#[ignore = "local paired diagnostic; explicitly run --release on an idle host"]
fn paired_fresh_isb_fill_cost() {
    assert!(!black_box(cfg!(debug_assertions)), "requires --release");
    const ROUNDS: usize = 512;
    let variants: [(&str, Fill); 2] = [("byte", byte_fill), ("pixel", pixel_fill)];
    for (width, height) in [(1920, 24), (1024, 1024)] {
        for generation in 0..64 {
            for (_, fill) in variants {
                black_box(fresh_sample(width, height, generation, black_box(fill)));
            }
        }
        let mut samples = [Vec::with_capacity(ROUNDS), Vec::with_capacity(ROUNDS)];
        for generation in 0..ROUNDS {
            // Alternate order; both arms always allocate and seal fresh ISBs.
            for index in [generation % 2, 1 - generation % 2] {
                samples[index].push(fresh_sample(
                    width,
                    height,
                    generation as u64,
                    black_box(variants[index].1),
                ));
            }
        }
        for (index, (variant, _)) in variants.into_iter().enumerate() {
            for (field, stage) in ["fill", "total"].into_iter().enumerate() {
                let mut values: Vec<_> = samples[index].iter().map(|r| r[field]).collect();
                values.sort_unstable();
                println!(
                    "isb_fill_pair schema=1 width={width} height={height} variant={variant} stage={stage} rounds={ROUNDS} median_ns={} p95_ns={} mean_ns={}",
                    values[ROUNDS / 2],
                    values[ROUNDS * 95 / 100],
                    values.iter().sum::<u128>() / ROUNDS as u128,
                );
            }
        }
    }
}

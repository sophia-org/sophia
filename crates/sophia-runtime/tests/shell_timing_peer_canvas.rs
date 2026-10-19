//! The peer's one upload canvas, without a wire. No native display claim.
#[path = "../examples/shell_timing_peer/canvas.rs"]
mod canvas;
use canvas::{Canvas, rgba_bytes};

fn pattern(length: usize, generation: u64) -> Vec<u8> {
    (0..length)
        .map(|index| {
            if index % 4 == 3 {
                255
            } else {
                (generation % 128) as u8
            }
        })
        .collect()
}

#[test]
fn canvas_allocates_on_first_fill_and_never_grows() {
    let mut canvas = Canvas::new(4096);
    assert_eq!(canvas.allocated(), 0, "construction must not allocate");
    let first = canvas.fill(1024, 1).unwrap().as_ptr();
    assert_eq!(canvas.allocated(), 4096);
    let second = canvas.fill(4096, 2).unwrap().as_ptr();
    let third = canvas.fill(16, 3).unwrap().as_ptr();
    assert_eq!((first, second), (third, third), "the canvas moved");
    assert_eq!(canvas.allocated(), 4096);
}

#[test]
fn every_fill_overwrites_its_prefix_with_the_generation_bytes() {
    let mut canvas = Canvas::new(4096);
    assert_eq!(canvas.fill(4096, 7).unwrap(), pattern(4096, 7));
    // A shorter upload after a longer one sends only its own fresh prefix.
    assert_eq!(canvas.fill(1024, 8).unwrap(), pattern(1024, 8));
    assert_eq!(canvas.fill(4096, 129).unwrap(), pattern(4096, 1));
}

#[test]
fn packed_fill_preserves_partial_prefixes_and_unaligned_ranges() {
    for length in 0..=260 {
        for offset in 0..8 {
            for generation in [0, 1, 127, 128, 255, u64::MAX] {
                let mut bytes = vec![0xa5; length + 16];
                canvas::fill_pixels(&mut bytes[offset..offset + length], generation);
                assert_eq!(&bytes[offset..offset + length], pattern(length, generation));
                assert!(bytes[..offset].iter().all(|&b| b == 0xa5));
                assert!(bytes[offset + length..].iter().all(|&b| b == 0xa5));
            }
        }
    }
}

#[test]
fn canvas_refuses_empty_and_oversized_uploads_without_allocating() {
    let mut canvas = Canvas::new(64);
    assert!(canvas.fill(0, 1).is_err());
    assert!(canvas.fill(65, 1).is_err());
    assert_eq!(canvas.allocated(), 0);
    assert_eq!(canvas.fill(64, 1).unwrap().len(), 64);
    assert!(canvas.fill(65, 2).is_err());
    assert_eq!(canvas.allocated(), 64);
}

#[test]
fn image_sizes_are_checked_at_the_arithmetic_boundary() {
    assert_eq!(rgba_bytes(1920, 24), Some(184_320));
    assert_eq!(rgba_bytes(1024, 1024), Some(4 << 20));
    assert_eq!(rgba_bytes(0, u32::MAX), Some(0));
    // width * height fits u64 for any u32 pair; the factor of four is the edge.
    let largest = u64::from(u32::MAX) * u64::from(u32::MAX);
    assert!(largest.checked_mul(4).is_none());
    assert_eq!(rgba_bytes(u32::MAX, u32::MAX), None);
    assert_eq!(
        rgba_bytes(u32::MAX, 1 << 30),
        Some(u64::from(u32::MAX) << 32)
    );
    assert_eq!(rgba_bytes(u32::MAX, (1 << 30) + 1), None);
}

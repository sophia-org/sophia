use sophia_portal_protocol::{IMAGE_MAX_BYTES, decode_frame_info};
fn record(fields: [u64; 8]) -> Vec<u8> {
    fields.into_iter().flat_map(u64::to_le_bytes).collect()
}
#[test]
fn frame_bounds_and_exact_record() {
    let valid = [1, u64::MAX, 2, 3, 4096, 4096, 16384, 1];
    let bytes = record(valid);
    let frame = decode_frame_info(&bytes).unwrap();
    assert_eq!(frame.output, u64::MAX);
    assert_eq!(
        (frame.output_generation, frame.presentation_generation),
        (2, 3)
    );
    assert_eq!(
        (frame.width, frame.height, frame.stride),
        (4096, 4096, 16384)
    );
    assert_eq!(frame.image_bytes, IMAGE_MAX_BYTES);
    for n in 0..64 {
        assert!(decode_frame_info(&bytes[..n]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode_frame_info(&trailing).is_err());
    for (index, value) in [
        (0, 2),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 0),
        (4, u32::MAX as u64 + 1),
        (5, u32::MAX as u64 + 1),
        (5, 4097),
        (6, 16385),
        (7, 2),
    ] {
        let mut f = valid;
        f[index] = value;
        assert!(decode_frame_info(&record(f)).is_err(), "field {index}");
    }
    assert!(
        decode_frame_info(&record([
            1,
            1,
            1,
            1,
            u32::MAX as u64,
            u32::MAX as u64,
            u32::MAX as u64 * 4,
            1
        ]))
        .is_err()
    );
    assert_eq!(
        decode_frame_info(&record([1, 1, 1, 1, 1, 1, 4, 1]))
            .unwrap()
            .image_bytes,
        4
    );
}

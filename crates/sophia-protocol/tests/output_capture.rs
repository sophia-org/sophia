use sophia_protocol::{OutputId, output_capture::*};

#[test]
fn record_has_exact_versioned_fields_and_no_trailing_bytes() {
    let info = OutputCaptureFrameInfo {
        output: OutputId::from_raw(7),
        output_generation: 9,
        presentation_generation: 12,
        width: 2,
        height: 3,
    };
    let record = info.encode().unwrap();
    let mut golden = [0u8; 64];
    for (offset, value) in [
        (0, 1),
        (8, 7),
        (16, 9),
        (24, 12),
        (32, 2),
        (40, 3),
        (48, 8),
        (56, 1),
    ] {
        golden[offset] = value;
    }
    assert_eq!(record, golden);
    assert_eq!(OutputCaptureFrameInfo::decode(&golden), Some(info));
    assert_eq!(info.byte_len(), Some(24));
    for offset in [0, 8, 16, 24, 32, 40, 48, 56] {
        let mut invalid = golden;
        invalid[offset] = 0;
        assert_eq!(OutputCaptureFrameInfo::decode(&invalid), None);
    }
    assert_eq!(OutputCaptureFrameInfo::decode(&golden[..63]), None);
    assert_eq!(OutputCaptureFrameInfo::decode(&[0; 65]), None);
    assert_eq!(
        OutputCaptureFrameInfo {
            width: u32::MAX,
            ..info
        }
        .byte_len(),
        None
    );
    assert_eq!(
        OutputCaptureFrameInfo {
            width: 8192,
            height: 8192,
            ..info
        }
        .byte_len(),
        None
    );
}

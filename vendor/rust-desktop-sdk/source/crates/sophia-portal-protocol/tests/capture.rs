use sophia_portal_protocol::*;
#[test]
fn capture_wire_and_status_boundaries() {
    let request = CaptureRequest {
        serial: 0x0102030405060708,
        output: u64::MAX,
    };
    let bytes = encode_capture(request).unwrap();
    assert_eq!(
        bytes,
        [
            1, 0, 0, 0, 0, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1, 255, 255, 255, 255, 255, 255, 255, 255
        ]
    );
    assert!(
        encode_capture(CaptureRequest {
            serial: 0,
            ..request
        })
        .is_err()
    );
    assert!(
        encode_capture(CaptureRequest {
            output: 0,
            ..request
        })
        .is_err()
    );
    for (state, expected) in [
        CaptureState::Idle,
        CaptureState::Queued,
        CaptureState::Capturing,
        CaptureState::Ready,
        CaptureState::Denied,
        CaptureState::Unavailable,
        CaptureState::Pressure,
        CaptureState::Cancelled,
    ]
    .into_iter()
    .enumerate()
    {
        let mut record = [0; 33];
        record[..24].copy_from_slice(&bytes);
        record[24] = state as u8;
        if state == 0 {
            record[8..24].fill(0);
        }
        let status = decode_capture_status(&record[..32]).unwrap();
        assert_eq!(status.state, expected);
        assert_eq!(status.serial, if state == 0 { 0 } else { request.serial });
        assert_eq!(status.output, if state == 0 { 0 } else { request.output });
        for n in 0..=33 {
            if n != 32 {
                assert!(decode_capture_status(&record[..n]).is_err());
            }
        }
        for offset in [8, 16] {
            let mut invalid = record;
            invalid[offset..offset + 8].fill(if state == 0 { 1 } else { 0 });
            assert!(decode_capture_status(&invalid[..32]).is_err());
        }
        for (offset, value) in [(0, 2), (24, 8), (31, 1)] {
            let mut invalid = record;
            invalid[offset] = value;
            assert!(decode_capture_status(&invalid[..32]).is_err());
        }
    }
}

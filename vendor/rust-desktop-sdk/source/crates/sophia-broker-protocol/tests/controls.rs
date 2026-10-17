use sophia_broker_protocol::{encode_ack, encode_negotiate, encode_submit};
#[test]
fn literal_controls_keep_domain_refusals_and_output_boundaries() {
    for (lo, hi) in [(2, 2), (0, 0), (3, 1), (0, u16::MAX)] {
        let mut out = [0xa5; 48];
        assert_eq!(encode_negotiate(7, 9, lo, hi, &mut out), Ok(40));
        let mut expected = [0; 40];
        expected[0] = 40;
        expected[4] = 1;
        expected[7] = 1;
        expected[8] = 7;
        expected[16] = 9;
        expected[32..34].copy_from_slice(&lo.to_le_bytes());
        expected[34..36].copy_from_slice(&hi.to_le_bytes());
        assert_eq!(out[..40], expected);
        assert_eq!(out[40..], [0xa5; 8]);
    }
    for length in [40u32, 41, 448] {
        let mut out = [0xa5; 32];
        assert_eq!(encode_submit(7, 9, length, &mut out), Ok(24));
        let mut expected = [0; 24];
        expected[0] = 7;
        expected[8] = 9;
        expected[16..20].copy_from_slice(&length.to_le_bytes());
        assert_eq!(out[..24], expected);
        assert_eq!(out[24..], [0xa5; 8]);
    }
    let mut out = [0xa5; 24];
    encode_ack(u64::MAX, 1, &mut out).unwrap();
    assert_eq!(out[..8], [255; 8]);
    assert_eq!(out[8..16], [1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(out[16..], [0xa5; 8]);
}
#[test]
fn rejected_control_never_partially_writes() {
    let mut out = [0xa5; 40];
    for (epoch, id) in [(0, 1), (1, 0)] {
        assert!(encode_negotiate(epoch, id, 2, 2, &mut out).is_err());
        assert!(encode_submit(epoch, id, 40, &mut out).is_err());
        assert!(encode_ack(epoch, id, &mut out).is_err());
    }
    for n in 0..40 {
        assert!(encode_negotiate(1, 1, 2, 2, &mut out[..n]).is_err());
    }
    for n in 0..24 {
        assert!(encode_submit(1, 1, 40, &mut out[..n]).is_err());
    }
    for n in 0..16 {
        assert!(encode_ack(1, 1, &mut out[..n]).is_err());
    }
    for length in [0, 39, 449, u32::MAX] {
        assert!(encode_submit(1, 1, length, &mut out).is_err());
    }
    assert_eq!(out, [0xa5; 40]);
}

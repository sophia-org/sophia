use sophia_broker_protocol::{decode_api, decode_limits};

fn limits() -> [u8; 72] {
    let mut b = [0; 72];
    b[0] = 72;
    b[4] = 1;
    b[6] = 1;
    b[8] = 7;
    b[32] = 2;
    b[40] = 128;
    b[42] = 2;
    for (offset, value) in [
        (36, 1024u32),
        (44, 16),
        (48, 4096),
        (52, 512),
        (56, 5000),
        (60, 2000),
        (64, 12000),
    ] {
        b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    b
}
#[test]
fn exact_api_and_epoch_bounds() {
    for epoch in [1, u64::MAX] {
        let text = format!(
            "sophia-broker-files version=1 role=metadata-broker epoch={epoch} fd_transfer=none\n"
        );
        assert_eq!(decode_api(text.as_bytes()), Ok(epoch));
        for n in 0..text.len() {
            assert!(decode_api(&text.as_bytes()[..n]).is_err());
        }
    }
    for epoch in ["0", "01", "", "-1", "+1", "18446744073709551616", "1 "] {
        let text = format!(
            "sophia-broker-files version=1 role=metadata-broker epoch={epoch} fd_transfer=none\n"
        );
        assert!(decode_api(text.as_bytes()).is_err());
    }
}
#[test]
fn literal_limits_bounds_and_reserved_bytes() {
    let good = limits();
    assert_eq!(decode_limits(&good, 7).unwrap().max_surfaces, 1024);
    assert!(decode_limits(&good, 8).is_err());
    assert!(decode_limits(&good, 0).is_err());
    for n in 0..72 {
        assert!(decode_limits(&good[..n], 7).is_err());
    }
    for offset in [4, 6, 16, 24, 32, 34, 42, 68] {
        let mut bad = good;
        bad[offset] ^= 1;
        assert!(decode_limits(&bad, 7).is_err());
    }
    for (offset, lo, hi) in [
        (36, 1u32, 1024),
        (44, 4, 16),
        (48, 352, 4096),
        (52, 448, 512),
        (56, 1, 5000),
        (60, 1, 2000),
        (64, 1, 12000),
    ] {
        for value in [lo - 1, lo, hi, hi + 1] {
            let mut b = good;
            b[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert_eq!(decode_limits(&b, 7).is_ok(), (lo..=hi).contains(&value));
        }
    }
    for value in [0u16, 1, 128, 129] {
        let mut b = good;
        b[40..42].copy_from_slice(&value.to_le_bytes());
        assert_eq!(decode_limits(&b, 7).is_ok(), (1..=128).contains(&value));
    }
}

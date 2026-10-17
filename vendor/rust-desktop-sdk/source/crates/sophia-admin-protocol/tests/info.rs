use sophia_admin_protocol::{Info, decode_info};

#[test]
fn literal_identity_and_deadline_boundaries() {
    assert_eq!(
        decode_info(b"v1 info 0 1 7 10000 2000 60000\n"),
        Ok(Info {
            session_low: 0,
            session_high: 1,
            connection: 7,
            command_ms: 10000,
            frame_ms: 2000,
            idle_ms: 60000,
        })
    );
    let max = decode_info(
        b"v1 info 18446744073709551615 0 18446744073709551615 4294967295 1 4294967295\n",
    )
    .unwrap();
    assert_eq!(max.session_low, u64::MAX);
    assert_eq!(max.connection, u64::MAX);
    assert_eq!(max.command_ms, u32::MAX);
}

#[test]
fn rejects_malformed_snapshots_and_all_truncated_prefixes() {
    let valid = b"v1 info 0 1 7 10000 2000 60000\n";
    for end in 0..valid.len() {
        assert!(decode_info(&valid[..end]).is_err());
    }
    for bad in [
        "v1 info 0 0 7 1 1 1\n",
        "v1 info 1 0 0 1 1 1\n",
        "v1 info 1 0 1 0 1 1\n",
        "v1 info 1 0 1 1 0 1\n",
        "v1 info 1 0 1 1 1 0\n",
        "v1 info 1 0 1 4294967296 1 1\n",
        "v1 info 18446744073709551616 0 1 1 1 1\n",
        "v1 info 01 0 1 1 1 1\n",
        "v1 info +1 0 1 1 1 1\n",
        "v1 info 1 0 1 1 1 1 \n",
        "v1 info 1  0 1 1 1 1\n",
        "v1 info 1 0 1 1 1 1\r\n",
        "v1 info 1 0 1 1 1 1\n\n",
        "v2 info 1 0 1 1 1 1\n",
        "v1 info 1 0 1 1 1 1\0\n",
        "v1 info １ 0 1 1 1 1\n",
    ] {
        assert!(decode_info(bad.as_bytes()).is_err(), "{bad:?}");
    }
    assert!(decode_info(&[b'1'; 513]).is_err());
    assert!(decode_info(b"v1 info \xff 0 1 1 1 1\n").is_err());
}

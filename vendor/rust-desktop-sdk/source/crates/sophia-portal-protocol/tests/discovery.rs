use sophia_portal_protocol::*;
#[test]
fn discovery_literals_and_malformed_snapshots() {
    for (name, capability) in [
        ("unavailable", RequestCapability::Unavailable),
        ("output-capture-v1", RequestCapability::OutputCaptureV1),
    ] {
        let api = format!(
            "sophia-portal-client version=1 epoch={} request={name} fd_transfer=none\n",
            u64::MAX
        );
        assert_eq!(
            decode_api(api.as_bytes()).unwrap(),
            Api {
                epoch: u64::MAX,
                request: capability
            }
        );
        if capability == RequestCapability::OutputCaptureV1 {
            assert_eq!(api.len(), API_MAX_BYTES);
        }
        for n in 0..api.len() {
            assert!(decode_api(&api.as_bytes()[..n]).is_err());
        }
        for (old, new) in [
            ("18446744073709551615", "0"),
            ("18446744073709551615", "01"),
            ("18446744073709551615", "+1"),
            ("18446744073709551615", "18446744073709551616"),
            ("version=1", "version=2"),
            ("none", "scm-rights"),
            ("\n", "\r\n"),
        ] {
            assert!(decode_api(api.replace(old, new).as_bytes()).is_err());
        }
        assert!(decode_api(format!("{api}x").as_bytes()).is_err());
        for (word, state) in [
            ("absent", TransferState::Absent),
            ("ready", TransferState::Ready),
            ("revoked", TransferState::Revoked),
        ] {
            let text = format!("request={name} transfer={word}\n");
            assert_eq!(
                decode_status(text.as_bytes()).unwrap(),
                Status {
                    request: capability,
                    transfer: state
                }
            );
            for n in 0..text.len() {
                assert!(decode_status(&text.as_bytes()[..n]).is_err());
            }
            assert!(decode_status(format!("{text}\n").as_bytes()).is_err());
        }
    }
    for text in [
        "request=unknown transfer=ready\n",
        "request=unavailable transfer=unknown\n",
        "request=unavailable transfer=ready\0\n",
    ] {
        assert!(decode_status(text.as_bytes()).is_err());
    }
    assert!(decode_api(b"\xff").is_err());
    assert!(decode_status(b"\xff").is_err());
}

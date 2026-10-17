//! Explicit r2 selection preserves the plain r1 negotiation codec.
use sophia_lock_protocol::lock_files::*;

#[test]
fn selection_is_on_wire_and_never_inferred_from_grants() {
    for revision in [1, 2] {
        for capabilities in 0..32 {
            let value = LockNegotiated {
                granted_chords: 0,
                granted_capabilities: capabilities,
            };
            let allowed =
                capabilities & 1 != 0 && capabilities & !(if revision == 1 { 3 } else { 15 }) == 0;
            let encoded = value.encode_at_revision(revision);
            assert_eq!(encoded.is_ok(), allowed, "r{revision}, caps {capabilities}");
            let mut wire = vec![0; 16];
            wire[..2].copy_from_slice(&revision.to_le_bytes());
            wire[8..].copy_from_slice(&capabilities.to_le_bytes());
            assert_eq!(LockNegotiated::decode_selected(&wire).is_ok(), allowed);
            if let Ok(bytes) = encoded {
                assert_eq!(bytes, wire);
                assert_eq!(
                    LockNegotiated::decode_selected(&bytes),
                    Ok((revision, value))
                );
                assert_eq!(
                    LockNegotiated::decode_at_revision(&bytes, revision),
                    Ok(value)
                );
                assert!(LockNegotiated::decode_at_revision(&bytes, 3 - revision).is_err());
                assert_eq!(LockNegotiated::decode(&bytes).is_ok(), revision == 1);
                if revision == 1 {
                    assert_eq!(value.encode().unwrap(), bytes);
                }
            }
        }
    }
}

#[test]
fn malformed_selection_never_installs_a_revision() {
    let value = LockNegotiated {
        granted_chords: 0,
        granted_capabilities: 5,
    };
    let bytes = value.encode_at_revision(2).unwrap();
    for revision in [0u16, 3, u16::MAX] {
        assert!(value.encode_at_revision(revision).is_err());
        assert!(LockNegotiated::decode_at_revision(&bytes, revision).is_err());
        let mut bad = bytes.clone();
        bad[..2].copy_from_slice(&revision.to_le_bytes());
        assert!(LockNegotiated::decode_selected(&bad).is_err());
    }
    for end in 0..bytes.len() {
        assert!(LockNegotiated::decode_selected(&bytes[..end]).is_err());
    }
    let mut bad = bytes.clone();
    bad.push(0);
    assert!(LockNegotiated::decode_selected(&bad).is_err());
    for offset in 4..8 {
        let mut bad = bytes.clone();
        bad[offset] = 1;
        assert!(LockNegotiated::decode_selected(&bad).is_err());
    }
    let mut bad = bytes;
    bad[2] = 9;
    assert!(LockNegotiated::decode_selected(&bad).is_err());
}

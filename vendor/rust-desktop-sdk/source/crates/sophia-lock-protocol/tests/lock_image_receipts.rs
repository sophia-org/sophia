use sophia_lock_protocol::lock_files::*;

#[test]
fn receipt_extension_is_explicit_in_the_contract() {
    let schema: kdl::KdlDocument = include_str!("../../../spec/sophia-lock-files-v1.kdl")
        .parse()
        .unwrap();
    let submitted = schema
        .nodes()
        .first()
        .unwrap()
        .children()
        .unwrap()
        .nodes()
        .iter()
        .find(|node| {
            node.name().value() == "body"
                && node.get(0).and_then(|v| v.as_string()) == Some("Submitted")
        })
        .unwrap();
    let field = submitted
        .children()
        .unwrap()
        .nodes()
        .iter()
        .find(|node| node.get(0).and_then(|v| v.as_string()) == Some("candidate_kind"))
        .unwrap();
    assert_eq!(field.get("min").unwrap().as_integer(), Some(256));
    assert_eq!(field.get("max").unwrap().as_integer(), Some(263));
    assert_eq!(field.get("r2-value").unwrap().as_integer(), Some(0x7000));
}

#[test]
fn image_receipt_requires_explicit_r2() {
    let value = LockSubmitted {
        submission_id: 17,
        candidate_kind: LockFileKind::ImageBegin,
    };
    let bytes = value.encode_at_revision(2).unwrap();
    assert_eq!(u16::from_le_bytes([bytes[8], bytes[9]]), 0x7000);
    assert_eq!(LockSubmitted::decode_at_revision(&bytes, 2).unwrap(), value);
    assert!(value.encode().is_err());
    assert!(LockSubmitted::decode(&bytes).is_err());
    for revision in [0, 1, 3, u16::MAX] {
        assert!(value.encode_at_revision(revision).is_err());
        assert!(LockSubmitted::decode_at_revision(&bytes, revision).is_err());
    }
    for kind in [
        LockFileKind::Negotiate,
        LockFileKind::ResourceBegin,
        LockFileKind::ResourceEnd,
        LockFileKind::ResourceCancel,
        LockFileKind::ResourceRetire,
        LockFileKind::Candidate,
        LockFileKind::FrameDemand,
    ] {
        let legacy = LockSubmitted {
            submission_id: 17,
            candidate_kind: kind,
        };
        assert_eq!(
            legacy.encode_at_revision(2).unwrap(),
            legacy.encode().unwrap()
        );
        assert_eq!(
            LockSubmitted::decode_at_revision(&legacy.encode().unwrap(), 2).unwrap(),
            legacy
        );
    }
    for kind in [
        LockFileKind::ImageAdmitted,
        LockFileKind::SourceReleased,
        LockFileKind::ChannelReady,
        LockFileKind::ChannelClosed,
        LockFileKind::ImageLimits,
        LockFileKind::ImageFormats,
    ] {
        assert!(
            LockSubmitted {
                submission_id: 17,
                candidate_kind: kind
            }
            .encode_at_revision(2)
            .is_err()
        );
        let mut bad = bytes.clone();
        bad[8..10].copy_from_slice(&(kind as u16).to_le_bytes());
        assert!(LockSubmitted::decode_at_revision(&bad, 2).is_err());
    }
    for at in 10..16 {
        let mut bad = bytes.clone();
        bad[at] = 1;
        assert!(LockSubmitted::decode_at_revision(&bad, 2).is_err());
    }
    for end in 0..bytes.len() {
        assert!(LockSubmitted::decode_at_revision(&bytes[..end], 2).is_err());
    }
    let mut bad = bytes.clone();
    bad.push(0);
    assert!(LockSubmitted::decode_at_revision(&bad, 2).is_err());
    let mut bad = bytes;
    bad[..8].fill(0);
    assert!(LockSubmitted::decode_at_revision(&bad, 2).is_err());
    assert!(
        LockSubmitted {
            submission_id: 0,
            ..value
        }
        .encode_at_revision(2)
        .is_err()
    );
}

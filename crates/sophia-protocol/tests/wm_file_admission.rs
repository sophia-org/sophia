use sophia_protocol::wm_files::*;
use sophia_protocol::*;

fn header(kind: WmFileKind) -> WmFileHeader {
    WmFileHeader {
        kind,
        connection_epoch: 7,
        submission_id: if wm_file_class(kind) == WmFileClass::Candidate {
            9
        } else {
            0
        },
        sequence: if wm_file_class(kind) == WmFileClass::Event {
            11
        } else {
            0
        },
    }
}
fn profile() -> PolicyProfileCommand {
    PolicyProfileCommand {
        transaction: TransactionId::from_raw(13),
        identity: PolicyProfileIdentity::new(7, 17, [0x21; 32]).unwrap(),
    }
}
fn changed_length(bytes: Vec<u8>, decode: impl Fn(&[u8]) -> Result<(), WmFilePayloadError>) {
    assert_eq!(decode(&bytes), Ok(()));
    for end in 32..bytes.len() {
        let mut bad = bytes[..end].to_vec();
        bad[..4].copy_from_slice(&u32::try_from(end).unwrap().to_le_bytes());
        assert!(decode(&bad).is_err());
    }
    let mut bad = bytes;
    bad.push(0);
    let len = u32::try_from(bad.len()).unwrap();
    bad[..4].copy_from_slice(&len.to_le_bytes());
    assert_eq!(
        decode(&bad),
        Err(WmFilePayloadError::Envelope(WmFileCodecError::Length))
    );
}

#[test]
fn limits_publish_the_fixed_contract_and_require_profile_support_when_needed() {
    let value = WmFileLimits {
        capability_ceiling: 0,
        profile_required: false,
    };
    let bytes = encode_wm_file_limits(header(WmFileKind::Limits), value).unwrap();
    assert_eq!(decode_wm_file_limits(&bytes).unwrap(), value);
    assert_eq!(
        &bytes[32..],
        &[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 16, 0, 0, 0, 16, 0, 64, 0, 32, 0, 224, 46, 0, 0, 160, 15,
            0, 0, 0, 0, 0, 0,
        ]
    );
    changed_length(bytes.clone(), |b| decode_wm_file_limits(b).map(|_| ()));
    for offset in [40, 44, 48, 50, 52, 56, 60, 62, 63] {
        let mut bad = bytes.clone();
        bad[offset] = 255;
        assert!(decode_wm_file_limits(&bad).is_err(), "field {offset}");
    }
    assert!(
        encode_wm_file_limits(
            header(WmFileKind::Limits),
            WmFileLimits {
                profile_required: true,
                ..value
            }
        )
        .is_err()
    );
    let value = WmFileLimits {
        capability_ceiling: SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION,
        profile_required: true,
    };
    let bytes = encode_wm_file_limits(header(WmFileKind::Limits), value).unwrap();
    assert_eq!(decode_wm_file_limits(&bytes).unwrap(), value);
    let mut bad = bytes;
    bad[32..40].fill(0);
    assert!(decode_wm_file_limits(&bad).is_err());
}

#[test]
fn required_and_optional_capabilities_must_be_disjoint_in_both_directions() {
    for bit in [1, SOPHIA_WM_CAPABILITY_PROFILE_ACTIVATION, 1 << 63] {
        let offer = WmFileNegotiationOffer {
            required_capabilities: bit,
            optional_capabilities: bit,
        };
        assert_eq!(
            encode_wm_file_negotiate(header(WmFileKind::Negotiate), offer),
            Err(WmFilePayloadError::Value)
        );
        let mut body = Vec::new();
        body.extend(bit.to_le_bytes());
        body.extend(bit.to_le_bytes());
        let bytes = encode_wm_file_record(header(WmFileKind::Negotiate), &body).unwrap();
        assert_eq!(
            decode_wm_file_negotiate(&bytes),
            Err(WmFilePayloadError::Value)
        );
    }
}

#[test]
fn capability_records_preserve_required_optional_and_selected_without_negotiating() {
    let value = WmFileNegotiationOffer {
        required_capabilities: 1 << 63,
        optional_capabilities: 0x0123,
    };
    let bytes = encode_wm_file_negotiate(header(WmFileKind::Negotiate), value).unwrap();
    assert_eq!(
        &bytes[32..],
        &[0, 0, 0, 0, 0, 0, 0, 128, 35, 1, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(decode_wm_file_negotiate(&bytes).unwrap(), value);
    changed_length(bytes.clone(), |b| decode_wm_file_negotiate(b).map(|_| ()));
    assert!(decode_wm_file_negotiated(&bytes).is_err());
    // Unknown required bits reach the admission owner's refusal; this byte
    // codec does not grant them or silently remove them from the offer.
    let bytes = encode_wm_file_negotiated(header(WmFileKind::Negotiated), 0x0123).unwrap();
    assert_eq!(decode_wm_file_negotiated(&bytes).unwrap(), 0x0123);
    changed_length(bytes, |b| decode_wm_file_negotiated(b).map(|_| ()));
}

#[test]
fn all_profile_stages_preserve_exact_identity_and_server_transaction() {
    let command = profile();
    let outcomes = [
        PolicyProfileOutcome::Accepted,
        PolicyProfileOutcome::RejectedIdentity,
        PolicyProfileOutcome::RejectedState,
    ];
    for (sent, reply) in [
        (WmFileKind::ProfilePrepare, WmFileKind::ProfilePrepared),
        (WmFileKind::ProfileActivate, WmFileKind::ProfileActive),
        (WmFileKind::ProfileRollback, WmFileKind::ProfileRolledBack),
    ] {
        let bytes = encode_wm_file_profile_command(header(sent), command, u64::MAX).unwrap();
        assert_eq!(
            &bytes[32..48],
            &[13, 0, 0, 0, 0, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(&bytes[48..80], &[0x21; 32]);
        assert_eq!(
            decode_wm_file_profile_command(&bytes, sent, u64::MAX).unwrap(),
            command
        );
        changed_length(bytes.clone(), |b| {
            decode_wm_file_profile_command(b, sent, u64::MAX).map(|_| ())
        });
        assert!(decode_wm_file_profile_command(&bytes, sent, 0).is_err());
        assert!(decode_wm_file_profile_command(&bytes, WmFileKind::Cycle, u64::MAX).is_err());
        for outcome in outcomes {
            let value = PolicyProfileCompletion {
                transaction: command.transaction,
                identity: command.identity,
                outcome,
            };
            let bytes = encode_wm_file_profile_completion(header(reply), value, u64::MAX).unwrap();
            assert_eq!(
                decode_wm_file_profile_completion(&bytes, reply, u64::MAX).unwrap(),
                value
            );
            // The submission id 9, server transaction 13 and event sequence
            // 11 are independent. Completion never substitutes one for another.
            assert_eq!(
                decode_wm_file_record(&bytes, WmFileClass::Candidate)
                    .unwrap()
                    .header
                    .submission_id,
                9
            );
            changed_length(bytes.clone(), |b| {
                decode_wm_file_profile_completion(b, reply, u64::MAX).map(|_| ())
            });
            assert!(decode_wm_file_profile_completion(&bytes, reply, 0).is_err());
            for offset in [80, 82, 83, 84, 85, 86, 87] {
                let mut bad = bytes.clone();
                bad[offset] = 255;
                assert!(decode_wm_file_profile_completion(&bad, reply, u64::MAX).is_err());
            }
        }
    }
}

#[test]
fn profile_kind_epoch_identity_and_digest_refusals_are_explicit() {
    let kind = WmFileKind::ProfilePrepare;
    let command = profile();
    let bytes = encode_wm_file_profile_command(header(kind), command, u64::MAX).unwrap();
    for range in [32..40, 40..48, 48..80] {
        let mut bad = bytes.clone();
        bad[range].fill(0);
        assert!(decode_wm_file_profile_command(&bad, kind, u64::MAX).is_err());
    }
    assert!(decode_wm_file_profile_command(&bytes, WmFileKind::ProfileActivate, u64::MAX).is_err());
    let mut h = header(kind);
    h.connection_epoch = 8;
    assert_eq!(
        encode_wm_file_profile_command(h, command, u64::MAX),
        Err(WmFilePayloadError::Identity)
    );
    let wrong = PolicyProfileCommand {
        transaction: TransactionId::INVALID,
        ..command
    };
    assert!(encode_wm_file_profile_command(header(kind), wrong, u64::MAX).is_err());
    let wrong = PolicyProfileCommand {
        identity: PolicyProfileIdentity {
            profile_digest: [0; 32],
            ..command.identity
        },
        ..command
    };
    assert!(encode_wm_file_profile_command(header(kind), wrong, u64::MAX).is_err());
}

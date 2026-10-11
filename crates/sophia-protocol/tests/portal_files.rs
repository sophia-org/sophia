//! Portal file rules the KDL states in prose rather than as field
//! constraints, and the envelope's malformed, truncated and stale handling.
use sophia_protocol::portal_files::*;
use sophia_protocol::{
    BinaryCodecError, NamespaceId, PortalBrokerResponseDecision, PortalBrokerResponsePacket,
    PortalDecision, PortalGrant, PortalGrantState, PortalRequest, PortalTransfer, PortalTransferId,
    PortalTransferKind,
};

const EPOCH: u64 = 9;

fn id(raw: u64) -> PortalTransferId {
    PortalTransferId::from_raw(raw)
}

fn request(mime_type: Option<&str>) -> PortalRequest {
    PortalRequest {
        transfer: PortalTransfer {
            transfer: id(41),
            source_namespace: NamespaceId::from_raw(2),
            target_namespace: NamespaceId::from_raw(3),
            kind: PortalTransferKind::Clipboard,
            mime_type: mime_type.map(str::to_owned),
            byte_size: 12,
            decision: PortalDecision::Pending,
            generation: 5,
        },
        deadline_msec: 9_000,
    }
}

fn grant(transfer: u64) -> PortalGrant {
    PortalGrant {
        transfer: id(transfer),
        source_namespace: NamespaceId::from_raw(2),
        target_namespace: NamespaceId::from_raw(3),
        kind: PortalTransferKind::Clipboard,
        source_generation: 5,
        broker_generation: 7,
        deadline_msec: 9_000,
        state: PortalGrantState::Active,
    }
}

fn candidates() -> Vec<PortalFileCandidate> {
    vec![
        PortalFileCandidate::Negotiate(PortalNegotiate {
            minimum_revision: 3,
            maximum_revision: 1,
        }),
        PortalFileCandidate::TransferRequest(request(Some("text/plain"))),
        PortalFileCandidate::TransferRequest(request(Some(""))),
        PortalFileCandidate::TransferRequest(request(None)),
        PortalFileCandidate::PayloadBegin(PortalPayloadBegin {
            transfer: id(41),
            total_bytes: 65_536,
        }),
        PortalFileCandidate::PayloadEnd(PortalPayloadEnd {
            transfer: id(41),
            total_bytes: 0,
        }),
        PortalFileCandidate::TransferCancel(id(41)),
    ]
}

fn events() -> Vec<PortalFileEvent> {
    vec![
        PortalFileEvent::Negotiated,
        PortalFileEvent::Refused,
        PortalFileEvent::Submitted(PortalSubmitted {
            submission_id: 5,
            candidate_kind: PortalFileKind::TransferRequest,
        }),
        PortalFileEvent::Decision(PortalBrokerResponsePacket {
            transfer: id(41),
            decision: PortalBrokerResponseDecision::Allowed(grant(41)),
        }),
        PortalFileEvent::Decision(PortalBrokerResponsePacket {
            transfer: id(0),
            decision: PortalBrokerResponseDecision::Denied,
        }),
        PortalFileEvent::TransferOutcome(PortalTransferOutcome {
            transfer: id(41),
            state: PortalGrantState::Completed,
        }),
    ]
}

fn invalid<T: std::fmt::Debug>(result: Result<T, PortalFileError>) -> bool {
    matches!(result, Err(PortalFileError::Invalid(_)))
}

#[test]
fn every_record_round_trips_and_names_its_kind() {
    for candidate in candidates() {
        let bytes = encode_portal_file_candidate(EPOCH, 5, &candidate).unwrap();
        assert_eq!(bytes.len(), candidate.kind().record_bytes());
        let (header, decoded) = decode_portal_file_candidate(&bytes, EPOCH).unwrap();
        assert_eq!(decoded, candidate);
        assert_eq!(header.kind, candidate.kind());
        assert_eq!((header.submission_id, header.sequence), (5, 0));
    }
    for event in events() {
        let bytes = encode_portal_file_event(EPOCH, 3, &event).unwrap();
        let (header, decoded) = decode_portal_file_event(&bytes, EPOCH).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(header.kind, event.kind());
        assert_eq!((header.submission_id, header.sequence), (0, 3));
    }
    let limits = PortalFileLimits::defaults(7);
    let bytes = encode_portal_file_limits(EPOCH, &limits).unwrap();
    assert_eq!(bytes.len(), PORTAL_FILE_MAX_OBJECT_BYTES);
    assert_eq!(decode_portal_file_limits(&bytes, EPOCH), Ok(limits));
}

#[test]
fn a_well_formed_record_of_another_epoch_is_stale_not_malformed() {
    let candidate =
        encode_portal_file_candidate(EPOCH, 5, &PortalFileCandidate::TransferCancel(id(41)))
            .unwrap();
    assert_eq!(
        decode_portal_file_candidate(&candidate, EPOCH + 1),
        Err(PortalFileError::StaleEpoch)
    );
    let event = encode_portal_file_event(EPOCH, 3, &PortalFileEvent::Refused).unwrap();
    assert_eq!(
        decode_portal_file_event(&event, 1),
        Err(PortalFileError::StaleEpoch)
    );
    let limits = encode_portal_file_limits(EPOCH, &PortalFileLimits::defaults(7)).unwrap();
    assert_eq!(
        decode_portal_file_limits(&limits, 1),
        Err(PortalFileError::StaleEpoch)
    );
    let submit = encode_portal_file_submit(PortalFileSubmit {
        connection_epoch: EPOCH,
        submission_id: 5,
        candidate_bytes: 40,
    })
    .unwrap();
    assert_eq!(
        decode_portal_file_submit(&submit, 1),
        Err(PortalFileError::StaleEpoch)
    );
    let ack = encode_portal_file_ack(PortalFileAck {
        connection_epoch: EPOCH,
        sequence: 3,
    })
    .unwrap();
    assert_eq!(
        decode_portal_file_ack(&ack, 1),
        Err(PortalFileError::StaleEpoch)
    );
    // A stale record's body is never interpreted.
    let mut corrupt = candidate.clone();
    corrupt[32..40].fill(0);
    assert!(invalid(decode_portal_file_candidate(&corrupt, EPOCH)));
    assert_eq!(
        decode_portal_file_candidate(&corrupt, EPOCH + 1),
        Err(PortalFileError::StaleEpoch)
    );
    // A malformed envelope is EINVAL even under another epoch.
    let mut short = candidate;
    short[0] = 39;
    assert!(invalid(decode_portal_file_candidate(&short, EPOCH + 1)));
}

#[test]
fn truncated_extended_and_misclassified_records_are_refused() {
    let mut records = Vec::new();
    for candidate in candidates() {
        records.push(encode_portal_file_candidate(EPOCH, 5, &candidate).unwrap());
    }
    for event in events() {
        records.push(encode_portal_file_event(EPOCH, 3, &event).unwrap());
    }
    records.push(encode_portal_file_limits(EPOCH, &PortalFileLimits::defaults(7)).unwrap());
    let decode_any = |bytes: &[u8]| {
        decode_portal_file_candidate(bytes, EPOCH).is_ok()
            || decode_portal_file_event(bytes, EPOCH).is_ok()
            || decode_portal_file_limits(bytes, EPOCH).is_ok()
    };
    for record in &records {
        assert!(decode_any(record));
        for len in 0..record.len() {
            assert!(!decode_any(&record[..len]), "prefix {len}");
            // A truncated record whose length field claims the cut is still
            // shorter than its kind.
            let mut cut = record[..len.max(32)].to_vec();
            if len >= 32 {
                cut[..4].copy_from_slice(&(len as u32).to_le_bytes());
                assert!(!decode_any(&cut), "relabelled prefix {len}");
            }
        }
        let mut longer = record.clone();
        longer.push(0);
        assert!(!decode_any(&longer));
        longer[..4].copy_from_slice(&(record.len() as u32 + 1).to_le_bytes());
        assert!(!decode_any(&longer));
    }
    // Each class is read only by its own decoder.
    let event = encode_portal_file_event(EPOCH, 3, &PortalFileEvent::Refused).unwrap();
    assert!(invalid(decode_portal_file_candidate(&event, EPOCH)));
    let candidate =
        encode_portal_file_candidate(EPOCH, 5, &PortalFileCandidate::TransferCancel(id(1)))
            .unwrap();
    assert!(invalid(decode_portal_file_event(&candidate, EPOCH)));
    assert!(invalid(decode_portal_file_limits(&candidate, EPOCH)));
    // Unknown kinds are malformed.
    let mut unknown = candidate;
    unknown[6..8].copy_from_slice(&261u16.to_le_bytes());
    assert_eq!(
        decode_portal_file_candidate(&unknown, EPOCH),
        Err(PortalFileError::Invalid(
            BinaryCodecError::UnknownMessageKind(261)
        ))
    );
    let wrong_sizes: [&[u8]; 2] = [&[0; 23], &[0; 25]];
    for bytes in wrong_sizes {
        assert!(invalid(decode_portal_file_submit(bytes, EPOCH)));
    }
    assert!(invalid(decode_portal_file_ack(&[0; 17], EPOCH)));
}

fn request_bytes(mime: Option<&str>) -> Vec<u8> {
    encode_portal_transfer_request(&request(mime)).unwrap()
}

#[test]
fn mime_distinguishes_absent_from_empty_and_has_one_encoding() {
    let absent = request_bytes(None);
    let empty = request_bytes(Some(""));
    assert_ne!(absent, empty);
    assert_eq!(
        decode_portal_transfer_request(&empty)
            .unwrap()
            .transfer
            .mime_type,
        Some(String::new())
    );
    assert_eq!(
        decode_portal_transfer_request(&absent)
            .unwrap()
            .transfer
            .mime_type,
        None
    );
    let longest = "m".repeat(255);
    assert!(decode_portal_transfer_request(&request_bytes(Some(&longest))).is_ok());
    assert!(encode_portal_transfer_request(&request(Some(&"m".repeat(256)))).is_err());
    // Absent MIME carries no length and no bytes.
    let mut bytes = absent.clone();
    bytes[28] = 1;
    assert!(decode_portal_transfer_request(&bytes).is_err());
    let mut bytes = absent;
    bytes[56] = b'x';
    assert!(decode_portal_transfer_request(&bytes).is_err());
    // Padding after the text is zero.
    let mut bytes = request_bytes(Some("text"));
    bytes[56 + 4] = b'x';
    assert!(decode_portal_transfer_request(&bytes).is_err());
    // The text is UTF-8.
    let mut bytes = request_bytes(Some("text"));
    bytes[56] = 0xff;
    assert_eq!(
        decode_portal_transfer_request(&bytes),
        Err(BinaryCodecError::InvalidUtf8 { field: "mime" })
    );
}

#[test]
fn a_request_is_always_pending_and_zero_identities_reach_the_lifecycle() {
    for decision in [
        PortalDecision::Allowed,
        PortalDecision::Denied,
        PortalDecision::Revoked,
    ] {
        let mut value = request(None);
        value.transfer.decision = decision;
        assert!(
            encode_portal_transfer_request(&value).is_err(),
            "{decision:?}"
        );
    }
    // Zero transfer, generation and deadline are the lifecycle's to deny.
    let mut value = request(None);
    value.transfer.transfer = id(0);
    value.transfer.generation = 0;
    value.deadline_msec = 0;
    let decoded =
        decode_portal_transfer_request(&encode_portal_transfer_request(&value).unwrap()).unwrap();
    assert_eq!(decoded, value);
    // Namespaces are nonzero syntax; distinctness is admission's check.
    let mut value = request(None);
    value.transfer.source_namespace = NamespaceId::from_raw(0);
    assert!(encode_portal_transfer_request(&value).is_err());
    let mut value = request(None);
    value.transfer.target_namespace = value.transfer.source_namespace;
    assert!(encode_portal_transfer_request(&value).is_ok());
    // Every transfer kind remains representable.
    for kind in [
        PortalTransferKind::Clipboard,
        PortalTransferKind::DragAndDrop,
        PortalTransferKind::FileHandoff,
        PortalTransferKind::ScreenCapture,
        PortalTransferKind::ScreenRecording,
        PortalTransferKind::UriOpen,
        PortalTransferKind::Notification,
    ] {
        let mut value = request(None);
        value.transfer.kind = kind;
        let bytes = encode_portal_transfer_request(&value).unwrap();
        assert_eq!(
            decode_portal_transfer_request(&bytes)
                .unwrap()
                .transfer
                .kind,
            kind
        );
    }
}

#[test]
fn denial_discloses_only_the_transfer_and_allowance_needs_a_valid_grant() {
    let denied = |transfer| PortalBrokerResponsePacket {
        transfer: id(transfer),
        decision: PortalBrokerResponseDecision::Denied,
    };
    for transfer in [0, 41] {
        let bytes = encode_portal_decision(&denied(transfer)).unwrap();
        assert_eq!(decode_portal_decision(&bytes), Ok(denied(transfer)));
        // Any other nonzero byte is a disclosure.
        for at in 9..56 {
            let mut bytes = bytes.clone();
            bytes[at] = 1;
            assert!(decode_portal_decision(&bytes).is_err(), "byte {at}");
        }
    }
    let allowed = |transfer, grant| PortalBrokerResponsePacket {
        transfer: id(transfer),
        decision: PortalBrokerResponseDecision::Allowed(grant),
    };
    assert!(encode_portal_decision(&allowed(41, grant(41))).is_ok());
    assert!(encode_portal_decision(&allowed(0, grant(0))).is_err());
    assert!(encode_portal_decision(&allowed(41, grant(42))).is_err());
    for state in [
        PortalGrantState::Completed,
        PortalGrantState::Revoked,
        PortalGrantState::Expired,
    ] {
        let mut value = grant(41);
        value.state = state;
        assert!(
            encode_portal_decision(&allowed(41, value)).is_err(),
            "{state:?}"
        );
    }
    let mut value = grant(41);
    value.target_namespace = NamespaceId::from_raw(0);
    assert!(encode_portal_decision(&allowed(41, value)).is_err());
    // An allowed decision cannot carry a zero transfer on the wire either.
    let mut bytes = encode_portal_decision(&allowed(41, grant(41))).unwrap();
    bytes[..8].fill(0);
    assert!(decode_portal_decision(&bytes).is_err());
    // Kind zero is only the denial's.
    let mut bytes = encode_portal_decision(&allowed(41, grant(41))).unwrap();
    bytes[10..12].fill(0);
    assert!(decode_portal_decision(&bytes).is_err());
}

#[test]
fn bodies_hold_their_prose_rules() {
    let mut limits = PortalFileLimits::defaults(7);
    assert_eq!(limits.validate(), Ok(()));
    limits.max_transfers = 8;
    limits.max_retained_transfers = 7;
    assert!(limits.validate().is_err());
    assert!(limits.encode().is_err());
    limits.max_retained_transfers = 8;
    assert!(limits.encode().is_ok());
    assert!(PortalFileLimits::defaults(0).validate().is_err());

    assert!(
        PortalSubmitted {
            submission_id: 5,
            candidate_kind: PortalFileKind::Decision,
        }
        .encode()
        .is_err()
    );
    let mut bytes = PortalSubmitted {
        submission_id: 5,
        candidate_kind: PortalFileKind::Negotiate,
    }
    .encode()
    .unwrap();
    bytes[8..10].copy_from_slice(&(PortalFileKind::Limits as u16).to_le_bytes());
    assert!(PortalSubmitted::decode(&bytes).is_err());

    assert!(
        PortalTransferOutcome {
            transfer: id(41),
            state: PortalGrantState::Active,
        }
        .encode()
        .is_err()
    );
    assert!(
        PortalPayloadBegin {
            transfer: id(41),
            total_bytes: PORTAL_FILE_MAX_PAYLOAD_BYTES + 1,
        }
        .encode()
        .is_err()
    );
    assert!(encode_portal_transfer_cancel(id(0)).is_err());

    // Negotiated confirms its own header's epoch.
    let mut bytes = encode_portal_file_event(EPOCH, 3, &PortalFileEvent::Negotiated).unwrap();
    bytes[40..48].copy_from_slice(&(EPOCH + 1).to_le_bytes());
    assert!(invalid(decode_portal_file_event(&bytes, EPOCH)));

    assert_eq!(
        portal_file_api_line(EPOCH),
        "sophia-portal-files version=1 role=portal-requester epoch=9 fd_transfer=none\n"
    );
}

#[test]
fn controls_hold_their_identity_rules() {
    let submit = |connection_epoch, submission_id, candidate_bytes| PortalFileSubmit {
        connection_epoch,
        submission_id,
        candidate_bytes,
    };
    assert!(encode_portal_file_submit(submit(EPOCH, 5, 40)).is_ok());
    for bad in [submit(0, 5, 40), submit(EPOCH, 0, 40), submit(EPOCH, 5, 39)] {
        assert!(encode_portal_file_submit(bad).is_err(), "{bad:?}");
    }
    // A zero epoch is malformed, never merely stale.
    let mut bytes = encode_portal_file_submit(submit(EPOCH, 5, 40)).unwrap();
    bytes[..8].fill(0);
    assert!(invalid(decode_portal_file_submit(&bytes, EPOCH)));
    assert!(
        encode_portal_file_ack(PortalFileAck {
            connection_epoch: EPOCH,
            sequence: 0,
        })
        .is_err()
    );
    // Records refuse an identity the class forbids.
    assert!(
        encode_portal_file_candidate(EPOCH, 0, &PortalFileCandidate::TransferCancel(id(1)))
            .is_err()
    );
    assert!(encode_portal_file_event(EPOCH, 0, &PortalFileEvent::Refused).is_err());
    assert!(encode_portal_file_limits(0, &PortalFileLimits::defaults(7)).is_err());
}

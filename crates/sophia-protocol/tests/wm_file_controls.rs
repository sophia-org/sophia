use sophia_protocol::wm_files::*;
use sophia_protocol::*;

#[path = "support/policy_scalar_fixture.rs"]
mod fixture;

fn header(kind: WmFileKind) -> WmFileHeader {
    WmFileHeader {
        kind,
        connection_epoch: 3,
        submission_id: if wm_file_class(kind) == WmFileClass::Candidate {
            101
        } else {
            0
        },
        sequence: if wm_file_class(kind) == WmFileClass::Event {
            203
        } else {
            0
        },
    }
}

fn cycle(request: PolicyProjectionRequest) -> WmFileCycle {
    WmFileCycle {
        snapshot_transaction: TransactionId::from_raw(7),
        request_transaction: TransactionId::from_raw(9),
        request,
    }
}

fn repair_length(bytes: &mut [u8]) {
    let len = u32::try_from(bytes.len()).unwrap();
    bytes[..4].copy_from_slice(&len.to_le_bytes());
}

#[test]
fn every_cause_uses_its_own_complete_body_and_selected_capabilities() {
    let requests = fixture::ordinary_causes()
        .into_iter()
        .map(fixture::request)
        .chain([
            fixture::output_action(),
            fixture::presentation_action(false),
            fixture::presentation_action(true),
        ]);
    for request in requests {
        let value = cycle(request);
        let h = header(WmFileKind::Cycle);
        let bytes = encode_wm_file_cycle(h, &value, u64::MAX).unwrap();
        assert_eq!(decode_wm_file_cycle(&bytes, u64::MAX).unwrap(), value);
        let caps = policy_request_cause_capabilities(&value.request.cause);
        for bit in (0..64).map(|i| 1u64 << i).filter(|bit| caps & bit != 0) {
            assert_eq!(
                decode_wm_file_cycle(&bytes, !bit),
                Err(WmFilePayloadError::Capabilities { missing: bit })
            );
            assert!(encode_wm_file_cycle(h, &value, !bit).is_err());
        }
        for end in 32..bytes.len() {
            let mut short = bytes[..end].to_vec();
            repair_length(&mut short);
            assert!(
                decode_wm_file_cycle(&short, u64::MAX).is_err(),
                "cause {:?} cut {end}",
                value.request.cause
            );
        }
        let mut long = bytes.clone();
        long.push(0);
        repair_length(&mut long);
        assert!(decode_wm_file_cycle(&long, u64::MAX).is_err());
    }
}

#[test]
fn cycle_offsets_and_domain_identities_are_independent_of_event_sequence() {
    let value = cycle(fixture::request(PolicyRequestCause::SceneChanged));
    let bytes = encode_wm_file_cycle(header(WmFileKind::Cycle), &value, 0).unwrap();
    assert_eq!(bytes.len(), 96);
    assert_eq!(
        &bytes[32..72],
        &[
            7, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0,
            0, 0, 0, 0, 13, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
    assert_eq!(
        &bytes[72..96],
        &[
            0, 0, 2, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0
        ]
    );
    for at in [32, 40, 48, 56, 64, 80, 88] {
        let mut bad = bytes.clone();
        bad[at..at + 8].fill(0);
        assert!(decode_wm_file_cycle(&bad, 0).is_err(), "identity {at}");
    }
    for at in [72, 76, 77, 78, 79] {
        let mut bad = bytes.clone();
        bad[at] = 255;
        assert!(decode_wm_file_cycle(&bad, u64::MAX).is_err());
    }
    let mut bad = bytes.clone();
    bad[74..76].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(decode_wm_file_cycle(&bad, 0).is_err());
    let mut bad = bytes.clone();
    bad[88..96].copy_from_slice(&1u64.to_le_bytes());
    assert!(decode_wm_file_cycle(&bad, 0).is_err());
}

#[test]
fn file_pointer_and_interaction_codes_are_not_legacy_cause_codes() {
    let pointer = cycle(fixture::request(PolicyRequestCause::PointerFocus {
        output: OutputId::from_raw(2),
        target: None,
    }));
    let bytes = encode_wm_file_cycle(header(WmFileKind::Cycle), &pointer, u64::MAX).unwrap();
    assert_eq!(&bytes[72..74], &[3, 0]);
    assert_eq!(
        &bytes[96..112],
        &[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    let interaction = cycle(fixture::request(PolicyRequestCause::Interaction {
        phase: PolicyInteractionPhase::Update,
        kind: PolicyInteractionKind::Resize,
        axis: PolicyInteractionAxis::None,
        target: SurfaceId::new(8, 3),
        geometry: Rect {
            x: -5,
            y: 7,
            width: 640,
            height: 480,
        },
    }));
    let mut bytes =
        encode_wm_file_cycle(header(WmFileKind::Cycle), &interaction, u64::MAX).unwrap();
    assert_eq!(&bytes[72..74], &[4, 0]);
    assert_eq!(
        &bytes[96..128],
        &[
            2, 0, 2, 0, 0, 0, 0, 0, 8, 0, 0, 0, 3, 0, 0, 0, 251, 255, 255, 255, 7, 0, 0, 0, 128, 2,
            0, 0, 224, 1, 0, 0,
        ]
    );
    bytes[120..124].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(decode_wm_file_cycle(&bytes, u64::MAX).is_err());
}

#[test]
fn new_file_targets_are_strict_despite_the_preserved_legacy_exception() {
    let h = header(WmFileKind::Cycle);
    let value = cycle(fixture::request(PolicyRequestCause::Focus {
        target: SurfaceId::new(4, 1),
    }));
    let mut bytes = encode_wm_file_cycle(h, &value, 0).unwrap();
    bytes[96..100].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_wm_file_cycle(&bytes, 0).is_err());
    let bad = cycle(fixture::request(PolicyRequestCause::Focus {
        target: SurfaceId::new(u32::MAX, 1),
    }));
    assert!(encode_wm_file_cycle(h, &bad, 0).is_err());

    let h = header(WmFileKind::SessionOperation);
    for target in [None, Some(SurfaceId::new(0, 1)), Some(SurfaceId::new(9, 4))] {
        let value = WmFileSessionOperation {
            transaction: TransactionId::from_raw(5),
            request: fixture::session_operation(target),
        };
        let bytes = encode_wm_file_session_operation(h, &value, u64::MAX).unwrap();
        assert_eq!(
            decode_wm_file_session_operation(&bytes, u64::MAX).unwrap(),
            value
        );
        assert!(decode_wm_file_session_operation(&bytes, 0).is_err());
        assert!(encode_wm_file_session_operation(h, &value, 0).is_err());
    }
    for target in [SurfaceId::new(5, 0), SurfaceId::new(u32::MAX, 1)] {
        let value = WmFileSessionOperation {
            transaction: TransactionId::from_raw(5),
            request: fixture::session_operation(Some(target)),
        };
        assert!(encode_wm_file_session_operation(h, &value, u64::MAX).is_err());
        let good = WmFileSessionOperation {
            request: fixture::session_operation(None),
            ..value
        };
        let mut bytes = encode_wm_file_session_operation(h, &good, u64::MAX).unwrap();
        bytes[56..60].copy_from_slice(&target.index().to_le_bytes());
        bytes[60..64].copy_from_slice(&target.generation().to_le_bytes());
        assert!(decode_wm_file_session_operation(&bytes, u64::MAX).is_err());
    }
}

#[test]
fn dirty_requires_complete_unique_outputs_and_its_selected_capability() {
    let value = fixture::dirty();
    let h = header(WmFileKind::Dirty);
    let bytes = encode_wm_file_dirty(h, &value, u64::MAX).unwrap();
    assert_eq!(decode_wm_file_dirty(&bytes, u64::MAX).unwrap(), value);
    assert_eq!(
        decode_wm_file_dirty(&bytes, 0),
        Err(WmFilePayloadError::Capabilities {
            missing: SOPHIA_WM_CAPABILITY_POLICY_DIRTY
        })
    );
    for at in [32, 42, 43, 44, 45, 46, 47] {
        let mut bad = bytes.clone();
        bad[at] = if at == 32 { 0 } else { 1 };
        assert!(decode_wm_file_dirty(&bad, u64::MAX).is_err());
    }
    let mut bad = value.clone();
    bad.affected_outputs.push(bad.affected_outputs[0]);
    assert!(encode_wm_file_dirty(h, &bad, u64::MAX).is_err());
    let mut bad = bytes.clone();
    bad[40..42].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(decode_wm_file_dirty(&bad, u64::MAX).is_err());
}

#[test]
fn every_outcome_retains_its_domain_correlation_and_only_valid_codes() {
    for outcome in fixture::OUTCOMES {
        let config = WmFileConfigurationOutcome {
            transaction: TransactionId::from_raw(7),
            generation: 11,
            outcome,
        };
        let bytes = encode_wm_file_configuration_outcome(
            header(WmFileKind::ConfigurationOutcome),
            &config,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(
            decode_wm_file_configuration_outcome(&bytes, u64::MAX).unwrap(),
            config
        );
        assert!(decode_wm_file_configuration_outcome(&bytes, 0).is_err());
        let operation = WmFileSessionOperationOutcome {
            transaction: TransactionId::from_raw(9),
            outcome: PolicySessionOperationOutcome {
                connection_epoch: 3,
                request_id: 13,
                outcome,
            },
        };
        let bytes = encode_wm_file_session_operation_outcome(
            header(WmFileKind::SessionOperationOutcome),
            &operation,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(
            decode_wm_file_session_operation_outcome(&bytes, u64::MAX).unwrap(),
            operation
        );
        assert!(decode_wm_file_session_operation_outcome(&bytes, 0).is_err());
        for expect_session_operation in [false, true] {
            let projection = WmFileProjectionOutcome {
                transaction: TransactionId::from_raw(15),
                request_id: 17,
                scene_generation: 19,
                outcome,
                expect_session_operation,
            };
            let h = header(WmFileKind::ProjectionOutcome);
            let bytes = encode_wm_file_projection_outcome(h, &projection, u64::MAX).unwrap();
            assert_eq!(
                decode_wm_file_projection_outcome(&bytes, u64::MAX).unwrap(),
                projection
            );
            assert_eq!(
                decode_wm_file_projection_outcome(&bytes, 0).is_err(),
                expect_session_operation
            );
            for at in [32, 40, 48, 56, 58, 60, 61, 62, 63] {
                let mut bad = bytes.clone();
                bad[at] = if at < 56 { 0 } else { 255 };
                assert!(
                    decode_wm_file_projection_outcome(&bad, u64::MAX).is_err(),
                    "field {at}"
                );
            }
        }
    }
}

#[test]
fn receipts_do_not_lose_the_output_and_presentation_epoch() {
    for outcome in fixture::PRESENTATION_OUTCOMES {
        let value = WmFilePresentationReceipt {
            transaction: TransactionId::from_raw(7),
            receipt: fixture::receipt(outcome),
        };
        let bytes = encode_wm_file_presentation_receipt(
            header(WmFileKind::PresentationReceipt),
            &value,
            u64::MAX,
        )
        .unwrap();
        assert_eq!(
            decode_wm_file_presentation_receipt(&bytes, u64::MAX).unwrap(),
            value
        );
        assert!(decode_wm_file_presentation_receipt(&bytes, 0).is_err());
        for at in [32, 40, 48, 56, 64, 72, 74, 75, 76, 77, 78, 79] {
            let mut bad = bytes.clone();
            bad[at] = if at < 72 { 0 } else { 255 };
            assert!(decode_wm_file_presentation_receipt(&bad, u64::MAX).is_err());
        }
    }
}

#[test]
fn submitted_only_reports_candidate_custody_not_policy_outcome() {
    let value = WmFileSubmitted {
        submission_id: 101,
        candidate_kind: WmFileKind::Projection,
    };
    let h = header(WmFileKind::Submitted);
    let bytes = encode_wm_file_submitted(h, value).unwrap();
    assert_eq!(encode_wm_file_submitted_body(value).unwrap(), bytes[32..]);
    assert_eq!(
        &bytes[32..],
        &[101, 0, 0, 0, 0, 0, 0, 0, 6, 1, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(decode_wm_file_submitted(&bytes).unwrap(), value);
    assert_eq!(
        decode_wm_file_record(&bytes, WmFileClass::Event)
            .unwrap()
            .header
            .sequence,
        203
    );
    assert!(
        encode_wm_file_submitted(
            h,
            WmFileSubmitted {
                candidate_kind: WmFileKind::ProjectionOutcome,
                ..value
            }
        )
        .is_err()
    );
    for at in [32, 40, 42, 43, 44, 45, 46, 47] {
        let mut bad = bytes.clone();
        bad[at] = if at == 32 { 0 } else { 255 };
        assert!(decode_wm_file_submitted(&bad).is_err());
    }
}

fn refuses_changed_length(
    bytes: Vec<u8>,
    decode: impl Fn(&[u8]) -> Result<(), WmFilePayloadError>,
) {
    assert_eq!(decode(&bytes), Ok(()));
    for end in 32..bytes.len() {
        let mut short = bytes[..end].to_vec();
        repair_length(&mut short);
        assert!(decode(&short).is_err(), "truncated at {end}");
    }
    let mut long = bytes;
    long.push(0);
    repair_length(&mut long);
    assert_eq!(
        decode(&long),
        Err(WmFilePayloadError::Envelope(WmFileCodecError::Length))
    );
}

#[test]
fn fixed_control_bodies_refuse_both_truncation_and_extra_zero_tails() {
    let transaction = TransactionId::from_raw(7);
    let outcome = PolicyProjectionOutcome::Committed;
    refuses_changed_length(
        encode_wm_file_configuration_outcome(
            header(WmFileKind::ConfigurationOutcome),
            &WmFileConfigurationOutcome {
                transaction,
                generation: 11,
                outcome,
            },
            u64::MAX,
        )
        .unwrap(),
        |b| decode_wm_file_configuration_outcome(b, u64::MAX).map(|_| ()),
    );
    refuses_changed_length(
        encode_wm_file_projection_outcome(
            header(WmFileKind::ProjectionOutcome),
            &WmFileProjectionOutcome {
                transaction,
                request_id: 13,
                scene_generation: 17,
                outcome,
                expect_session_operation: true,
            },
            u64::MAX,
        )
        .unwrap(),
        |b| decode_wm_file_projection_outcome(b, u64::MAX).map(|_| ()),
    );
    refuses_changed_length(
        encode_wm_file_session_operation_outcome(
            header(WmFileKind::SessionOperationOutcome),
            &WmFileSessionOperationOutcome {
                transaction,
                outcome: PolicySessionOperationOutcome {
                    connection_epoch: 3,
                    request_id: 19,
                    outcome,
                },
            },
            u64::MAX,
        )
        .unwrap(),
        |b| decode_wm_file_session_operation_outcome(b, u64::MAX).map(|_| ()),
    );
    refuses_changed_length(
        encode_wm_file_session_operation(
            header(WmFileKind::SessionOperation),
            &WmFileSessionOperation {
                transaction,
                request: fixture::session_operation(None),
            },
            u64::MAX,
        )
        .unwrap(),
        |b| decode_wm_file_session_operation(b, u64::MAX).map(|_| ()),
    );
    refuses_changed_length(
        encode_wm_file_presentation_receipt(
            header(WmFileKind::PresentationReceipt),
            &WmFilePresentationReceipt {
                transaction,
                receipt: fixture::receipt(PolicyPresentationOutcome::Presented),
            },
            u64::MAX,
        )
        .unwrap(),
        |b| decode_wm_file_presentation_receipt(b, u64::MAX).map(|_| ()),
    );
    refuses_changed_length(
        encode_wm_file_submitted(
            header(WmFileKind::Submitted),
            WmFileSubmitted {
                submission_id: 101,
                candidate_kind: WmFileKind::Dirty,
            },
        )
        .unwrap(),
        decode_wm_file_submitted_for_length,
    );
}

fn decode_wm_file_submitted_for_length(bytes: &[u8]) -> Result<(), WmFilePayloadError> {
    decode_wm_file_submitted(bytes).map(|_| ())
}

#[test]
fn scalar_file_encoders_refuse_epoch_mismatch_and_wrong_direction() {
    let request = cycle(fixture::request(PolicyRequestCause::SceneChanged));
    let mut h = header(WmFileKind::Cycle);
    h.connection_epoch += 1;
    assert!(encode_wm_file_cycle(h, &request, u64::MAX).is_err());
    let dirty = fixture::dirty();
    let mut h = header(WmFileKind::Dirty);
    h.connection_epoch += 1;
    assert!(encode_wm_file_dirty(h, &dirty, u64::MAX).is_err());
    let bytes = encode_wm_file_dirty(header(WmFileKind::Dirty), &dirty, u64::MAX).unwrap();
    assert!(decode_wm_file_session_operation(&bytes, u64::MAX).is_err());
    assert!(decode_wm_file_cycle(&bytes, u64::MAX).is_err());
    let h = header(WmFileKind::SessionOperation);
    assert!(encode_wm_file_dirty(h, &dirty, u64::MAX).is_err());
}

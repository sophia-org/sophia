use sophia_protocol::*;

const VALID_CORPUS: &str = include_str!("../../../protocol/golden/sophia-wm-v1.frames");
const MALFORMED_CORPUS: &str =
    include_str!("../../../protocol/golden/sophia-wm-v1-malformed.frames");
const RECORD_CORPUS: &str = include_str!("../../../protocol/golden/sophia-wm-v1.records");

#[test]
fn tab_extension_ordinals_cannot_wrap() {
    let member = SurfaceId::new(1, 1);
    let group = PolicyTabGroup {
        output: OutputId::from_raw(1),
        group: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 24,
        },
        focused: true,
        selected: Some(member),
        members: vec![member],
    };
    assert!(encode_wm_tab_groups(&[group], 1, u16::MAX).is_err());
}

#[test]
fn generated_rust_codec_matches_every_golden_frame() {
    for line in corpus_lines(VALID_CORPUS) {
        let mut fields = line.split('|');
        let name = fields.next().unwrap();
        let transaction = fields.next().unwrap().parse::<u64>().unwrap();
        let frame = decode_hex(fields.next().unwrap());
        assert!(fields.next().is_none(), "invalid corpus row: {line}");

        let encoded = roundtrip(name, transaction, &frame);
        assert_eq!(encoded, frame, "golden mismatch for {name}");
    }
}

#[test]
fn generated_rust_codec_rejects_the_shared_malformed_corpus() {
    for line in corpus_lines(MALFORMED_CORPUS) {
        let mut fields = line.split('|');
        let case = fields.next().unwrap();
        let decoder = fields.next().unwrap();
        let expected = fields.next().unwrap();
        let frame = decode_hex(fields.next().unwrap());
        assert!(fields.next().is_none(), "invalid corpus row: {line}");

        let error = match decoder {
            "client_hello" => decode_wm_v1_client_hello_frame(&frame).unwrap_err(),
            "server_welcome" => decode_wm_v1_server_welcome_frame(&frame).unwrap_err(),
            "snapshot_begin" => decode_wm_v1_snapshot_begin_frame(&frame).unwrap_err(),
            "snapshot_chunk" => decode_wm_v1_snapshot_chunk_frame(&frame).unwrap_err(),
            other => panic!("unknown decoder `{other}`"),
        };
        assert_eq!(
            error_name(&error),
            expected,
            "wrong error for {case}: {error:?}"
        );
    }
}

#[test]
fn generated_rust_record_codec_matches_every_golden_record() {
    for line in corpus_lines(RECORD_CORPUS) {
        let mut fields = line.split('|');
        let name = fields.next().unwrap();
        let data = decode_hex(fields.next().unwrap());
        assert!(fields.next().is_none(), "invalid corpus row: {line}");
        let encoded = match name {
            "snapshot_output" => encode_wm_v1_snapshot_output_records(
                &decode_wm_v1_snapshot_output_records(&data, 1).unwrap(),
            )
            .unwrap(),
            "snapshot_surface" => encode_wm_v1_snapshot_surface_records(
                &decode_wm_v1_snapshot_surface_records(&data, 1).unwrap(),
            )
            .unwrap(),
            "snapshot_action" => encode_wm_v1_snapshot_action_records(
                &decode_wm_v1_snapshot_action_records(&data, 1).unwrap(),
            )
            .unwrap(),
            "snapshot_session_operation" => encode_wm_v1_snapshot_session_operation_records(
                &decode_wm_v1_snapshot_session_operation_records(&data, 1).unwrap(),
            )
            .unwrap(),
            "projection_output" => encode_wm_v1_projection_output_records(
                &decode_wm_v1_projection_output_records(&data, 1).unwrap(),
            )
            .unwrap(),
            "projection_placement" => encode_wm_v1_projection_placement_records(
                &decode_wm_v1_projection_placement_records(&data, 1).unwrap(),
            )
            .unwrap(),
            "projection_indicator" => encode_wm_v1_projection_indicator_records(
                &decode_wm_v1_projection_indicator_records(&data, 1).unwrap(),
            )
            .unwrap(),
            "projection_output_status" => encode_wm_v1_projection_output_status_records(
                &decode_wm_v1_projection_output_status_records(&data, 1).unwrap(),
            )
            .unwrap(),
            // The extension record's codec is hand-written rather than
            // generated, and the corpus proves it against the same bytes an
            // independent client sees.
            "snapshot_surface_classification" => {
                encode_wm_v1_snapshot_surface_classification_records(
                    &decode_wm_v1_snapshot_surface_classification_records(&data, 1).unwrap(),
                )
                .unwrap()
            }
            "projection_tab_group" | "projection_tab_member" => {
                let find = |name: &str| {
                    RECORD_CORPUS
                        .lines()
                        .find_map(|line| line.strip_prefix(&format!("{name}|")))
                        .map(decode_hex)
                        .unwrap()
                };
                let chunks = vec![
                    WmV1ProjectionChunk {
                        connection_epoch: 1,
                        ordinal: 0,
                        record_kind: PROJECTION_TAB_GROUP_RECORD_KIND,
                        item_count: 1,
                        data: find("projection_tab_group"),
                    },
                    WmV1ProjectionChunk {
                        connection_epoch: 1,
                        ordinal: 1,
                        record_kind: PROJECTION_TAB_MEMBER_RECORD_KIND,
                        item_count: 1,
                        data: find("projection_tab_member"),
                    },
                ];
                let groups = decode_wm_tab_groups(&chunks).unwrap();
                assert_eq!(groups[0].group, 1);
                encode_wm_tab_groups(&groups, 1, 0).unwrap()
                    [usize::from(name == "projection_tab_member")]
                .data
                .clone()
            }
            "projection_translation_group" | "projection_translation_member" => {
                let find = |name: &str| {
                    RECORD_CORPUS
                        .lines()
                        .find_map(|line| line.strip_prefix(&format!("{name}|")))
                        .map(decode_hex)
                        .unwrap()
                };
                let chunks = vec![
                    WmV1ProjectionChunk {
                        connection_epoch: 1,
                        ordinal: 0,
                        record_kind: PROJECTION_TRANSLATION_GROUP_RECORD_KIND,
                        item_count: 1,
                        data: find("projection_translation_group"),
                    },
                    WmV1ProjectionChunk {
                        connection_epoch: 1,
                        ordinal: 1,
                        record_kind: PROJECTION_TRANSLATION_MEMBER_RECORD_KIND,
                        item_count: 1,
                        data: find("projection_translation_member"),
                    },
                ];
                let groups = decode_wm_translation_groups(&chunks).unwrap();
                assert_eq!(groups[0].group, 1);
                encode_wm_translation_groups(&groups, 1, 0).unwrap()
                    [usize::from(name == "projection_translation_member")]
                .data
                .clone()
            }
            "projection_output_launch_context" => {
                let chunks = [WmV1ProjectionChunk {
                    connection_epoch: 1,
                    ordinal: 0,
                    record_kind: PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
                    item_count: 1,
                    data: data.clone(),
                }];
                encode_wm_output_launch_contexts(
                    &decode_wm_output_launch_contexts(&chunks).unwrap(),
                    1,
                    0,
                )
                .unwrap()[0]
                    .data
                    .clone()
            }
            "projection_launch_context" | "snapshot_launch_origin" => {
                encode_wm_launch_context_records(
                    &decode_wm_launch_context_records(&data, 1).unwrap(),
                )
                .unwrap()
            }
            "snapshot_output_policy_key" => {
                assert_eq!(data.len(), 24);
                let mut encoded = Vec::new();
                for field in data.chunks_exact(8) {
                    let value = u64::from_le_bytes(field.try_into().unwrap());
                    assert_eq!(value, 1);
                    encoded.extend(value.to_le_bytes());
                }
                encoded
            }
            other => panic!("unknown record `{other}`"),
        };
        assert_eq!(encoded, data, "golden mismatch for {name}");
    }
}

#[test]
fn record_codec_rejects_reserved_and_trailing_data() {
    let line = corpus_lines(RECORD_CORPUS)
        .find(|line| line.starts_with("projection_output|"))
        .unwrap();
    let mut data = decode_hex(line.split('|').nth(1).unwrap());
    data[20] = 1;
    assert_eq!(
        decode_wm_v1_projection_output_records(&data, 1),
        Err(IpcCodecError::ReservedNonZero(1))
    );
    data[20] = 0;
    data.push(0);
    assert_eq!(
        decode_wm_v1_projection_output_records(&data, 1),
        Err(IpcCodecError::TrailingBytes(1))
    );
}

#[test]
fn chunk_payload_and_transaction_bounds_fail_before_encoding() {
    let transaction = TransactionId::from_raw(1);
    let message = WmV1ProjectionChunk {
        connection_epoch: 1,
        ordinal: 0,
        record_kind: 1,
        item_count: 1,
        data: vec![0; 65_521],
    };
    assert_eq!(
        encode_wm_v1_projection_chunk_frame(transaction, &message),
        Err(IpcCodecError::FieldTooLarge {
            field: "data",
            len: 65_521,
            max: 65_520,
        })
    );

    let valid = WmV1ProjectionEnd {
        connection_epoch: 1,
        request_id: 1,
        base_generation: 1,
        chunk_count: 0,
    };
    assert_eq!(
        encode_wm_v1_projection_end_frame(TransactionId::INVALID, &valid),
        Err(IpcCodecError::InvalidTransaction(0))
    );
}

fn roundtrip(name: &str, transaction: u64, frame: &[u8]) -> Vec<u8> {
    let expected_transaction = TransactionId::from_raw(transaction);
    match name {
        "client_hello" => {
            let message = decode_wm_v1_client_hello_frame(frame).unwrap();
            encode_wm_v1_client_hello_frame(&message).unwrap()
        }
        "server_welcome" => {
            let message = decode_wm_v1_server_welcome_frame(frame).unwrap();
            encode_wm_v1_server_welcome_frame(&message).unwrap()
        }
        "snapshot_begin" => {
            let (actual, message) = decode_wm_v1_snapshot_begin_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_snapshot_begin_frame(actual, &message).unwrap()
        }
        "snapshot_chunk" => {
            let (actual, message) = decode_wm_v1_snapshot_chunk_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_snapshot_chunk_frame(actual, &message).unwrap()
        }
        "snapshot_end" => {
            let (actual, message) = decode_wm_v1_snapshot_end_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_snapshot_end_frame(actual, &message).unwrap()
        }
        "output_action_request" => {
            let (actual, message) = decode_wm_v1_output_action_request_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_output_action_request_frame(actual, &message).unwrap()
        }
        "projection_request" => {
            let (actual, message) = decode_wm_v1_projection_request_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_projection_request_frame(actual, &message).unwrap()
        }
        "projection_begin" => {
            let (actual, message) = decode_wm_v1_projection_begin_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_projection_begin_frame(actual, &message).unwrap()
        }
        "projection_chunk" => {
            let (actual, message) = decode_wm_v1_projection_chunk_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_projection_chunk_frame(actual, &message).unwrap()
        }
        "projection_end" => {
            let (actual, message) = decode_wm_v1_projection_end_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_projection_end_frame(actual, &message).unwrap()
        }
        "projection_outcome" => {
            let (actual, message) = decode_wm_v1_projection_outcome_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_projection_outcome_frame(actual, &message).unwrap()
        }
        "policy_configuration" => {
            let (actual, message) = decode_wm_v1_policy_configuration_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_policy_configuration_frame(actual, &message).unwrap()
        }
        "policy_configuration_outcome" => {
            let (actual, message) = decode_wm_v1_policy_configuration_outcome_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_policy_configuration_outcome_frame(actual, &message).unwrap()
        }
        "policy_dirty" => {
            let (actual, message) = decode_wm_v1_policy_dirty_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_policy_dirty_frame(actual, &message).unwrap()
        }
        "session_operation_request" => {
            let (actual, message) = decode_wm_v1_session_operation_request_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_session_operation_request_frame(actual, &message).unwrap()
        }
        "session_operation_outcome" => {
            let (actual, message) = decode_wm_v1_session_operation_outcome_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_session_operation_outcome_frame(actual, &message).unwrap()
        }
        "profile_prepare" => {
            let (actual, message) = decode_wm_v1_profile_prepare_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_profile_prepare_frame(actual, &message).unwrap()
        }
        "profile_prepared" => {
            let (actual, message) = decode_wm_v1_profile_prepared_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_profile_prepared_frame(actual, &message).unwrap()
        }
        "profile_activate" => {
            let (actual, message) = decode_wm_v1_profile_activate_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_profile_activate_frame(actual, &message).unwrap()
        }
        "profile_active" => {
            let (actual, message) = decode_wm_v1_profile_active_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_profile_active_frame(actual, &message).unwrap()
        }
        "profile_rollback" => {
            let (actual, message) = decode_wm_v1_profile_rollback_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_profile_rollback_frame(actual, &message).unwrap()
        }
        "profile_rolled_back" => {
            let (actual, message) = decode_wm_v1_profile_rolled_back_frame(frame).unwrap();
            assert_eq!(actual, expected_transaction);
            encode_wm_v1_profile_rolled_back_frame(actual, &message).unwrap()
        }
        other => panic!("unknown golden message `{other}`"),
    }
}

fn corpus_lines(corpus: &str) -> impl Iterator<Item = &str> {
    corpus
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

fn decode_hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn error_name(error: &IpcCodecError) -> &'static str {
    match error {
        IpcCodecError::Truncated => "truncated",
        IpcCodecError::BadMagic => "bad_magic",
        IpcCodecError::UnsupportedVersion(_) => "unsupported_frame_version",
        IpcCodecError::UnknownMessageKind(_) => "wrong_message_kind",
        IpcCodecError::PayloadTooLarge(_) => "payload_too_large",
        IpcCodecError::ReservedNonZero(_) => "reserved_nonzero",
        IpcCodecError::TrailingBytes(_) => "trailing_bytes",
        IpcCodecError::InvalidTransaction(_) => "invalid_transaction",
        IpcCodecError::InvalidEnum {
            field: "message_kind",
            ..
        } => "wrong_message_kind",
        other => panic!("malformed corpus lacks an error name for {other:?}"),
    }
}

//! The golden WM record corpus (`protocol/golden/sophia-wm-v1.records`)
//! through the shared record codecs, without the socket codecs. The C SDK's
//! file tests read the same corpus, so it outlives the socket frames. The
//! legacy chunk wrappers' pass over it retired under t269 (`policy_wire.rs`
//! at 2eeb074e8).
use sophia_protocol::*;

const RECORD_CORPUS: &str = include_str!("../../../protocol/golden/sophia-wm-v1.records");

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

fn find(name: &str) -> Vec<u8> {
    RECORD_CORPUS
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name}|")))
        .map(decode_hex)
        .unwrap()
}

/// One section of one row per named record, in the given order.
fn sections(records: &[(&str, u16)]) -> Vec<PolicyRecordSection> {
    records
        .iter()
        .map(|(name, kind)| PolicyRecordSection {
            kind: *kind,
            count: 1,
            bytes: find(name),
        })
        .collect()
}

fn refs(sections: &[PolicyRecordSection]) -> Vec<PolicyRecordSectionRef<'_>> {
    sections.iter().map(PolicyRecordSection::as_ref).collect()
}

/// The neutral twin of `policy_wire.rs`'s
/// `generated_rust_record_codec_matches_every_golden_record`: every row
/// decodes and re-encodes to its exact golden bytes.
#[test]
fn shared_record_codecs_match_every_golden_record() {
    let mut rows = 0;
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
            "snapshot_surface_classification" => {
                encode_wm_v1_snapshot_surface_classification_records(
                    &decode_wm_v1_snapshot_surface_classification_records(&data, 1).unwrap(),
                )
                .unwrap()
            }
            "projection_tab_group" | "projection_tab_member" => {
                let input = sections(&[
                    ("projection_tab_group", PROJECTION_TAB_GROUP_RECORD_KIND),
                    ("projection_tab_member", PROJECTION_TAB_MEMBER_RECORD_KIND),
                ]);
                let groups = decode_policy_tab_groups_records(&refs(&input)).unwrap();
                assert_eq!(groups[0].group, 1);
                encode_policy_tab_groups_records(&groups).unwrap()
                    [usize::from(name == "projection_tab_member")]
                .bytes
                .clone()
            }
            "projection_translation_group" | "projection_translation_member" => {
                let input = sections(&[
                    (
                        "projection_translation_group",
                        PROJECTION_TRANSLATION_GROUP_RECORD_KIND,
                    ),
                    (
                        "projection_translation_member",
                        PROJECTION_TRANSLATION_MEMBER_RECORD_KIND,
                    ),
                ]);
                let groups = decode_policy_translation_groups_records(&refs(&input)).unwrap();
                assert_eq!(groups[0].group, 1);
                encode_policy_translation_groups_records(&groups).unwrap()
                    [usize::from(name == "projection_translation_member")]
                .bytes
                .clone()
            }
            "projection_output_launch_context" => {
                let input = sections(&[(
                    "projection_output_launch_context",
                    PROJECTION_OUTPUT_LAUNCH_CONTEXT_RECORD_KIND,
                )]);
                encode_policy_output_launch_contexts_records(
                    &decode_policy_output_launch_contexts_records(1, &refs(&input)).unwrap(),
                    1,
                )
                .unwrap()[0]
                    .bytes
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
            "projection_presentation"
            | "projection_presentation_output"
            | "projection_surface_instance"
            | "projection_presentation_region"
            | "projection_presentation_binding" => {
                let records = [
                    (
                        "projection_presentation",
                        PROJECTION_PRESENTATION_RECORD_KIND,
                    ),
                    (
                        "projection_presentation_output",
                        PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND,
                    ),
                    (
                        "projection_surface_instance",
                        PROJECTION_SURFACE_INSTANCE_RECORD_KIND,
                    ),
                    (
                        "projection_presentation_region",
                        PROJECTION_PRESENTATION_REGION_RECORD_KIND,
                    ),
                    (
                        "projection_presentation_binding",
                        PROJECTION_PRESENTATION_BINDING_RECORD_KIND,
                    ),
                ];
                let input = sections(&records);
                let presentation = decode_policy_presentation_records(&refs(&input))
                    .unwrap()
                    .unwrap();
                assert_eq!(presentation.instances[0].id, 2);
                let index = records
                    .iter()
                    .position(|(label, _)| *label == name)
                    .unwrap();
                encode_policy_presentation_records(Some(&presentation), 1).unwrap()[index]
                    .bytes
                    .clone()
            }
            other => panic!("unknown record `{other}`"),
        };
        assert_eq!(encoded, data, "golden mismatch for {name}");
        rows += 1;
    }
    // Control: the corpus is actually read, not silently empty.
    assert!(rows >= 20, "only {rows} golden rows");
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
        Err(BinaryCodecError::ReservedNonZero(1))
    );
    data[20] = 0;
    data.push(0);
    assert_eq!(
        decode_wm_v1_projection_output_records(&data, 1),
        Err(BinaryCodecError::TrailingBytes(1))
    );
}

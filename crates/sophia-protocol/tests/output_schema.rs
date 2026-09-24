//! Independent schema samples must agree with the handwritten output codec.
use sophia_protocol::*;

fn corpus(text: &str) -> Vec<(&str, Vec<u8>)> {
    text.lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let (name, hex) = line.split_once(' ').unwrap();
            let bytes = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            (name, bytes)
        })
        .collect()
}

fn samples() -> Vec<(&'static str, Vec<u8>)> {
    corpus(include_str!(
        "../../../protocol/golden/sophia-output-v1.frames"
    ))
}

fn encode_decoded(name: &str, bytes: &[u8]) -> Result<Vec<u8>, IpcCodecError> {
    match name {
        "ClientHello" => {
            encode_output_v1_client_hello_frame(decode_output_v1_client_hello_frame(bytes)?)
        }
        "ServerWelcome" => {
            encode_output_v1_server_welcome_frame(decode_output_v1_server_welcome_frame(bytes)?)
        }
        "Snapshot" => {
            let (id, value) = decode_output_v1_snapshot_frame(bytes)?;
            encode_output_v1_snapshot_frame(id, &value)
        }
        "Proposal" => {
            let (id, value) = decode_output_v1_proposal_frame(bytes)?;
            encode_output_v1_proposal_frame(id, &value)
        }
        "Outcome" => {
            let (id, value) = decode_output_v1_outcome_frame(bytes)?;
            encode_output_v1_outcome_frame(id, value)
        }
        _ => panic!("unknown schema message {name}"),
    }
}

#[test]
fn every_schema_message_decodes_to_the_declared_values() {
    let samples = samples();
    assert_eq!(samples.len(), 5);
    for (name, bytes) in &samples {
        assert_eq!(encode_decoded(name, bytes).unwrap(), *bytes, "{name}");
    }
    let get = |name| &samples.iter().find(|(n, _)| *n == name).unwrap().1;
    assert_eq!(
        decode_output_v1_client_hello_frame(get("ClientHello")).unwrap(),
        OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: 3,
        }
    );
    assert_eq!(
        decode_output_v1_server_welcome_frame(get("ServerWelcome")).unwrap(),
        OutputV1ServerWelcome {
            selected_revision: 1,
            capabilities: 3,
            connection_epoch: 9,
            max_heads: 16,
            max_groups: 16,
            max_modes_per_head: 128,
            max_heads_per_group: 4,
        }
    );
    let (id, snapshot) = decode_output_v1_snapshot_frame(get("Snapshot")).unwrap();
    assert_eq!(id.raw(), 1);
    assert_eq!(snapshot.connection_epoch, 9);
    assert_eq!(
        snapshot.snapshot,
        OutputAuthoritySnapshot {
            topology_epoch: 7,
            primary_output: OutputId::from_raw(21),
            heads: vec![OutputHeadDescriptor {
                head: DisplayHeadId::from_raw(11),
                generation: 2,
                label: "Display".into(),
                connected: true,
                enabled: true,
                vrr_capable: true,
                transforms: OutputTransformSet::from_bits(255).unwrap(),
                current_mode: Some(DisplayModeId::from_raw(101)),
                modes: vec![OutputModeDescriptor {
                    mode: DisplayModeId::from_raw(101),
                    pixel_size: Size {
                        width: 800,
                        height: 600
                    },
                    refresh_millihz: 60000,
                    preferred: true,
                }],
            }],
            groups: vec![OutputLogicalGroupState {
                output: OutputId::from_raw(21),
                generation: 3,
                logical: Rect {
                    x: 800,
                    y: 0,
                    width: 800,
                    height: 600
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(11),
                    mapping: OutputHeadMapping::Exact
                }],
            }],
        }
    );
    let (id, proposal) = decode_output_v1_proposal_frame(get("Proposal")).unwrap();
    assert_eq!(id.raw(), 1);
    assert_eq!(proposal.connection_epoch, 9);
    assert_eq!(
        proposal.candidate,
        OutputTopologyCandidate {
            base_topology_epoch: 7,
            intent: OutputTopologyIntent::ValidateOnly,
            primary_group_index: 0,
            heads: vec![OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(11),
                head_generation: 2,
                mode: DisplayModeId::from_raw(101),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Automatic,
            }],
            groups: vec![OutputLogicalGroupProposal {
                output: OutputId::from_raw(21),
                logical: Rect {
                    x: 800,
                    y: 0,
                    width: 800,
                    height: 600
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(11),
                    mapping: OutputHeadMapping::Exact
                }],
            }],
        }
    );
    proposal
        .candidate
        .validate_against(&snapshot.snapshot)
        .unwrap();
    assert_eq!(
        decode_output_v1_outcome_frame(get("Outcome")).unwrap(),
        (
            TransactionId::from_raw(1),
            OutputV1Outcome {
                connection_epoch: 9,
                topology_epoch: 7,
                kind: OutputV1OutcomeKind::Validated,
                reason: 0,
            }
        )
    );
}

#[test]
fn malformed_schema_vectors_and_every_truncated_prefix_fail_closed() {
    for (case, bytes) in corpus(include_str!(
        "../../../protocol/golden/sophia-output-v1-malformed.frames"
    )) {
        let (name, _) = case.split_once('.').unwrap();
        assert!(encode_decoded(name, &bytes).is_err(), "{case}");
    }
    for (name, bytes) in samples() {
        for end in 0..bytes.len() {
            assert!(
                encode_decoded(name, &bytes[..end]).is_err(),
                "{name} prefix {end}"
            );
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(
            encode_decoded(name, &trailing).is_err(),
            "{name} trailing envelope"
        );
        let payload_len = (trailing.len() - 24) as u32;
        trailing[16..20].copy_from_slice(&payload_len.to_le_bytes());
        assert!(
            encode_decoded(name, &trailing).is_err(),
            "{name} trailing payload"
        );
    }
}

#[test]
fn schema_bounds_and_capabilities_match_the_public_constants() {
    let values = include_str!("../../../protocol/golden/sophia-output-v1.values");
    for (family, name, expected) in [
        ("limit", "heads", MAX_OUTPUT_AUTHORITY_HEADS as u64),
        ("limit", "groups", MAX_OUTPUT_AUTHORITY_GROUPS as u64),
        (
            "limit",
            "modes_per_head",
            MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD as u64,
        ),
        (
            "limit",
            "heads_per_group",
            MAX_OUTPUT_AUTHORITY_HEADS_PER_GROUP as u64,
        ),
        (
            "limit",
            "label_bytes",
            MAX_OUTPUT_AUTHORITY_LABEL_BYTES as u64,
        ),
        ("capability", "observe", SOPHIA_OUTPUT_CAPABILITY_OBSERVE),
        (
            "capability",
            "configure",
            SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
        ),
        (
            "reason",
            "none",
            u64::from(SOPHIA_OUTPUT_OUTCOME_REASON_NONE),
        ),
        (
            "reason",
            "stale",
            u64::from(SOPHIA_OUTPUT_OUTCOME_REASON_STALE),
        ),
        (
            "reason",
            "preparation",
            u64::from(SOPHIA_OUTPUT_OUTCOME_REASON_PREPARATION),
        ),
        (
            "reason",
            "apply",
            u64::from(SOPHIA_OUTPUT_OUTCOME_REASON_APPLY),
        ),
        (
            "reason",
            "head_lost",
            u64::from(SOPHIA_OUTPUT_OUTCOME_REASON_HEAD_LOST),
        ),
        (
            "reason",
            "first_presentation",
            u64::from(SOPHIA_OUTPUT_OUTCOME_REASON_FIRST_PRESENTATION),
        ),
        (
            "reason",
            "rollback",
            u64::from(SOPHIA_OUTPUT_OUTCOME_REASON_ROLLBACK),
        ),
        (
            "reason",
            "invariant",
            u64::from(SOPHIA_OUTPUT_OUTCOME_REASON_INVARIANT),
        ),
    ] {
        assert!(
            values
                .lines()
                .any(|line| line == format!("{family} {name} {expected}")),
            "{family} {name}"
        );
    }
}

#[test]
fn proposal_decoding_preserves_signed_coordinates_but_owner_rejects_negative_origins() {
    let samples = samples();
    let mut bytes = samples
        .iter()
        .find(|(n, _)| *n == "Proposal")
        .unwrap()
        .1
        .clone();
    // Header, proposal prefix, one HeadTarget, then the group output ID.
    bytes[88..92].copy_from_slice(&(-800_i32).to_le_bytes());
    let (id, proposal) = decode_output_v1_proposal_frame(&bytes).unwrap();
    assert_eq!(proposal.candidate.groups[0].logical.x, -800);
    assert_eq!(
        encode_output_v1_proposal_frame(id, &proposal).unwrap(),
        bytes
    );
    let snapshot = &samples.iter().find(|(n, _)| *n == "Snapshot").unwrap().1;
    let (_, snapshot) = decode_output_v1_snapshot_frame(snapshot).unwrap();
    assert!(
        proposal
            .candidate
            .validate_against(&snapshot.snapshot)
            .is_err()
    );
}

#[test]
fn largest_published_snapshot_stays_within_one_family_payload() {
    let samples = samples();
    let (_, mut message) =
        decode_output_v1_snapshot_frame(&samples.iter().find(|(n, _)| *n == "Snapshot").unwrap().1)
            .unwrap();
    let head = message.snapshot.heads[0].clone();
    let group = message.snapshot.groups[0].clone();
    message.snapshot.heads.clear();
    message.snapshot.groups.clear();
    for index in 0..MAX_OUTPUT_AUTHORITY_HEADS {
        let mut head = head.clone();
        head.head = DisplayHeadId::from_raw(index as u64 + 1);
        head.label = "D".repeat(MAX_OUTPUT_AUTHORITY_LABEL_BYTES);
        head.modes = (0..MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD)
            .map(|mode| OutputModeDescriptor {
                mode: DisplayModeId::from_raw(mode as u64 + 1),
                ..head.modes[0]
            })
            .collect();
        head.current_mode = Some(head.modes[0].mode);
        let mut group = group.clone();
        group.output = OutputId::from_raw(index as u64 + 1);
        group.logical.x = index as i32 * 800;
        group.members[0].head = head.head;
        message.snapshot.heads.push(head);
        message.snapshot.groups.push(group);
    }
    message.snapshot.primary_output = message.snapshot.groups[0].output;
    let bytes = encode_output_v1_snapshot_frame(TransactionId::from_raw(1), &message).unwrap();
    assert!(bytes.len() <= SOPHIA_IPC_HEADER_LEN + SOPHIA_IPC_MAX_PAYLOAD_LEN);
    assert_eq!(decode_output_v1_snapshot_frame(&bytes).unwrap().1, message);
    // A valid UTF-8 label that exceeds its fixed bound is still invalid.
    message.snapshot.heads[0].label.push('D');
    assert!(encode_output_v1_snapshot_frame(TransactionId::from_raw(1), &message).is_err());
}

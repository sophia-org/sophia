use sha2::{Digest, Sha256};
use sophia_protocol::output_files::*;
use sophia_protocol::*;

pub fn snapshot(maximum: bool) -> OutputAuthoritySnapshot {
    let count = if maximum {
        MAX_OUTPUT_AUTHORITY_HEADS
    } else {
        1
    };
    let modes = if maximum {
        MAX_OUTPUT_AUTHORITY_MODES_PER_HEAD
    } else {
        2
    };
    OutputAuthoritySnapshot {
        topology_epoch: 7,
        primary_output: OutputId::from_raw(1),
        heads: (0..count)
            .map(|i| OutputHeadDescriptor {
                head: DisplayHeadId::from_raw(i as u64 + 1),
                generation: 1,
                label: if maximum {
                    format!("{i:064}")
                } else {
                    "panel".into()
                },
                connected: true,
                enabled: true,
                vrr_capable: i % 2 != 0,
                transforms: OutputTransformSet::ALL,
                current_mode: Some(DisplayModeId::from_raw(1)),
                modes: (0..modes)
                    .map(|m| OutputModeDescriptor {
                        mode: DisplayModeId::from_raw(m as u64 + 1),
                        pixel_size: Size {
                            width: 800,
                            height: 600,
                        },
                        refresh_millihz: 60_000 + m as u32 * 15_000,
                        preferred: m == 0,
                    })
                    .collect(),
            })
            .collect(),
        groups: (0..count)
            .map(|i| OutputLogicalGroupState {
                output: OutputId::from_raw(i as u64 + 1),
                generation: 1,
                logical: Rect {
                    x: i as i32 * 800,
                    y: 0,
                    width: 800,
                    height: 600,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(i as u64 + 1),
                    mapping: OutputHeadMapping::Exact,
                }],
            })
            .collect(),
    }
}

pub fn candidate(snapshot: &OutputAuthoritySnapshot) -> OutputTopologyCandidate {
    OutputTopologyCandidate {
        base_topology_epoch: snapshot.topology_epoch,
        intent: OutputTopologyIntent::ValidateOnly,
        primary_group_index: 0,
        heads: snapshot
            .heads
            .iter()
            .map(|head| OutputHeadTargetProposal {
                head: head.head,
                head_generation: head.generation,
                mode: head.current_mode.unwrap(),
                // These target values are supplied by the fixture; revision 1
                // does not publish current transform or VRR settings.
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Disabled,
            })
            .collect(),
        groups: snapshot
            .groups
            .iter()
            .map(|group| OutputLogicalGroupProposal {
                output: group.output,
                logical: group.logical,
                members: group.members.clone(),
            })
            .collect(),
    }
}

pub fn identity(snapshot: &OutputAuthoritySnapshot) -> serde_json::Value {
    let topology = encode_output_file_record(
        OutputFileHeader {
            kind: OutputFileKind::Topology,
            connection_epoch: 1,
            submission_id: 0,
            sequence: 0,
        },
        &encode_output_file_topology(&OutputV1Snapshot {
            connection_epoch: 1,
            snapshot: snapshot.clone(),
        })
        .unwrap(),
    )
    .unwrap();
    let proposal = encode_output_file_record(
        OutputFileHeader {
            kind: OutputFileKind::Proposal,
            connection_epoch: 1,
            submission_id: 2,
            sequence: 0,
        },
        &encode_output_file_proposal(
            TransactionId::from_raw(1),
            &OutputV1Proposal {
                connection_epoch: 1,
                candidate: candidate(snapshot),
            },
        )
        .unwrap(),
    )
    .unwrap();
    serde_json::json!({
        "topology_sha256": format!("{:x}", Sha256::digest(&topology)),
        "proposal_sha256": format!("{:x}", Sha256::digest(&proposal)),
        "topology_bytes": topology.len(), "proposal_bytes": proposal.len(),
        "head_count": snapshot.heads.len(), "group_count": snapshot.groups.len(),
        "mode_count": snapshot.heads.iter().map(|h| h.modes.len()).sum::<usize>(),
        "identity_connection_epoch": 1, "identity_transaction": 1,
    })
}

#[test]
fn maximum_fixture_reaches_both_published_record_bounds() {
    let topology = snapshot(true);
    topology.validate().unwrap();
    let identity = identity(&topology);
    assert_eq!(identity["topology_bytes"], 52_216);
    assert_eq!(identity["proposal_bytes"], 1_784);
    assert_eq!(identity["mode_count"], 2_048);
    assert_eq!(candidate(&topology).heads.len(), 16);
    snapshot(false).validate().unwrap();
}

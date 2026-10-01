// Output topology fixtures for the neutral output-configuration tests.
fn output_head(
    head: u64,
    generation: u64,
    label: &str,
    width: i32,
    height: i32,
) -> OutputHeadDescriptor {
    OutputHeadDescriptor {
        head: DisplayHeadId::from_raw(head),
        generation,
        label: label.to_owned(),
        connected: true,
        enabled: true,
        current_mode: Some(DisplayModeId::from_raw(head * 10)),
        transforms: OutputTransformSet::ALL,
        vrr_capable: true,
        modes: vec![OutputModeDescriptor {
            mode: DisplayModeId::from_raw(head * 10),
            pixel_size: Size { width, height },
            refresh_millihz: 60_000,
            preferred: true,
        }],
    }
}

fn output_authority_snapshot() -> OutputAuthoritySnapshot {
    OutputAuthoritySnapshot {
        topology_epoch: 7,
        primary_output: OutputId::from_raw(1),
        heads: vec![
            output_head(1, 3, "DP-1", 2560, 1440),
            output_head(2, 4, "DP-2", 1920, 1080),
            output_head(3, 8, "HDMI-A-1", 3840, 2160),
        ],
        groups: vec![
            OutputLogicalGroupState {
                output: OutputId::from_raw(1),
                generation: 5,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
                members: vec![
                    OutputGroupMember {
                        head: DisplayHeadId::from_raw(1),
                        mapping: OutputHeadMapping::Exact,
                    },
                    OutputGroupMember {
                        head: DisplayHeadId::from_raw(2),
                        mapping: OutputHeadMapping::Fit,
                    },
                ],
            },
            OutputLogicalGroupState {
                output: OutputId::from_raw(2),
                generation: 9,
                logical: Rect {
                    x: 2560,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Fit,
                }],
            },
        ],
    }
}

fn mixed_output_candidate() -> OutputTopologyCandidate {
    OutputTopologyCandidate {
        base_topology_epoch: 7,
        intent: OutputTopologyIntent::Apply,
        primary_group_index: 0,
        heads: vec![
            OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(1),
                head_generation: 3,
                mode: DisplayModeId::from_raw(10),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Automatic,
            },
            OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(2),
                head_generation: 4,
                mode: DisplayModeId::from_raw(20),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Disabled,
            },
            OutputHeadTargetProposal {
                head: DisplayHeadId::from_raw(3),
                head_generation: 8,
                mode: DisplayModeId::from_raw(30),
                transform: OutputTransform::Normal,
                vrr: OutputVrrPolicy::Always,
            },
        ],
        groups: vec![
            OutputLogicalGroupProposal {
                output: OutputId::from_raw(1),
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
                members: vec![
                    OutputGroupMember {
                        head: DisplayHeadId::from_raw(1),
                        mapping: OutputHeadMapping::Exact,
                    },
                    OutputGroupMember {
                        head: DisplayHeadId::from_raw(2),
                        mapping: OutputHeadMapping::Fit,
                    },
                ],
            },
            OutputLogicalGroupProposal {
                output: OutputId::from_raw(2),
                logical: Rect {
                    x: 2560,
                    y: 0,
                    width: 1920,
                    height: 1080,
                },
                members: vec![OutputGroupMember {
                    head: DisplayHeadId::from_raw(3),
                    mapping: OutputHeadMapping::Fit,
                }],
            },
        ],
    }
}

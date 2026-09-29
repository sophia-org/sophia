use sophia_protocol::shell_files::*;
use sophia_protocol::*;
#[path = "support/descriptor_file.rs"]
mod file;

fn snapshot() -> ShellTabSnapshot {
    ShellTabSnapshot {
        connection_epoch: 5,
        generation: 6,
        groups: vec![ShellTabGroup {
            slot: 1,
            output: OutputId::from_raw(7),
            focused: true,
            selected_slot: Some(1),
            entries: (1..=2)
                .map(|slot| ShellV1Descriptor {
                    slot,
                    generation: 9,
                    label: Some(DisplayLabel {
                        text: format!("Tab {slot}"),
                        redacted: false,
                    }),
                    trust_level: TrustLevel::Trusted,
                    attention: AttentionState::None,
                    action: ToplevelActionCapabilityRef {
                        token: u64::from(slot) + 40,
                        issuer_epoch: 3,
                        issuer_revocation_epoch: 4,
                        recipient_epoch: 5,
                        target_slot: slot,
                        target_generation: 9,
                    },
                })
                .collect(),
        }],
    }
}
#[test]
fn complete_transfer_and_candidate_round_trip() {
    file::round_trip(ShellDescriptorRecord::Tabs(snapshot()));
    let c = ShellTabCandidate {
        connection_epoch: 5,
        snapshot_generation: 6,
        candidate_generation: 7,
        groups: vec![1],
    };
    file::round_trip(ShellDescriptorRecord::TabsCandidate(c));
}
#[test]
fn malformed_whole_objects_fail_closed() {
    let bytes = file::encode(ShellDescriptorRecord::Tabs(snapshot())).unwrap();
    // File header + domain transaction + 24-byte table header. The first
    // group's focused boolean and reserved u16 remain strict on files.
    let group = SHELL_FILE_HEADER_BYTES + 8 + 24;
    for (offset, byte) in [(group + 18, 2), (group + 22, 1)] {
        let mut changed = bytes.clone();
        changed[offset] = byte;
        assert!(decode_shell_file_descriptor(&changed, ShellFileKind::Tabs).is_err());
    }
    // A file object has one epoch, rather than one header per old phase.
    let mut changed = bytes;
    changed[SHELL_FILE_HEADER_BYTES + 8] ^= 1;
    assert!(decode_shell_file_descriptor(&changed, ShellFileKind::Tabs).is_err());
}
#[test]
fn bounds_and_occurrence_identity_are_enforced() {
    let mut s = snapshot();
    s.groups[0].selected_slot = Some(3);
    assert!(validate_shell_tab_snapshot(&s).is_err());
    let mut s = snapshot();
    s.groups.push(s.groups[0].clone());
    assert!(validate_shell_tab_snapshot(&s).is_err());
    let mut s = snapshot();
    s.groups[0].entries[1].slot = 1;
    assert!(validate_shell_tab_snapshot(&s).is_err());
    let mut s = snapshot();
    s.groups[0].entries[0].action.recipient_epoch = 4;
    assert!(validate_shell_tab_snapshot(&s).is_err());
}

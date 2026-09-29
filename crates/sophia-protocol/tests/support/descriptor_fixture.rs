use sophia_protocol::*;

pub fn action(slot: u16, generation: u64) -> ToplevelActionCapabilityRef {
    ToplevelActionCapabilityRef {
        token: u64::from(slot) + 40,
        issuer_epoch: 3,
        issuer_revocation_epoch: 4,
        recipient_epoch: 5,
        target_slot: slot,
        target_generation: generation,
    }
}

pub fn snapshot() -> ShellV1DescriptorSnapshot {
    ShellV1DescriptorSnapshot {
        connection_epoch: 5,
        snapshot_generation: 6,
        output: OutputId::from_raw(7),
        output_generation: 8,
        broker_epoch: 3,
        broker_revocation_epoch: 4,
        descriptors: vec![
            ShellV1Descriptor {
                slot: 1,
                generation: 9,
                label: None,
                trust_level: TrustLevel::Trusted,
                attention: AttentionState::None,
                action: action(1, 9),
            },
            ShellV1Descriptor {
                slot: 2,
                generation: 10,
                label: Some(DisplayLabel {
                    text: "Browser".to_owned(),
                    redacted: true,
                }),
                trust_level: TrustLevel::Isolated,
                attention: AttentionState::Notice,
                action: action(2, 10),
            },
        ],
    }
}

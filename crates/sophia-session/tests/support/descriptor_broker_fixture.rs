//! Already-sanitized metadata for the descriptor presentation test. No broker
//! process is started; disclosure policy is outside that test's evidence.
#![cfg(test)]
use super::*;
use sophia_protocol::{AttentionState, BrokerToplevelActionGrant, ChromeDescriptor, TrustLevel};

pub(in crate::live_session) struct DescriptorFixture {
    descriptors: sophia_engine::ChromeDescriptorTable,
    grants: BTreeMap<SurfaceId, BrokerToplevelActionGrant>,
}

impl MetadataBrokerView for DescriptorFixture {
    fn connection_epoch(&self) -> u64 {
        1
    }
    fn descriptors(&self) -> &sophia_engine::ChromeDescriptorTable {
        &self.descriptors
    }
    fn grants(&self) -> &BTreeMap<SurfaceId, BrokerToplevelActionGrant> {
        &self.grants
    }
}

pub(in crate::live_session) fn broker_fixture(surface: SurfaceId) -> DescriptorFixture {
    let mut descriptors = sophia_engine::ChromeDescriptorTable::default();
    descriptors.upsert(ChromeDescriptor {
        surface,
        label: None,
        icon: None,
        trust_level: TrustLevel::Unknown,
        attention: AttentionState::None,
        generation: 1,
    });
    DescriptorFixture {
        descriptors,
        grants: [(
            surface,
            BrokerToplevelActionGrant {
                token: 7,
                revocation_epoch: 1,
                target_generation: 1,
            },
        )]
        .into(),
    }
}

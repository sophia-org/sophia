//! Already-sanitized metadata for the descriptor presentation test. No broker
//! process is started; disclosure policy is outside that test's evidence.
#![cfg(test)]
use super::*;
use sophia_protocol::{AttentionState, BrokerToplevelActionGrant, ChromeDescriptor, TrustLevel};

pub(in crate::live_session) fn broker_fixture(
    directory: &std::path::Path,
    surface: SurfaceId,
) -> LiveMetadataBroker {
    let mut descriptors = sophia_engine::ChromeDescriptorTable::default();
    descriptors.upsert(ChromeDescriptor {
        surface,
        label: None,
        icon: None,
        trust_level: TrustLevel::Unknown,
        attention: AttentionState::None,
        generation: 1,
    });
    LiveMetadataBroker {
        supervisor: ProcessSupervisor::new(
            SupervisedProcessKind::MetadataBroker,
            ProcessLaunchSpec::new("/bin/false"),
        ),
        transport: sophia_runtime::MetadataBrokerSessionTransport::bind_for_supervised_uid(
            directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap(),
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
        admitted: Default::default(),
        retired: Default::default(),
        connection_epoch: 1,
        next_transaction: 1,
    }
}

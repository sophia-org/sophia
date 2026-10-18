//! Real connection registry/negotiation, supplied protected-peer evidence and
//! afid verdicts. No process is spawned: this fixture does not prove launch,
//! protection or launch authentication. The authenticated negotiation is the
//! owner's own, so its sealed-image channel is configured by Session.
use super::*;
use std::path::PathBuf;

impl ShellComponentProcesses {
    pub(crate) fn reconnect_fixture_endpoint(
        &mut self,
        slot: usize,
        policy: ShellContentAdmissionPolicy,
    ) -> (ComponentConnectionKey, PathBuf) {
        let key = self.connections.reserve_attempt(slot).unwrap();
        self.slots[slot].key = Some(key);
        self.connections
            .begin_authenticated_negotiation(
                key,
                &ProtectionDomainEvidence {
                    backend: ProtectionBackendKind::Bubblewrap,
                    supervisor_pid: std::process::id(),
                    peer_pid: std::process::id(),
                    roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
                },
                Duration::from_secs(2),
                policy,
                Box::new(crate::test_image_authority::Factory),
            )
            .unwrap();
        (key, self.connections.socket_path(slot).unwrap().to_owned())
    }
}

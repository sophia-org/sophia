//! Real connection registry/negotiation, supplied protected-peer evidence.
//! No process is spawned: this fixture does not prove launch or protection.
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
            .begin_negotiation(
                key,
                &ProtectionDomainEvidence {
                    backend: ProtectionBackendKind::Bubblewrap,
                    supervisor_pid: std::process::id(),
                    peer_pid: std::process::id(),
                    roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
                },
                Duration::from_secs(2),
                policy,
            )
            .unwrap();
        (key, self.connections.socket_path(slot).unwrap().to_owned())
    }
}

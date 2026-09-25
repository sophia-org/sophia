//! Real connection registry/negotiation, supplied protected-peer evidence.
//! No process is spawned: this fixture does not prove launch or protection.
use super::*;
use std::os::unix::net::UnixStream;

impl ShellComponentProcesses {
    pub(crate) fn reconnect_fixture_peer(
        &mut self,
        slot: usize,
        policy: ShellContentAdmissionPolicy,
    ) -> (ComponentConnectionKey, UnixStream) {
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
        let peer = UnixStream::connect(self.connections.socket_path(slot).unwrap()).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        peer.set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        (key, peer)
    }
}

//! Private socket scaffolding only; all admission and FIFO policy is production.
use super::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
static NEXT: AtomicU64 = AtomicU64::new(1);
pub(super) const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 2,
    content_grant_epoch: 2,
};
pub(super) fn tx(id: u64) -> TransactionId {
    TransactionId::from_raw(id)
}
pub(super) fn limits() -> ContentLimits {
    ContentLimits::prototype(GRANT)
}
pub(super) fn empty() -> ContentEpochRegistry {
    ContentEpochRegistry::new(64 * 1024 * 1024).unwrap()
}
pub(super) fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    }
}
pub(super) struct Peer {
    pub transport: ShellComponentTransport,
    pub client: UnixStream,
    directory: std::path::PathBuf,
}
impl Peer {
    pub fn new(epochs: &mut ContentEpochRegistry, profile: ContentStoreProfile) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "catalog-action-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        transport
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        transport
            .reserve_content_with_profile(epochs, limits(), profile)
            .unwrap();
        let client = UnixStream::connect(transport.socket_path()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        Self {
            transport,
            client,
            directory,
        }
    }
    pub fn negotiate(
        &mut self,
        epochs: &mut ContentEpochRegistry,
        hello: ShellV1ClientHello,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.transport.begin_negotiation(
            epochs,
            GRANT.connection_epoch,
            Duration::from_secs(2),
            policy,
        )?;
        self.client
            .write_all(&encode_shell_v1_client_hello_frame(hello).unwrap())
            .unwrap();
        for _ in 0..2048 {
            if let Some(welcome) = self.transport.poll_negotiation(epochs, 7)? {
                return Ok(welcome);
            }
        }
        panic!("bounded negotiation did not complete")
    }
    pub fn read(&mut self) -> Vec<u8> {
        let mut frame = vec![0; SOPHIA_IPC_HEADER_LEN];
        self.client.read_exact(&mut frame).unwrap();
        let bytes = u32::from_le_bytes(frame[16..20].try_into().unwrap()) as usize;
        assert!(bytes <= 65536);
        frame.resize(SOPHIA_IPC_HEADER_LEN + bytes, 0);
        self.client
            .read_exact(&mut frame[SOPHIA_IPC_HEADER_LEN..])
            .unwrap();
        frame
    }
    pub fn send_content(&mut self, record: ShellContentRecord) {
        self.client
            .write_all(&encode_shell_content_frame(tx(10), &record).unwrap())
            .unwrap();
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

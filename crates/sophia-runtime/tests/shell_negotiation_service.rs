//! Private socket negotiation with supplied protection evidence. No supervisor,
//! display, device, Session scheduling, or natural kernel saturation claim.
use sophia_protocol::*;
use sophia_runtime::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Peer {
    server: ShellComponentTransport,
    client: UnixStream,
    directory: std::path::PathBuf,
}
impl Peer {
    fn new(registry: &mut ContentEpochRegistry, epoch: u64) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "shell-handshake-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut server = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        server
            .authorize_protected_peer(&ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            })
            .unwrap();
        let mut limits = ContentLimits::prototype(grant(epoch));
        limits.max_staging_bytes = 4 * 1024 * 1024;
        limits.max_resident_bytes = 12 * 1024 * 1024;
        limits.max_retiring_bytes = 8 * 1024 * 1024;
        server.reserve_content(registry, limits).unwrap();
        server
            .begin_negotiation(
                registry,
                epoch,
                Duration::from_secs(5),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: false,
                },
            )
            .unwrap();
        let client = UnixStream::connect(server.socket_path()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        Self {
            server,
            client,
            directory,
        }
    }
    fn visit(
        &mut self,
        registry: &mut ContentEpochRegistry,
        budget: usize,
    ) -> Result<Option<ShellV1ServerWelcome>, ShellTransportError> {
        self.server.poll_negotiation(registry, budget)
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn grant(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}
fn hello() -> Vec<u8> {
    encode_shell_v1_client_hello_frame(ShellV1ClientHello {
        minimum_revision: 5,
        maximum_revision: 6,
        required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
    })
    .unwrap()
}
fn frame(client: &mut UnixStream) -> Vec<u8> {
    let mut bytes = vec![0; SOPHIA_IPC_HEADER_LEN];
    client.read_exact(&mut bytes).unwrap();
    let length = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
    bytes.resize(SOPHIA_IPC_HEADER_LEN + length, 0);
    client
        .read_exact(&mut bytes[SOPHIA_IPC_HEADER_LEN..])
        .unwrap();
    bytes
}

#[test]
fn partial_hello_does_not_block_neighbor_or_consume_pipelined_record() {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut a = Peer::new(&mut registry, 1);
    let mut b = Peer::new(&mut registry, 2);
    a.client.write_all(&hello()[..7]).unwrap();
    assert!(a.visit(&mut registry, 65536).unwrap().is_none());
    b.client.write_all(&hello()).unwrap();
    assert_eq!(
        b.visit(&mut registry, 65536)
            .unwrap()
            .unwrap()
            .connection_epoch,
        2
    );
    assert!(a.visit(&mut registry, 65536).unwrap().is_none());
    assert!(registry.resources(grant(1)).is_some());
    assert!(registry.resources(grant(2)).is_some());
    a.client.write_all(&hello()[7..]).unwrap();
    // A second framed record is a byte-preservation probe, not a second
    // authorized handshake. Normal FIFO dispatch must retain it unchanged.
    a.client.write_all(&hello()).unwrap();
    assert_eq!(
        a.visit(&mut registry, 65536)
            .unwrap()
            .unwrap()
            .connection_epoch,
        1
    );
    assert_eq!(
        decode_shell_v1_server_welcome_frame(&frame(&mut a.client))
            .unwrap()
            .connection_epoch,
        1
    );
    assert_eq!(
        a.server
            .poll_kind(&mut registry, IpcMessageKind::ShellV1ClientHello)
            .unwrap(),
        Some(hello())
    );
    assert!(b.server.content_grant().is_some());
    a.server.disconnect(&mut registry).unwrap();
    b.server.disconnect(&mut registry).unwrap();
    assert!(
        a.server
            .collect_content_accounting(&mut registry)
            .quiescent()
    );
}

#[test]
fn exact_byte_budget_retains_reply_credit_until_final_byte() {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut peer = Peer::new(&mut registry, 1);
    peer.client.write_all(&hello()).unwrap();
    assert!(peer.visit(&mut registry, 0).unwrap().is_none());
    assert!(peer.server.content_grant().is_none());
    let unchanged = peer.server.content_accounting(&registry);
    assert_eq!(
        peer.server.send_async(&mut registry, hello()),
        Err(ShellTransportError::NotConnected)
    );
    assert_eq!(peer.server.content_accounting(&registry), unchanged);
    assert_eq!(
        peer.server.begin_negotiation(
            &registry,
            1,
            Duration::from_secs(1),
            ShellContentAdmissionPolicy::Denied
        ),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert_eq!(peer.server.content_accounting(&registry), unchanged);
    assert!(peer.visit(&mut registry, hello().len()).unwrap().is_none());
    let pending = peer.server.content_accounting(&registry);
    assert_eq!(pending.input_records, 1);
    assert!(pending.response_records >= 2 && pending.response_bytes >= 512);
    let welcome = ShellV1ServerWelcome {
        selected_revision: 6,
        connection_epoch: 1,
        capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
            | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
            | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
        max_descriptors: SOPHIA_SHELL_MAX_DESCRIPTORS as u16,
        max_label_bytes: MAX_CHROME_LABEL_LEN as u16,
        max_pending_activations: SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS as u16,
    };
    let mut limits = ContentLimits::prototype(grant(1));
    limits.max_staging_bytes = 4 * 1024 * 1024;
    limits.max_resident_bytes = 12 * 1024 * 1024;
    limits.max_retiring_bytes = 8 * 1024 * 1024;
    let length = encode_shell_v1_server_welcome_frame(welcome).unwrap().len()
        + encode_shell_content_frame(TransactionId::INVALID, &ShellContentRecord::Limits(limits))
            .unwrap()
            .len();
    let mut received = Vec::new();
    for _ in 1..length {
        assert!(peer.visit(&mut registry, 1).unwrap().is_none());
        assert!(peer.server.content_grant().is_none());
        assert_eq!(peer.server.content_accounting(&registry), pending);
        let mut byte = [0];
        peer.client.read_exact(&mut byte).unwrap();
        received.extend_from_slice(&byte);
    }
    assert_eq!(peer.visit(&mut registry, 1).unwrap(), Some(welcome));
    let mut byte = [0];
    peer.client.read_exact(&mut byte).unwrap();
    received.extend_from_slice(&byte);
    let welcome_bytes = encode_shell_v1_server_welcome_frame(welcome).unwrap();
    assert_eq!(&received[..welcome_bytes.len()], welcome_bytes.as_slice());
    assert!(peer.visit(&mut registry, 1).is_err());
    assert_eq!(peer.server.content_grant(), Some(grant(1)));
    assert_eq!(peer.server.content_accounting(&registry).input_records, 0);
}

#[test]
fn malformed_and_eof_revoke_only_the_pending_reservation() {
    for eof in [false, true] {
        let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
        let mut a = Peer::new(&mut registry, 1);
        let b = Peer::new(&mut registry, 2);
        if eof {
            a.client.write_all(&hello()[..4]).unwrap();
            a.client.shutdown(std::net::Shutdown::Write).unwrap();
        } else {
            let mut bad = hello();
            bad[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
            a.client.write_all(&bad[..24]).unwrap();
        }
        assert!(a.visit(&mut registry, 65536).is_err());
        assert!(registry.resources(grant(1)).is_none());
        assert!(registry.resources(grant(2)).is_some());
        assert_eq!(a.server.content_accounting(&registry).response_records, 0);
        assert!(!b.server.content_accounting(&registry).quiescent());
    }
}

#[test]
fn deadline_and_explicit_disconnect_release_pending_owner() {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut peer = Peer::new(&mut registry, 1);
    peer.server.disconnect(&mut registry).unwrap();
    assert!(
        peer.server
            .collect_content_accounting(&mut registry)
            .quiescent()
    );
    assert!(
        peer.server
            .begin_negotiation(
                &registry,
                2,
                Duration::ZERO,
                ShellContentAdmissionPolicy::Unavailable
            )
            .is_ok()
    );
    assert!(matches!(
        peer.visit(&mut registry, 0),
        Err(ShellTransportError::Endpoint(
            PolicyRoleEndpointError::AcceptTimedOut
        ))
    ));
    assert!(peer.server.content_accounting(&registry).quiescent());
}

#[test]
fn content_refusal_is_delivered_before_terminal_error() {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let mut peer = Peer::new(&mut registry, 1);
    peer.server.disconnect(&mut registry).unwrap();
    peer.server
        .begin_negotiation(
            &registry,
            2,
            Duration::from_secs(5),
            ShellContentAdmissionPolicy::Denied,
        )
        .unwrap();
    peer.client.write_all(&hello()).unwrap();
    assert!(peer.visit(&mut registry, hello().len()).unwrap().is_none());
    let mut completed = false;
    for _ in 0..512 {
        match peer.visit(&mut registry, 1) {
            Ok(None) => assert!(!peer.server.content_accounting(&registry).quiescent()),
            Err(ShellTransportError::ContentAdmissionRefused(_)) => {
                completed = true;
                break;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert!(completed);
    let (_, record) = decode_shell_content_frame(&frame(&mut peer.client)).unwrap();
    assert!(matches!(record, ShellContentRecord::AdmissionRefused(_)));
    assert!(peer.server.content_accounting(&registry).quiescent());
}

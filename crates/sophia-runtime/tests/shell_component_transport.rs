//! Real private socket/codec/resource owners with supplied protection evidence.
//! No protected child, compositor, native presentation or launcher focus here.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ShellClientOptions, ShellConnection};

const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

fn limits(epoch: u64, launcher: bool) -> ContentLimits {
    let mut limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    });
    if launcher {
        limits.max_staging_bytes = 4 * MIB;
        limits.max_resident_bytes = 12 * MIB;
        limits.max_retiring_bytes = 8 * MIB;
    }
    limits
}

struct Component {
    transport: ShellComponentTransport,
    client: Option<ShellConnection>,
    directory: std::path::PathBuf,
}

impl Component {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "shell-component-{}-{}",
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
        Self {
            transport,
            client: None,
            directory,
        }
    }

    fn connect(&mut self, registry: &mut ContentEpochRegistry, expected: ContentLimits) {
        let socket = self.transport.socket_path().to_owned();
        let peer = std::thread::spawn(move || {
            let mut client = ShellConnection::connect(
                socket,
                ShellClientOptions {
                    minimum_revision: 5,
                    maximum_revision: 6,
                    required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
                    handshake_timeout: Duration::from_secs(2),
                },
            )
            .unwrap();
            let record = next_record(&mut client);
            (client, record)
        });
        let welcome = self
            .transport
            .accept_and_negotiate_with_content_policy(
                registry,
                expected.grant.connection_epoch,
                Duration::from_secs(2),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: false,
                },
            )
            .unwrap();
        let (client, received) = peer.join().unwrap();
        assert_eq!(welcome.connection_epoch, expected.grant.connection_epoch);
        assert_eq!(received, ShellContentRecord::Limits(expected.clone()));
        assert_eq!(self.transport.content_limits(), Some(&expected));
        self.client = Some(client);
    }

    fn send_upload(&mut self, grant: ContentGrant, id: u64, value: u8) {
        let client = self.client.as_mut().unwrap();
        let resource = ContentResourceId { id, generation: 1 };
        let tx = TransactionId::from_raw(id);
        for record in [
            ShellContentRecord::ResourceBegin(ContentResourceBegin {
                grant,
                resource,
                width_px: 1,
                height_px: 1,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 4,
            }),
            ShellContentRecord::ResourceChunk(ContentResourceChunk {
                grant,
                resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![value, 0, 0, 255],
            }),
            ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant,
                resource,
                total_bytes: 4,
                chunk_count: 1,
            }),
        ] {
            client.send_content(tx, &record).unwrap();
        }
    }

    fn uploaded(
        &mut self,
        registry: &mut ContentEpochRegistry,
        grant: ContentGrant,
        id: u64,
        now: u64,
    ) -> ContentResourceLease {
        let start = Instant::now();
        let mut received = Vec::new();
        while received.len() != 2 {
            self.transport
                .service_content_resources(registry, now)
                .unwrap();
            if let Some((_, record)) = self.client.as_mut().unwrap().poll_content().unwrap() {
                let ShellContentRecord::ResourceStatus(status) = record else {
                    panic!("resource status");
                };
                assert_eq!(status.grant, grant);
                assert_eq!(status.resource, ContentResourceId { id, generation: 1 });
                received.push(status.status);
            }
            assert!(start.elapsed() < Duration::from_secs(2));
        }
        assert_eq!(received, [1, 2]);
        self.transport
            .lease_content_resource(registry, grant, ContentResourceId { id, generation: 1 })
            .unwrap()
    }

    fn disconnect(&mut self, registry: &mut ContentEpochRegistry) {
        self.transport.disconnect(registry).unwrap();
        self.client = None;
    }
}

impl Drop for Component {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn next_record(client: &mut ShellConnection) -> ShellContentRecord {
    let start = Instant::now();
    loop {
        if let Some((_, record)) = client.poll_content().unwrap() {
            return record;
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
}

#[test]
fn two_connections_share_actual_stores_and_keep_neighbor_live_through_retirement() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let mut b = Component::new();
    let bar = limits(1, false);
    let menu = limits(2, true);
    a.transport
        .reserve_content(&mut registry, bar.clone())
        .unwrap();
    b.transport
        .reserve_content(&mut registry, menu.clone())
        .unwrap();
    assert_eq!(registry.reserved_bytes(), 64 * MIB); // before either peer connects
    assert!(!a.transport.supports_content());
    assert!(!b.transport.supports_content());
    a.connect(&mut registry, bar.clone());
    b.connect(&mut registry, menu.clone());
    a.send_upload(bar.grant, 1, 30);
    b.send_upload(menu.grant, 1, 70);
    let a_pixels = a.uploaded(&mut registry, bar.grant, 1, 0);
    let b_pixels = b.uploaded(&mut registry, menu.grant, 1, 0);
    assert_eq!(a_pixels.bytes(), [30, 0, 0, 255]);
    assert_eq!(b_pixels.bytes(), [70, 0, 0, 255]);
    assert!(
        a.transport
            .lease_content_resource(
                &registry,
                menu.grant,
                ContentResourceId {
                    id: 1,
                    generation: 1
                }
            )
            .is_err()
    );
    assert_eq!(registry.accounting().resources, 2);

    b.disconnect(&mut registry);
    assert_eq!(registry.retired_bytes(), 4);
    let replacement = limits(3, true);
    assert!(matches!(
        b.transport
            .reserve_content(&mut registry, replacement.clone()),
        Err(ShellTransportError::ContentStore(ContentStoreError::Budget))
    ));
    assert_eq!(a.transport.content_grant(), Some(bar.grant));
    a.send_upload(bar.grant, 2, 90);
    let next_a = a.uploaded(&mut registry, bar.grant, 2, 1);
    assert_eq!(next_a.bytes(), [90, 0, 0, 255]);
    assert_eq!(registry.retired_bytes(), 4);

    drop(b_pixels);
    registry.collect();
    assert_eq!(registry.retired_bytes(), 0);
    b.transport
        .reserve_content(&mut registry, replacement.clone())
        .unwrap();
    // A fresh protected-peer authorization is required for the new connection.
    b.transport
        .authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: std::process::id(),
            peer_pid: std::process::id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
    b.connect(&mut registry, replacement);
    assert_eq!(a.transport.content_grant(), Some(bar.grant));
    assert_eq!(a_pixels.bytes(), [30, 0, 0, 255]);
    drop((a_pixels, next_a));
    a.disconnect(&mut registry);
    b.disconnect(&mut registry);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

#[test]
fn peer_cannot_route_a_foreign_grant_into_its_neighbors_store() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let mut b = Component::new();
    let bar = limits(1, false);
    let menu = limits(2, true);
    a.transport
        .reserve_content(&mut registry, bar.clone())
        .unwrap();
    b.transport
        .reserve_content(&mut registry, menu.clone())
        .unwrap();
    a.connect(&mut registry, bar.clone());
    b.connect(&mut registry, menu.clone());
    a.send_upload(menu.grant, 1, 30);
    let before = registry.accounting();
    assert_eq!(
        a.transport.service_content_resources(&mut registry, 0),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert_eq!(registry.accounting(), before);
    a.disconnect(&mut registry);
    b.send_upload(menu.grant, 1, 70);
    let pixels = b.uploaded(&mut registry, menu.grant, 1, 0);
    assert_eq!(pixels.bytes(), [70, 0, 0, 255]);
    drop(pixels);
    b.disconnect(&mut registry);
    assert!(registry.accounting().quiescent());
}

#[test]
fn failed_handshake_revokes_exact_prelaunch_reservation_not_neighbor() {
    use std::io::{Read, Write};
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let mut b = Component::new();
    let bar = limits(1, false);
    let menu = limits(2, true);
    a.transport
        .reserve_content(&mut registry, bar.clone())
        .unwrap();
    a.connect(&mut registry, bar.clone());
    b.transport.reserve_content(&mut registry, menu).unwrap();
    let path = b.transport.socket_path().to_owned();
    let peer = std::thread::spawn(move || {
        let mut socket = std::os::unix::net::UnixStream::connect(path).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .write_all(
                &encode_shell_v1_client_hello_frame(ShellV1ClientHello {
                    minimum_revision: 6,
                    maximum_revision: 6,
                    required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER,
                })
                .unwrap(),
            )
            .unwrap();
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).unwrap();
        assert!(
            bytes.is_empty(),
            "no Welcome for a missing required content request"
        );
    });
    assert_eq!(
        b.transport.accept_and_negotiate_with_content_policy(
            &mut registry,
            2,
            Duration::from_secs(2),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false
            }
        ),
        Err(ShellTransportError::MissingCapability)
    );
    peer.join().unwrap();
    assert_eq!(registry.reserved_bytes(), 40 * MIB);
    assert_eq!(registry.accounting().active_epochs, 1);
    assert_eq!(a.transport.content_grant(), Some(bar.grant));
    assert!(!b.transport.supports_content());
    b.disconnect(&mut registry);
    assert_eq!(registry.accounting().active_epochs, 1);
    a.disconnect(&mut registry);
    assert!(registry.accounting().quiescent());
}

#[test]
fn legacy_negotiation_uses_common_epoch_after_a_reserved_component() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let mut b = Component::new();
    let small = limits(1, true);
    a.transport
        .reserve_content(&mut registry, small.clone())
        .unwrap();
    a.connect(&mut registry, small.clone());
    // No pre-reservation for the compatibility path: the registry supplies
    // content epoch 2, even though this new transport has never negotiated.
    b.connect(&mut registry, limits(2, false));
    assert_eq!(registry.reserved_bytes(), 64 * MIB);
    assert_eq!(a.transport.content_grant(), Some(small.grant));
    a.disconnect(&mut registry);
    b.disconnect(&mut registry);
    assert!(registry.accounting().quiescent());
}

#[test]
fn refused_replacement_and_wrong_connection_preserve_prelaunch_owner() {
    let mut registry = ContentEpochRegistry::new(64 * MIB).unwrap();
    let mut a = Component::new();
    let selected = limits(1, false);
    a.transport
        .reserve_content(&mut registry, selected.clone())
        .unwrap();
    let before = registry.accounting();
    assert_eq!(
        a.transport.reserve_content(&mut registry, limits(2, true)),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert_eq!(
        a.transport.accept_and_negotiate_with_content_policy(
            &mut registry,
            2,
            Duration::ZERO,
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false
            }
        ),
        Err(ShellTransportError::WrongContentGrant)
    );
    assert_eq!(registry.accounting(), before);
    a.connect(&mut registry, selected);
    a.disconnect(&mut registry);
    a.disconnect(&mut registry);
    assert!(registry.accounting().quiescent());
}

#[test]
fn explicit_three_owner_capacity_preserves_budget_and_neighbor_progress() {
    assert!(ContentEpochRegistry::with_active_capacity(64 * MIB, 0).is_err());
    assert!(ContentEpochRegistry::with_active_capacity(64 * MIB, 4).is_err());
    let bounded = |epoch| {
        let mut value = limits(epoch, true);
        value.max_staging_bytes = 4 * MIB;
        value.max_resident_bytes = if epoch == 1 { 12 * MIB } else { 8 * MIB };
        value.max_retiring_bytes = 8 * MIB;
        value
    };
    let mut legacy = ContentEpochRegistry::new(64 * MIB).unwrap();
    legacy.admit(bounded(1)).unwrap();
    legacy.admit(bounded(2)).unwrap();
    assert_eq!(legacy.admit(bounded(3)), Err(ContentStoreError::Budget));

    let mut registry = ContentEpochRegistry::with_active_capacity(64 * MIB, 3).unwrap();
    let mut peers: [_; 3] = std::array::from_fn(|_| Component::new());
    let mut held = Vec::new();
    for (index, peer) in peers.iter_mut().enumerate() {
        let budget = bounded(index as u64 + 1);
        peer.transport
            .reserve_content(&mut registry, budget.clone())
            .unwrap();
        peer.connect(&mut registry, budget.clone());
        peer.send_upload(budget.grant, 1, 30 + index as u8);
        held.push(peer.uploaded(&mut registry, budget.grant, 1, 0));
    }
    assert_eq!(registry.reserved_bytes(), 64 * MIB);
    assert_eq!(registry.admit(bounded(4)), Err(ContentStoreError::Budget));
    peers[2].disconnect(&mut registry);
    assert_eq!(registry.retired_bytes(), 4);
    assert_eq!(registry.admit(bounded(4)), Err(ContentStoreError::Budget));
    for (index, peer) in peers[..2].iter_mut().enumerate() {
        let grant = bounded(index as u64 + 1).grant;
        peer.send_upload(grant, 2, 80 + index as u8);
        let pixels = peer.uploaded(&mut registry, grant, 2, 1);
        assert_eq!(pixels.bytes(), [80 + index as u8, 0, 0, 255]);
    }
    drop(held.pop());
    registry.collect();
    assert_eq!(registry.retired_bytes(), 0);
    registry.admit(bounded(4)).unwrap();
    assert_eq!(registry.reserved_bytes(), 64 * MIB);
    assert!(registry.resources(bounded(3).grant).is_none());
    assert!(registry.disconnect(bounded(4).grant));
    for peer in &mut peers[..2] {
        peer.disconnect(&mut registry);
    }
    drop(held);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

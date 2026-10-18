//! Session's connection owner served by the public SDK over 9P. Protection
//! evidence is supplied; protected child launch is tested separately.
use sophia_config::ShellComponentRole;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::shell_component_connections::*;
use sophia_shell_client::{ShellClientOptions, ShellConnection};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[path = "../../../sophia-runtime/tests/support/image_test_authority.rs"]
mod authority;
#[path = "../../../sophia-runtime/tests/support/shell_sdk_image.rs"]
mod image;
use super::shell_file_peer as raw;
#[allow(dead_code)]
#[path = "component_image_custody.rs"]
mod allocation;

static NEXT: AtomicU64 = AtomicU64::new(0);

pub fn evidence() -> ProtectionDomainEvidence {
    ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
    }
}

pub struct Harness {
    pub owner: ShellComponentConnections,
    pub directory: std::path::PathBuf,
}

impl Harness {
    pub fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "session-component-files-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let mut owner = ShellComponentConnections::new().unwrap();
        for (id, role) in [
            ("panel", ShellComponentRole::Bar),
            ("menu", ShellComponentRole::ApplicationLauncher),
        ] {
            owner
                .add(
                    id,
                    role,
                    &directory.join(id),
                    rustix::process::geteuid().as_raw(),
                )
                .unwrap();
        }
        Self { owner, directory }
    }

    pub fn connect(&mut self, key: ComponentConnectionKey) -> ShellConnection {
        let native = key.slot == 1;
        let revision = 9;
        let capabilities = if key.slot == 2 {
            SOPHIA_SHELL_CAPABILITY_PERSISTENT_CATALOG
                | SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
                | SOPHIA_SHELL_CAPABILITY_WORK_AREA_RESERVATION
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
        } else if native {
            SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
                | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER
        } else {
            SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                | SOPHIA_SHELL_CAPABILITY_VIEW_INDICATORS
                | SOPHIA_SHELL_CAPABILITY_INDICATOR_ACTIVATION
        };
        let capabilities = capabilities | (1 << 13);
        self.owner
            .begin_authenticated_negotiation(
                key,
                &evidence(),
                Duration::from_secs(2),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: key.slot != 0,
                },
                Box::new(authority::Factory),
            )
            .unwrap();
        let socket = self.owner.socket_path(key.slot).unwrap().to_owned();
        let peer = std::thread::spawn(move || {
            let mut client = image::authenticated_client(
                &socket,
                ShellClientOptions {
                    minimum_revision: revision,
                    maximum_revision: revision,
                    required_capabilities: capabilities,
                    handshake_timeout: Duration::from_secs(2),
                },
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some((_, record)) = client.poll_content().unwrap() {
                    let ShellContentRecord::Limits(limits) = &record else {
                        panic!("Limits");
                    };
                    image::enable(&mut client, socket.parent().unwrap(), limits);
                    return (client, record);
                }
                assert!(Instant::now() < deadline, "missing Limits");
                std::thread::yield_now();
            }
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let welcome = loop {
            if let Some((received, result)) = self
                .owner
                .poll_negotiations(65536)
                .into_iter()
                .flatten()
                .next()
            {
                assert_eq!(received, key);
                break result.unwrap();
            }
            assert!(Instant::now() < deadline, "negotiation hung");
            std::thread::yield_now();
        };
        while !peer.is_finished() {
            self.owner
                .with_connection(key, |t| t.poll_io().unwrap())
                .unwrap();
            assert!(Instant::now() < deadline, "Limits fetch hung");
            std::thread::yield_now();
        }
        let (client, limits) = peer.join().unwrap();
        assert_eq!(client.welcome(), welcome);
        assert_eq!(welcome.connection_epoch, key.grant.connection_epoch);
        assert_eq!(welcome.selected_revision, revision);
        assert_eq!(
            welcome.capabilities & SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER != 0,
            native
        );
        self.owner
            .with_connection(key, |t| {
                assert_eq!(t.supports_native_launcher(), native);
                assert_eq!(
                    limits,
                    ShellContentRecord::Limits(t.content_limits().unwrap().clone())
                );
            })
            .unwrap();
        client
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

pub fn upload(
    h: &mut Harness,
    key: ComponentConnectionKey,
    client: &mut ShellConnection,
    id: u64,
) -> ContentResourceLease {
    let grant = key.grant;
    let resource = ContentResourceId { id, generation: 1 };
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    let native = key.slot == 1;
    let opening = NativeLauncherOpening {
        grant,
        output,
        opening: 1,
        catalog_generation: 1,
        state_revision: 1,
    };
    if native {
        h.owner
            .with_connection(key, |t| {
                allocation::publish(t, 1);
                t.publish_native_launcher_opening(TransactionId::from_raw(79), opening)
                    .unwrap();
            })
            .unwrap();
        client
            .enqueue_native_launcher_tracked(
                &sophia_protocol::shell_files::ShellFileNativeLauncherRecord {
                    transaction: TransactionId::from_raw(80),
                    record: ShellNativeLauncherRecord::AllocationRequest(
                        NativeLauncherAllocationRequest {
                            grant,
                            output,
                            opening: 1,
                            request_id: 1,
                            prior: ContentAllocationId::default(),
                            operation: 1,
                            edge: 1,
                            desired_width: 200,
                            desired_height: 20,
                            margins: ContentMargins::default(),
                        },
                    ),
                },
            )
            .unwrap();
    } else {
        client
            .send_content(
                TransactionId::from_raw(80),
                &ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                    grant,
                    output,
                    allocation_request_id: 1,
                    operation: 1,
                    role: 1,
                    edge: 1,
                    prior: ContentAllocationId::default(),
                    parent: ContentAllocationId::default(),
                    parent_presentation_epoch: 0,
                    anchor_parent_rect: ContentPixelRect::default(),
                    desired_width: 200,
                    desired_height: 20,
                    margins: ContentMargins::default(),
                }),
            )
            .unwrap();
    }
    let mut published = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(h.owner.turn_connected().iter().all(Option::is_none));
        let now = h.owner.content_clock_msec();
        h.owner
            .with_connection(key, |t| {
                if native {
                    let catalog = ShellApplicationCatalog {
                        connection_epoch: grant.connection_epoch,
                        generation: 1,
                        entries: Vec::new(),
                    };
                    t.service_native_launcher_content(
                        ContentCandidateContext {
                            output,
                            facts_generation: 1,
                            interaction_generation: 1,
                            allocations: &[],
                        },
                        NativeLauncherCandidateContext {
                            opening,
                            state_revision: 1,
                            catalog: &catalog,
                        },
                        now,
                    )
                    .unwrap();
                    if t.next_content_allocation_request().is_some() {
                        t.grant_content_allocation(
                            1,
                            ContentAllocationSnapshot {
                                native_opening: Some(1),
                                output,
                                allocation: ContentAllocationId {
                                    id: 1,
                                    generation: 1,
                                },
                                scale_generation: 1,
                                scale_numerator: 1,
                                scale_denominator: 1,
                                role: 3,
                                edge: 1,
                                margins: ContentMargins::default(),
                                logical: ContentLogicalRect {
                                    x: 0,
                                    y: 0,
                                    width: 200,
                                    height: 20,
                                },
                                pixel: ContentPixelRect {
                                    x: 0,
                                    y: 0,
                                    width: 200,
                                    height: 20,
                                },
                                parent: ContentAllocationId::default(),
                                anchor_parent_rect: ContentPixelRect::default(),
                                allowed_reservation_extent: 0,
                            },
                            &[],
                        )
                        .unwrap();
                    }
                } else {
                    allocation::service(t, now, &mut published);
                }
            })
            .unwrap();
        client.poll_io().unwrap();
        let record = if native {
            match client.take_native_launcher_observation().unwrap() {
                Some(sophia_shell_client::NativeLauncherObservation::Content(_, record)) => {
                    Some(record)
                }
                _ => None,
            }
        } else {
            client.poll_content().unwrap().map(|(_, record)| record)
        };
        if let Some(ShellContentRecord::AllocationResult(result)) = record {
            assert_eq!(result.status, 1, "{result:?}");
            break;
        }
        assert!(Instant::now() < deadline, "allocation not granted");
    }
    let metadata = sophia_shell_client::ProducerMetadata {
        resource: id,
        resource_generation: 1,
        admission_epoch: grant.content_grant_epoch,
        output_id: 1,
        allocation_id: 1,
        topology_generation: 1,
        source_kind: 1,
        source_format: 1,
        width: 1,
        height: 1,
        stride: 4,
        plane_count: 1,
        descriptor_count: 1,
        declared_bytes: 4,
        ..Default::default()
    };
    std::thread::scope(|scope| {
        let peer = scope.spawn(|| image::upload(client, metadata, &[1, 2, 3, 255]));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !peer.is_finished() {
            assert!(h.owner.turn_connected().iter().all(Option::is_none));
            let now = h.owner.content_clock_msec();
            h.owner
                .with_connection(key, |t| {
                    t.service_content_resources(now).unwrap();
                })
                .unwrap();
            assert!(Instant::now() < deadline, "image admission hung");
            std::thread::yield_now();
        }
        peer.join().unwrap();
    });
    h.owner
        .with_connection(key, |t| t.lease_content_resource(grant, resource).unwrap())
        .unwrap()
}

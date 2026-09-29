//! Session's connection owner served by the public SDK over 9P. Protection
//! evidence is supplied; protected child launch is tested separately.
use sophia_config::ShellComponentRole;
use sophia_protocol::*;
use sophia_runtime::*;
use sophia_session::shell_component_connections::*;
use sophia_shell_client::{ShellClientOptions, ShellConnection};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

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
        let revision = if key.slot == 2 {
            8
        } else if native {
            7
        } else {
            6
        };
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
        self.owner
            .begin_negotiation(
                key,
                &evidence(),
                Duration::from_secs(2),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: key.slot != 0,
                },
            )
            .unwrap();
        let socket = self.owner.socket_path(key.slot).unwrap().to_owned();
        let peer = std::thread::spawn(move || {
            let mut client = ShellConnection::connect_files(
                socket,
                ShellClientOptions {
                    minimum_revision: if revision == 6 { 5 } else { revision },
                    maximum_revision: revision,
                    required_capabilities: capabilities,
                    handshake_timeout: Duration::from_secs(2),
                },
            )
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some((_, record)) = client.poll_content().unwrap() {
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
            bytes: vec![1, 2, 3, 255],
        }),
        ShellContentRecord::ResourceEnd(ContentResourceEnd {
            grant,
            resource,
            total_bytes: 4,
            chunk_count: 1,
        }),
    ] {
        client
            .send_content(TransactionId::from_raw(id), &record)
            .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    for status in [1, 2] {
        loop {
            h.owner
                .with_connection(key, |t| {
                    t.service_content_resources(1).unwrap();
                    t.poll_io().unwrap();
                })
                .unwrap();
            if let Some((_, record)) = client.poll_content().unwrap() {
                let ShellContentRecord::ResourceStatus(value) = record else {
                    panic!("resource status");
                };
                assert_eq!(value.grant, grant);
                assert_eq!(value.resource, resource);
                assert_eq!(value.status, status);
                break;
            }
            assert!(Instant::now() < deadline, "upload hung");
            std::thread::yield_now();
        }
    }
    h.owner
        .with_connection(key, |t| t.lease_content_resource(grant, resource).unwrap())
        .unwrap()
}

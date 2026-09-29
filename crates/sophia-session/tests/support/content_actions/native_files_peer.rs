//! Production file export with supplied protection and renderer completions.
use super::shell_file_peer;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[allow(dead_code)]
#[path = "../../../../sophia-runtime/tests/support/native_launcher_content.rs"]
mod support;
pub(crate) use support::*;
#[path = "native_files_client.rs"]
mod client;
use client::{Client, Observation};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CAPS: u64 = SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER;

pub(crate) fn empty() -> ContentEpochRegistry {
    ContentEpochRegistry::new(64 * 1024 * 1024).unwrap()
}
pub(crate) struct Peer {
    pub transport: ShellComponentTransport,
    client: Client,
    limits: ContentLimits,
}

impl Peer {
    pub fn with_limits(
        r: &mut ContentEpochRegistry,
        profile: ContentStoreProfile,
        limits: ContentLimits,
    ) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "native-action-files-{}-{}",
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
            .reserve_content_with_profile(r, limits.clone(), profile)
            .unwrap();
        let client = Client::new(transport.socket_path(), limits.grant.connection_epoch);
        Self {
            transport,
            client,
            limits,
        }
    }

    pub fn negotiate(&mut self, r: &mut ContentEpochRegistry) {
        let limits = self.limits.clone();
        self.transport
            .begin_file_negotiation(
                r,
                limits.grant.connection_epoch,
                Duration::from_secs(3),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: true,
                },
            )
            .unwrap();
        std::thread::scope(|scope| {
            let client = &mut self.client;
            let worker = scope.spawn(move || {
                client.wire.setup();
                let offer = encode_shell_file_negotiate(
                    client.header(ShellFileKind::Negotiate),
                    ShellV1ClientHello {
                        minimum_revision: 7,
                        maximum_revision: 7,
                        required_capabilities: CAPS,
                    },
                )
                .unwrap();
                client.submit(&offer);
                let event = client.wire.next_event();
                let welcome = decode_shell_file_negotiated(&event).unwrap().welcome;
                client.wire.ack(&event);
                client.wire.open(6, b"limits", 0);
                assert_eq!(
                    decode_shell_file_limits(&client.wire.read(6, 0)).unwrap(),
                    limits
                );
                welcome
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            let welcome = loop {
                if let Some(welcome) = self.transport.poll_negotiation(r, 65536).unwrap() {
                    break welcome;
                }
                assert!(Instant::now() < deadline, "native negotiation hung");
                std::thread::yield_now();
            };
            while !worker.is_finished() {
                self.transport.poll_io(r).unwrap();
                assert!(Instant::now() < deadline, "native Limits read hung");
                std::thread::yield_now();
            }
            assert_eq!(worker.join().unwrap(), welcome);
            assert_eq!(welcome.selected_revision, 7);
            assert_eq!(welcome.capabilities, CAPS);
            assert_eq!(welcome.connection_epoch, self.limits.grant.connection_epoch);
        });
    }

    fn drive<R: Send>(
        &mut self,
        r: &mut ContentEpochRegistry,
        operation: impl FnOnce(&mut Client) -> R + Send,
    ) -> R {
        std::thread::scope(|scope| {
            let client = &mut self.client;
            let worker = scope.spawn(move || operation(client));
            let deadline = Instant::now() + Duration::from_secs(5);
            while !worker.is_finished() {
                self.transport.poll_io(r).unwrap();
                assert!(Instant::now() < deadline, "native file operation hung");
                std::thread::yield_now();
            }
            worker.join().unwrap()
        })
    }
    pub fn read_content(
        &mut self,
        r: &mut ContentEpochRegistry,
    ) -> (TransactionId, ShellContentRecord) {
        let Observation::Content(tx, value) = self.drive(r, Client::next) else {
            panic!("content event");
        };
        (tx, value)
    }
    pub fn read_native(
        &mut self,
        r: &mut ContentEpochRegistry,
    ) -> (TransactionId, ShellNativeLauncherRecord) {
        let Observation::Native(tx, value) = self.drive(r, Client::next) else {
            panic!("native event");
        };
        (tx, value)
    }
    pub fn read_catalog(&mut self, r: &mut ContentEpochRegistry) -> ShellApplicationCatalog {
        let Observation::Catalog(value) = self.drive(r, Client::next) else {
            panic!("catalog object");
        };
        value
    }
    pub fn send(&mut self, r: &mut ContentEpochRegistry, record: ShellNativeLauncherRecord) {
        self.drive(r, |client| {
            let bytes = encode_shell_file_native_launcher_transaction(
                client.header(shell_file_native_launcher_kind(&record).unwrap()),
                &ShellFileNativeLauncherRecord {
                    transaction: tx(20),
                    record,
                },
            )
            .unwrap();
            client.submit(&bytes);
        });
    }
    pub fn send_content(&mut self, r: &mut ContentEpochRegistry, record: ShellContentRecord) {
        self.drive(r, |client| {
            let bytes = encode_shell_file_transaction(
                client.header(shell_file_transaction_kind(&record).unwrap()),
                &ShellFileTransactionRecord {
                    transaction: tx(20),
                    record,
                },
            )
            .unwrap();
            client.submit(&bytes);
        });
    }
    pub fn candidate(&mut self, r: &mut ContentEpochRegistry) {
        self.drive(r, |client| {
            let b = begin();
            let c = chunk();
            let candidate = NativeContentCandidate {
                candidate: ContentCandidate {
                    grant: b.content.grant,
                    candidate_generation: b.content.candidate_generation,
                    output: b.content.output,
                    facts_generation: b.content.facts_generation,
                    pacing_permit: b.content.pacing_permit,
                    interaction_generation: b.content.interaction_generation,
                    surfaces: c.surfaces,
                    placements: c.placements,
                    targets: c.targets,
                },
                opening: b.opening,
                catalog_generation: b.catalog_generation,
                state_revision: b.state_revision,
                selected: b.selected,
                rows: b.rows,
            };
            let bytes = encode_shell_file_native_candidate(
                client.header(ShellFileKind::NativeCandidate),
                &ShellFileNativeCandidate {
                    transaction: tx(20),
                    candidate,
                },
            )
            .unwrap();
            client.submit(&bytes);
        });
    }
    pub fn connected(r: &mut ContentEpochRegistry) -> Self {
        let mut peer = Self::with_limits(r, ContentStoreProfile::NativeLauncher, limits());
        peer.negotiate(r);
        assert!(peer.transport.supports_native_launcher());
        assert!(!peer.transport.supports_indicators());
        assert!(!peer.transport.supports_launcher());
        peer.transport
            .publish_native_launcher_opening(r, tx(2), opening())
            .unwrap();
        assert_eq!(
            peer.read_native(r).1,
            ShellNativeLauncherRecord::Opening(opening())
        );
        peer
    }
    pub fn allocation(&mut self, r: &mut ContentEpochRegistry) -> Vec<ContentAllocationSnapshot> {
        self.transport
            .publish_content_output_facts(r, tx(1), 5, vec![facts()])
            .unwrap();
        assert!(matches!(
            self.read_content(r).1,
            ShellContentRecord::OutputFacts(_)
        ));
        self.send(r, ShellNativeLauncherRecord::AllocationRequest(request(1)));
        let c = catalog();
        assert_eq!(
            self.transport
                .service_native_launcher_content(r, context(&[]), native(&c), 0)
                .unwrap(),
            1
        );
        let pending = self.transport.next_content_allocation_request(r).unwrap();
        assert_eq!(pending.0, tx(20));
        assert_eq!(pending.1.role, 3);
        self.transport
            .grant_content_allocation(r, 1, allocation(), &[])
            .unwrap();
        let (_, ShellContentRecord::AllocationResult(result)) = self.read_content(r) else {
            panic!("allocation");
        };
        assert_eq!(result.status, 1);
        assert_eq!(result.allowed_reservation_extent, 0);
        vec![allocation()]
    }
    pub fn upload(&mut self, r: &mut ContentEpochRegistry) {
        self.drive(r, |client| {
            let bytes = encode_shell_file_resource_begin(
                client.header(ShellFileKind::ResourceBegin),
                &ShellFileResourceBegin {
                    transaction: tx(20),
                    slot: 0,
                    record: ShellContentRecord::ResourceBegin(ContentResourceBegin {
                        grant: GRANT,
                        resource: RESOURCE,
                        width_px: 2,
                        height_px: 1,
                        rendered_scale_numerator: 1,
                        rendered_scale_denominator: 1,
                        pixel_format: 1,
                        chunk_count: 1,
                        total_bytes: 8,
                    }),
                },
            )
            .unwrap();
            client.submit(&bytes);
        });
        assert_eq!(self.transport.service_content_resources(r, 0).unwrap(), 1);
        assert!(
            matches!(self.read_content(r).1, ShellContentRecord::ResourceStatus(v) if v.status == 1)
        );
        self.drive(r, |client| {
            assert_eq!(client.wire.open_path(10, &[b"upload", b"0"], 1).0, 13);
            assert_eq!(
                client
                    .wire
                    .write_at(10, 0, &[0, 0, 255, 255, 0, 128, 0, 128])
                    .0,
                119
            );
            // Keep the upload fid through End: clunking an unfinished upload
            // cancels it in the production export.
            let bytes = encode_shell_file_resource_end(
                client.header(ShellFileKind::ResourceEnd),
                &ShellFileTransactionRecord {
                    transaction: tx(20),
                    record: ShellContentRecord::ResourceEnd(ContentResourceEnd {
                        grant: GRANT,
                        resource: RESOURCE,
                        total_bytes: 8,
                        chunk_count: 1,
                    }),
                },
            )
            .unwrap();
            client.submit(&bytes);
        });
        assert_eq!(self.transport.service_content_resources(r, 0).unwrap(), 2);
        assert!(
            matches!(self.read_content(r).1, ShellContentRecord::ResourceStatus(v) if v.status == 2)
        );
        assert_eq!(
            self.transport
                .lease_content_resource(r, GRANT, RESOURCE)
                .unwrap()
                .bytes(),
            &[0, 0, 255, 255, 0, 128, 0, 128]
        );
    }
}

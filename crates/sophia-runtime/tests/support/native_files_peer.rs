//! Production file export with supplied protection and renderer completions.
use super::shell_file_peer;
#[path = "shell_required_image.rs"]
mod image;
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
#[allow(dead_code)]
#[path = "native_launcher_content.rs"]
mod support;
pub(crate) use support::*;
#[path = "native_files_client.rs"]
mod client;
use client::{Client, Observation};
#[path = "native_files_controls.rs"]
mod controls;

static NEXT: AtomicU64 = AtomicU64::new(0);
pub(crate) const CAPS: u64 = SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER
    | (1 << 13);

pub(crate) fn empty() -> ContentEpochRegistry {
    ContentEpochRegistry::new(64 * 1024 * 1024).unwrap()
}
pub(crate) struct Peer {
    pub transport: ShellComponentTransport,
    client: Client,
    limits: ContentLimits,
    image_owner: ShellIsbImageOwner,
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
        let image_owner = image::configure(&mut transport, &limits);
        transport
            .reserve_content_with_profile(r, limits.clone(), profile)
            .unwrap();
        let client = Client::new(transport.socket_path(), limits.grant.connection_epoch);
        Self {
            transport,
            client,
            limits,
            image_owner,
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
                image::setup(&mut client.wire);
                let offer = encode_shell_file_negotiate(
                    client.header(ShellFileKind::Negotiate),
                    ShellV1ClientHello {
                        minimum_revision: 9,
                        maximum_revision: 9,
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
            assert_eq!(welcome.selected_revision, 9);
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
        self.read_catalog_object(r).catalog.catalog
    }
    pub fn read_catalog_object(&mut self, r: &mut ContentEpochRegistry) -> ShellFileCatalog {
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
            let value = ShellFileTransactionRecord {
                transaction: tx(20),
                record,
            };
            let bytes = match &value.record {
                ShellContentRecord::ResourceRetire(_) => encode_shell_file_resource_retire(
                    client.header(ShellFileKind::ResourceRetire),
                    &value,
                ),
                ShellContentRecord::AllocationRequest(_) => encode_shell_file_allocation_request(
                    client.header(ShellFileKind::AllocationRequest),
                    value,
                ),
                _ => encode_shell_file_transaction(
                    client.header(shell_file_transaction_kind(&value.record).unwrap()),
                    &value,
                ),
            }
            .unwrap();
            client.submit(&bytes);
        });
    }
    pub fn candidate(&mut self, r: &mut ContentEpochRegistry) {
        self.candidate_parts(r, begin(), chunk());
    }
    pub fn candidate_parts(
        &mut self,
        r: &mut ContentEpochRegistry,
        b: NativeLauncherCandidateBegin,
        c: ContentCandidateChunk,
    ) {
        self.drive(r, |client| {
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
        Self::connected_with_limits(r, limits())
    }
    pub fn connected_with_limits(r: &mut ContentEpochRegistry, limits: ContentLimits) -> Self {
        let mut peer = Self::with_limits(r, ContentStoreProfile::NativeLauncher, limits);
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
    pub fn no_event(&mut self, r: &mut ContentEpochRegistry) {
        self.drive(r, Client::no_event);
    }
    pub fn shutdown(&mut self) {
        self.client.wire.shutdown();
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
        std::thread::scope(|scope| {
            let client = &mut self.client;
            let worker = scope.spawn(move || client.upload());
            let deadline = Instant::now() + Duration::from_secs(5);
            while !worker.is_finished() {
                self.transport.poll_io(r).unwrap();
                self.image_owner
                    .service(&mut self.transport, r, 0, Instant::now())
                    .unwrap();
                self.transport.service_content_resources(r, 0).unwrap();
                assert!(Instant::now() < deadline, "native sealed image hung");
                std::thread::yield_now();
            }
            worker.join().unwrap();
        });
        assert_eq!(
            self.transport
                .lease_content_resource(r, GRANT, RESOURCE)
                .unwrap()
                .bytes(),
            &[0, 0, 255, 255, 0, 128, 0, 128]
        );
    }
}

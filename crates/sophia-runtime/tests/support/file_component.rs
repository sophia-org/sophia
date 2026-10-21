//! One independently admitted component over `sophia_shell_fs_v1`: a real
//! `ShellComponentTransport` file export and the SDK's `connect_files`
//! client. Several may share one `ContentEpochRegistry`. After negotiation
//! the client is driven from the calling thread, interleaved with the owner's
//! service turns; the SDK's file I/O never blocks once connected.
#[allow(dead_code)]
#[path = "shell_required_image.rs"]
mod image;
#[path = "shell_file_peer.rs"]
mod shell_file_peer;
use sophia_shell_client::{ImageAdapterError, ImageChannelState, RingOfferState, RingUpload};
#[allow(dead_code)]
#[path = "shell_sdk_image.rs"]
mod sdk_image;
use sdk_image::authenticated_client;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sophia_protocol::*;
use sophia_runtime::*;
use sophia_shell_client::{ShellClientError, ShellClientOptions, ShellConnection};

pub const MIB: u64 = 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

pub const CONTENT: u64 =
    SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE;

pub fn evidence() -> ProtectionDomainEvidence {
    ProtectionDomainEvidence {
        backend: ProtectionBackendKind::Bubblewrap,
        supervisor_pid: std::process::id(),
        peer_pid: std::process::id(),
        roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
    }
}

pub fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: false,
    }
}

pub fn options(capabilities: u64) -> ShellClientOptions {
    ShellClientOptions {
        minimum_revision: if capabilities & (1 << 15) != 0 { 10 } else { 5 },
        maximum_revision: if capabilities & (1 << 15) != 0 { 10 } else { 6 },
        required_capabilities: capabilities,
        handshake_timeout: Duration::from_secs(2),
    }
}

pub fn next_record(client: &mut ShellConnection) -> ShellContentRecord {
    let start = Instant::now();
    loop {
        if let Some((_, record)) = client.poll_content().unwrap() {
            return record;
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "no content record"
        );
        std::thread::yield_now();
    }
}

pub struct Component {
    pub transport: ShellComponentTransport,
    pub client: Option<ShellConnection>,
    directory: std::path::PathBuf,
    image_owner: Option<ShellIsbImageOwner>,
    pending_image: Option<(ContentGrant, u64, u8)>,
}

impl Component {
    pub fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "shell-file-component-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut transport = ShellComponentTransport::bind_for_supervised_uid(
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        transport.authorize_protected_peer(&evidence()).unwrap();
        Self {
            transport,
            client: None,
            directory,
            image_owner: None,
            pending_image: None,
        }
    }

    /// Negotiate one client with `capabilities` and drive the owner until the
    /// client thread ends. The owner's negotiation result is returned beside
    /// the client's own connect result and its first content record.
    #[allow(clippy::type_complexity)]
    pub fn negotiate(
        &mut self,
        registry: &mut ContentEpochRegistry,
        epoch: u64,
        capabilities: u64,
    ) -> (
        Result<ShellV1ServerWelcome, ShellTransportError>,
        Result<(ShellConnection, ShellContentRecord), ShellClientError>,
    ) {
        let socket = self.transport.socket_path().to_owned();
        let peer = std::thread::spawn(move || {
            let mut client = if capabilities & (1 << 15) != 0 {
                authenticated_client(&socket, options(capabilities))?
            } else {
                ShellConnection::connect_files(socket, options(capabilities))?
            };
            let record = next_record(&mut client);
            Ok((client, record))
        });
        let start = Instant::now();
        let owner = self
            .transport
            .begin_file_negotiation(registry, epoch, Duration::from_secs(2), granted())
            .and_then(|()| {
                loop {
                    if let Some(welcome) = self.transport.poll_negotiation(registry, 64 * 1024)? {
                        break Ok(welcome);
                    }
                    assert!(start.elapsed() < Duration::from_secs(5), "negotiation hung");
                    std::thread::yield_now();
                }
            });
        // The client fetches Limits after Negotiated; keep serving the lane.
        while owner.is_ok() && !peer.is_finished() {
            self.transport.poll_io(registry).unwrap();
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "limits fetch hung"
            );
            std::thread::yield_now();
        }
        let client = peer
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        (owner, client)
    }

    /// The socket fixture's `connect`: the welcome epoch, the exact first
    /// Limits record and the owner's retained limits all match `expected`.
    pub fn connect(&mut self, registry: &mut ContentEpochRegistry, expected: ContentLimits) {
        self.image_owner = Some(image::configure(&mut self.transport, &expected));
        let (welcome, client) = self.negotiate(
            registry,
            expected.grant.connection_epoch,
            CONTENT | (1 << 15),
        );
        let welcome = welcome.unwrap();
        let (mut client, received) = client.unwrap();
        assert_eq!(welcome.connection_epoch, expected.grant.connection_epoch);
        assert_eq!(received, ShellContentRecord::Limits(expected.clone()));
        assert_eq!(self.transport.content_limits(), Some(&expected));
        sdk_image::enable(&mut client, &self.directory, &expected);
        self.client = Some(client);
        image::supply_allocation(registry, expected.grant);
        let start = Instant::now();
        while self
            .client
            .as_ref()
            .unwrap()
            .image_adapter()
            .unwrap()
            .channel()
            != Some(ImageChannelState::Ready)
        {
            self.service_image(registry, 0);
            assert!(start.elapsed() < Duration::from_secs(5));
        }
    }

    pub fn send_upload(&mut self, grant: ContentGrant, id: u64, value: u8) {
        assert!(self.pending_image.replace((grant, id, value)).is_none());
    }

    fn service_image(&mut self, registry: &mut ContentEpochRegistry, now: u64) {
        self.client.as_mut().unwrap().poll_io().unwrap();
        self.transport
            .service_content_resources(registry, now)
            .unwrap();
        self.image_owner
            .as_mut()
            .unwrap()
            .service(&mut self.transport, registry, now, Instant::now())
            .unwrap();
    }

    pub fn uploaded(
        &mut self,
        registry: &mut ContentEpochRegistry,
        grant: ContentGrant,
        id: u64,
        now: u64,
    ) -> ContentResourceLease {
        let (sent_grant, sent_id, value) = self.pending_image.take().unwrap();
        assert_eq!((sent_grant, sent_id), (grant, id));
        self.upload_pixels(registry, grant, id, 1, 1, &[value, 0, 0, 255], now)
    }

    pub fn upload_pixels(
        &mut self,
        registry: &mut ContentEpochRegistry,
        grant: ContentGrant,
        id: u64,
        width: u32,
        height: u32,
        pixels: &[u8],
        now: u64,
    ) -> ContentResourceLease {
        let metadata = RingUpload {
            resource: id,
            resource_generation: 1,
            output: 1,
            output_generation: 1,
            allocation: 1,
            allocation_generation: 1,
            topology_generation: 1,
            width,
            height,
        };
        let start = Instant::now();
        let offer = loop {
            self.service_image(registry, now);
            match self
                .client
                .as_mut()
                .unwrap()
                .image_upload_ring(metadata, pixels, Instant::now())
            {
                Ok(offer) => break offer,
                Err(ImageAdapterError::Again | ImageAdapterError::Busy) => {}
                Err(error) => panic!("ring upload refused: {error:?}"),
            }
            assert!(start.elapsed() < Duration::from_secs(5), "ring not ready");
            std::thread::yield_now();
        };
        let mut accepted = false;
        loop {
            self.service_image(registry, now);
            if let Some((_, record)) = self.client.as_mut().unwrap().poll_content().unwrap() {
                let ShellContentRecord::ResourceStatus(status) = record else {
                    panic!("resource status: {record:?}")
                };
                assert_eq!(status.grant, grant);
                assert_eq!(status.resource, ContentResourceId { id, generation: 1 });
                assert_eq!(status.status, 2);
                accepted = true;
            }
            let info = self
                .client
                .as_ref()
                .unwrap()
                .image_ring_offer(offer)
                .unwrap();
            if accepted && info.state == RingOfferState::Settled {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "image did not settle: {info:?}"
            );
            std::thread::yield_now();
        }
        self.client.as_mut().unwrap().image_forget(offer).unwrap();
        self.transport
            .lease_content_resource(registry, grant, ContentResourceId { id, generation: 1 })
            .unwrap()
    }

    pub fn disconnect(&mut self, registry: &mut ContentEpochRegistry) {
        self.transport.disconnect(registry).unwrap();
        self.client = None;
        if let Some(owner) = &mut self.image_owner {
            // The source remains charged until its persistent owner observes
            // the disconnected epoch, even when no copy jobs are pending.
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                owner
                    .service(&mut self.transport, registry, 0, Instant::now())
                    .unwrap();
                if owner.pending() == 0 {
                    break;
                }
                assert!(Instant::now() < deadline, "image owner did not drain");
                std::thread::yield_now();
            }
        }
        registry.collect();
    }
}

impl Drop for Component {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

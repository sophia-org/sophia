//! One independently admitted component over `sophia_shell_fs_v1`: a real
//! `ShellComponentTransport` file export and the SDK's `connect_files`
//! client. Several may share one `ContentEpochRegistry`. After negotiation
//! the client is driven from the calling thread, interleaved with the owner's
//! service turns; the SDK's file I/O never blocks once connected.
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
        minimum_revision: 5,
        maximum_revision: 6,
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
            let mut client = ShellConnection::connect_files(socket, options(capabilities))?;
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
        let (welcome, client) = self.negotiate(registry, expected.grant.connection_epoch, CONTENT);
        let welcome = welcome.unwrap();
        let (client, received) = client.unwrap();
        assert_eq!(welcome.connection_epoch, expected.grant.connection_epoch);
        assert_eq!(received, ShellContentRecord::Limits(expected.clone()));
        assert_eq!(self.transport.content_limits(), Some(&expected));
        self.client = Some(client);
    }

    pub fn send_upload(&mut self, grant: ContentGrant, id: u64, value: u8) {
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

    /// Serve until the client holds both statuses for its own grant, then
    /// lease the stored pixels.
    pub fn uploaded(
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
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::yield_now();
        }
        assert_eq!(received, [1, 2]);
        self.transport
            .lease_content_resource(registry, grant, ContentResourceId { id, generation: 1 })
            .unwrap()
    }

    pub fn disconnect(&mut self, registry: &mut ContentEpochRegistry) {
        self.transport.disconnect(registry).unwrap();
        self.client = None;
    }
}

impl Drop for Component {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

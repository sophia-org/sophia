//! Real private socket and content owners, supplied protection/geometry/catalog
//! and renderer completions, including transport-owned focus/input transitions.
//! No supervised child, physical input, application launch or native display.
use sophia_protocol::*;
use sophia_runtime::*;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
#[allow(dead_code)] // Shared store fixture also serves the direct ownership suite.
#[path = "native_launcher_content.rs"]
mod support;
pub(crate) use support::*;

pub(crate) const CAPS: u64 = SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER;
static NEXT: AtomicU64 = AtomicU64::new(1);
pub(crate) struct Peer {
    pub(crate) transport: ShellComponentTransport,
    pub(crate) client: UnixStream,
    pub(crate) directory: std::path::PathBuf,
}
impl Peer {
    pub(crate) fn new(r: &mut ContentEpochRegistry, profile: ContentStoreProfile) -> Self {
        Self::with_limits(r, profile, limits())
    }
    pub(crate) fn with_limits(
        r: &mut ContentEpochRegistry,
        profile: ContentStoreProfile,
        limits: ContentLimits,
    ) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "native-wire-{}-{}",
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
            .reserve_content_with_profile(r, limits, profile)
            .unwrap();
        let client = UnixStream::connect(transport.socket_path()).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        Self {
            transport,
            client,
            directory,
        }
    }
    pub(crate) fn negotiate(
        &mut self,
        r: &mut ContentEpochRegistry,
        hello: ShellV1ClientHello,
        policy: ShellContentAdmissionPolicy,
    ) -> Result<ShellV1ServerWelcome, ShellTransportError> {
        self.transport.begin_negotiation(
            r,
            GRANT.connection_epoch,
            Duration::from_secs(2),
            policy,
        )?;
        self.client
            .write_all(&encode_shell_v1_client_hello_frame(hello).unwrap())
            .unwrap();
        // Tiny visits also exercise retained handshake partial-write state.
        for _ in 0..2048 {
            if let Some(welcome) = self.transport.poll_negotiation(r, 7)? {
                return Ok(welcome);
            }
        }
        panic!("negotiation did not finish within bounded visits")
    }
    pub(crate) fn connected(r: &mut ContentEpochRegistry) -> Self {
        Self::connected_with_limits(r, limits())
    }
    pub(crate) fn connected_with_limits(
        r: &mut ContentEpochRegistry,
        limits: ContentLimits,
    ) -> Self {
        let mut peer = Self::with_limits(r, ContentStoreProfile::NativeLauncher, limits.clone());
        let welcome = peer.negotiate(r, hello(), granted()).unwrap();
        assert_eq!(welcome.selected_revision, 7);
        assert_eq!(welcome.capabilities, CAPS);
        assert!(peer.transport.supports_native_launcher());
        assert!(!peer.transport.supports_indicators());
        assert!(!peer.transport.supports_launcher());
        assert_eq!(
            decode_shell_v1_server_welcome_frame(&peer.read()).unwrap(),
            welcome
        );
        assert_eq!(
            decode_shell_content_frame(&peer.read()).unwrap().1,
            ShellContentRecord::Limits(limits)
        );
        peer.transport
            .publish_native_launcher_opening(r, tx(2), opening())
            .unwrap();
        peer.transport.poll_io(r).unwrap();
        assert_eq!(
            decode_shell_native_launcher_frame(&peer.read()).unwrap().1,
            ShellNativeLauncherRecord::Opening(opening())
        );
        peer
    }
    pub(crate) fn read(&mut self) -> Vec<u8> {
        let mut bytes = vec![0; SOPHIA_IPC_HEADER_LEN];
        self.client.read_exact(&mut bytes).unwrap();
        let n = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) as usize;
        assert!(n <= 65536);
        bytes.resize(SOPHIA_IPC_HEADER_LEN + n, 0);
        self.client
            .read_exact(&mut bytes[SOPHIA_IPC_HEADER_LEN..])
            .unwrap();
        bytes
    }
    pub(crate) fn send(&mut self, record: ShellNativeLauncherRecord) {
        self.client
            .write_all(&encode_shell_native_launcher_frame(tx(20), &record).unwrap())
            .unwrap();
    }
    pub(crate) fn send_content(&mut self, record: ShellContentRecord) {
        self.client
            .write_all(&encode_shell_content_frame(tx(20), &record).unwrap())
            .unwrap();
    }
    pub(crate) fn allocation(
        &mut self,
        r: &mut ContentEpochRegistry,
    ) -> Vec<ContentAllocationSnapshot> {
        self.transport
            .publish_content_output_facts(r, tx(1), 5, vec![facts()])
            .unwrap();

        self.transport.poll_io(r).unwrap();
        assert!(matches!(
            decode_shell_content_frame(&self.read()).unwrap().1,
            ShellContentRecord::OutputFacts(_)
        ));

        self.send(ShellNativeLauncherRecord::AllocationRequest(request(1)));
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
        self.transport.poll_io(r).unwrap();
        let (_, ShellContentRecord::AllocationResult(result)) =
            decode_shell_content_frame(&self.read()).unwrap()
        else {
            panic!()
        };
        assert_eq!(result.status, 1);
        assert_eq!(result.allowed_reservation_extent, 0);
        vec![allocation()]
    }
    pub(crate) fn upload(&mut self, r: &mut ContentEpochRegistry) {
        for record in [
            ShellContentRecord::ResourceBegin(ContentResourceBegin {
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
            ShellContentRecord::ResourceChunk(ContentResourceChunk {
                grant: GRANT,
                resource: RESOURCE,
                ordinal: 0,
                offset: 0,
                bytes: vec![0, 0, 255, 255, 0, 128, 0, 128],
            }),
            ShellContentRecord::ResourceEnd(ContentResourceEnd {
                grant: GRANT,
                resource: RESOURCE,
                total_bytes: 8,
                chunk_count: 1,
            }),
        ] {
            self.send_content(record);
        }
        let c = catalog();
        assert_eq!(
            self.transport
                .service_native_launcher_content(r, context(&[allocation()]), native(&c), 0)
                .unwrap(),
            3
        );
        self.transport.poll_io(r).unwrap();
        for expected in [1, 2] {
            assert!(
                matches!(decode_shell_content_frame(&self.read()).unwrap().1,ShellContentRecord::ResourceStatus(v) if v.status==expected)
            );
        }
    }
    pub(crate) fn permit(&mut self, r: &mut ContentEpochRegistry) {
        self.send_content(ShellContentRecord::FrameDemand(ContentFrameDemand {
            grant: GRANT,
            output: OUTPUT,
            allocation: ALLOCATION,
            demand_id: 1,
            reason: 1,
        }));
        let c = catalog();
        assert_eq!(
            self.transport
                .service_native_launcher_content(r, context(&[allocation()]), native(&c), 0)
                .unwrap(),
            1
        );
        assert_eq!(
            self.transport.next_content_demand(r).unwrap().1.demand_id,
            1
        );
        self.transport
            .grant_content_demand(r, tx(3), OUTPUT, 1, 0)
            .unwrap();
        self.transport.poll_io(r).unwrap();
        assert!(matches!(
            decode_shell_content_frame(&self.read()).unwrap().1,
            ShellContentRecord::FramePermit(_)
        ));
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
pub(crate) fn hello() -> ShellV1ClientHello {
    ShellV1ClientHello {
        minimum_revision: 7,
        maximum_revision: 7,
        required_capabilities: CAPS,
    }
}
pub(crate) fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    }
}
pub(crate) fn empty() -> ContentEpochRegistry {
    ContentEpochRegistry::new(64 * 1024 * 1024).unwrap()
}

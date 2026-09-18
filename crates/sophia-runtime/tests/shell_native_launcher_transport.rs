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
#[path = "support/native_launcher_content.rs"]
mod support;
use support::*;

const CAPS: u64 = SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG
    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
    | SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT
    | SOPHIA_SHELL_CAPABILITY_NATIVE_LAUNCHER;
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Peer {
    transport: ShellComponentTransport,
    client: UnixStream,
    directory: std::path::PathBuf,
}
impl Peer {
    fn new(r: &mut ContentEpochRegistry, profile: ContentStoreProfile) -> Self {
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
            .reserve_content_with_profile(r, limits(), profile)
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
    fn negotiate(
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
    fn connected(r: &mut ContentEpochRegistry) -> Self {
        let mut peer = Self::new(r, ContentStoreProfile::NativeLauncher);
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
            ShellContentRecord::Limits(limits())
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
    fn read(&mut self) -> Vec<u8> {
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
    fn send(&mut self, record: ShellNativeLauncherRecord) {
        self.client
            .write_all(&encode_shell_native_launcher_frame(tx(20), &record).unwrap())
            .unwrap();
    }
    fn send_content(&mut self, record: ShellContentRecord) {
        self.client
            .write_all(&encode_shell_content_frame(tx(20), &record).unwrap())
            .unwrap();
    }
    fn allocation(&mut self, r: &mut ContentEpochRegistry) -> Vec<ContentAllocationSnapshot> {
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
    fn upload(&mut self, r: &mut ContentEpochRegistry) {
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
    fn permit(&mut self, r: &mut ContentEpochRegistry) {
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
fn hello() -> ShellV1ClientHello {
    ShellV1ClientHello {
        minimum_revision: 7,
        maximum_revision: 7,
        required_capabilities: CAPS,
    }
}
fn granted() -> ShellContentAdmissionPolicy {
    ShellContentAdmissionPolicy::Granted {
        discrete_input: true,
    }
}
fn empty() -> ContentEpochRegistry {
    ContentEpochRegistry::new(64 * 1024 * 1024).unwrap()
}

#[test]
fn native_socket_assembles_exact_catalog_candidate_and_keeps_source_until_retirement() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    peer.upload(&mut r);
    peer.permit(&mut r);
    peer.send(ShellNativeLauncherRecord::CandidateBegin(begin()));
    peer.send(ShellNativeLauncherRecord::CandidateChunk(chunk()));
    peer.send_content(ShellContentRecord::CandidateEnd(end()));
    let c = catalog();
    assert_eq!(
        peer.transport
            .connection(&mut r)
            .service_native_launcher_content(context(&allocations), native(&c), 0)
            .unwrap(),
        3
    );
    let bundle = peer
        .transport
        .connection(&mut r)
        .begin_native_launcher_submission(1, context(&allocations), native(&c), 0)
        .unwrap();
    assert_eq!(bundle.native_launcher.unwrap().rows(), &[2, 1]);
    assert_eq!(
        bundle.resource(RESOURCE).unwrap().bytes(),
        &[0, 0, 255, 255, 0, 128, 0, 128]
    );
    peer.transport
        .content_prepared(&mut r, GRANT, OUTPUT, 1, 1, 1, 0)
        .unwrap();
    peer.transport
        .content_presented(&mut r, GRANT, OUTPUT, 1, 9, 1, 1)
        .unwrap();
    peer.transport.poll_io(&mut r).unwrap();
    for expected in [1, 2] {
        let (transaction, ShellContentRecord::CandidateOutcome(outcome)) =
            decode_shell_content_frame(&peer.read()).unwrap()
        else {
            panic!()
        };
        assert_eq!(transaction, tx(20));
        assert_eq!(outcome.kind, expected);
        assert_eq!(outcome.candidate_generation, 1);
    }
    peer.transport.disconnect(&mut r).unwrap();
    r.collect();
    assert_eq!(r.accounting().memory.resident, 8);
    assert_eq!(r.accounting().retired_epochs, 1);
    drop(bundle);
    r.collect();
    assert_eq!(r.accounting().retired_epochs, 0);
}

#[test]
fn role_reservation_cannot_be_selected_or_widened_by_peer_capabilities() {
    for mode in 0..4 {
        let mut r = empty();
        let mut peer = Peer::new(
            &mut r,
            if mode == 0 {
                ContentStoreProfile::Legacy
            } else {
                ContentStoreProfile::NativeLauncher
            },
        );
        let mut request = hello();
        if mode == 1 {
            request.required_capabilities |= SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER;
        }
        if mode == 2 {
            request.maximum_revision = 6;
            request.minimum_revision = 5;
        }
        if mode == 3 {
            request.required_capabilities &= !SOPHIA_SHELL_CAPABILITY_CONTENT_DISCRETE_INPUT;
        }
        assert!(peer.negotiate(&mut r, request, granted()).is_err());
        assert!(!peer.transport.supports_native_launcher());
        assert!(peer.transport.content_limits().is_none());
        assert_eq!(r.reserved_bytes(), 0);
    }
}

#[test]
fn operator_denial_is_an_encoded_refusal_and_retires_exact_reservation() {
    for (policy, reason) in [
        (ShellContentAdmissionPolicy::Unavailable, 4),
        (ShellContentAdmissionPolicy::Denied, 1),
        (
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
            1,
        ),
    ] {
        let mut r = empty();
        let mut peer = Peer::new(&mut r, ContentStoreProfile::NativeLauncher);
        assert!(
            matches!(peer.negotiate(&mut r,hello(),policy),Err(ShellTransportError::ContentAdmissionRefused(v)) if v.reason==reason)
        );
        assert!(
            matches!(decode_shell_content_frame(&peer.read()).unwrap().1,ShellContentRecord::AdmissionRefused(v) if v.reason==reason && v.denied_capabilities==CAPS)
        );
        assert_eq!(r.reserved_bytes(), 0);
    }
}

#[test]
fn stale_catalog_at_end_rejects_one_owned_candidate_over_real_fifo() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    peer.upload(&mut r);
    peer.permit(&mut r);
    peer.send(ShellNativeLauncherRecord::CandidateBegin(begin()));
    peer.send(ShellNativeLauncherRecord::CandidateChunk(chunk()));
    let mut c = catalog();
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        2
    );
    c.generation += 1;
    peer.send_content(ShellContentRecord::CandidateEnd(end()));
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        1
    );
    peer.transport.poll_io(&mut r).unwrap();
    let (transaction, ShellContentRecord::CandidateOutcome(v)) =
        decode_shell_content_frame(&peer.read()).unwrap()
    else {
        panic!()
    };
    assert_eq!(transaction, tx(20));
    assert_eq!(v.kind, 3);
    assert_eq!(v.reason, ContentReason::Stale as u16);
    assert!(peer.transport.next_content_submission(&r).is_none());
}

#[test]
fn wrong_grant_and_legacy_allocation_refuse_before_ownership_change() {
    for wrong_grant in [true, false] {
        let mut r = empty();
        let mut peer = Peer::connected(&mut r);
        if wrong_grant {
            let mut request = request(1);
            request.grant.connection_epoch += 1;
            peer.send(ShellNativeLauncherRecord::AllocationRequest(request));
        } else {
            let mut req = request(1);
            req.opening = 7;
            let legacy = ContentAllocationRequest {
                grant: GRANT,
                output: OUTPUT,
                allocation_request_id: 1,
                operation: 1,
                role: 1,
                edge: 1,
                prior: ContentAllocationId::default(),
                parent: ContentAllocationId::default(),
                parent_presentation_epoch: 0,
                anchor_parent_rect: ContentPixelRect::default(),
                desired_width: req.desired_width,
                desired_height: req.desired_height,
                margins: req.margins,
            };
            peer.send_content(ShellContentRecord::AllocationRequest(legacy));
        }
        let c = catalog();
        for _ in 0..2 {
            assert_eq!(
                peer.transport
                    .service_native_launcher_content(&mut r, context(&[]), native(&c), 0),
                Err(if wrong_grant {
                    ShellTransportError::WrongContentGrant
                } else {
                    ShellTransportError::WrongContentRecord
                })
            );
        }
        assert!(peer.transport.next_content_allocation_request(&r).is_none());
    }
}

#[test]
fn native_visit_shares_record_budget_across_resources_and_demands() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let allocations = peer.allocation(&mut r);
    for demand_id in 1..=17 {
        // First record is resource traffic, the others are coalesced pacing.
        if demand_id == 1 {
            peer.send_content(ShellContentRecord::ResourceRetire(ContentResourceRetire {
                grant: GRANT,
                resource: RESOURCE,
            }));
        } else {
            peer.send_content(ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant: GRANT,
                output: OUTPUT,
                allocation: ALLOCATION,
                demand_id,
                reason: 1,
            }));
        }
    }
    let c = catalog();
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        16
    );
    assert_eq!(
        peer.transport.next_content_demand(&r).unwrap().1.demand_id,
        16
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&allocations), native(&c), 0)
            .unwrap(),
        1
    );
    assert_eq!(
        peer.transport.next_content_demand(&r).unwrap().1.demand_id,
        17
    );
}

#[test]
fn buffered_native_visit_stops_at_payload_budget_and_resumes_exact_record() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let c = catalog();
    for ordinal in 0..2 {
        peer.send_content(ShellContentRecord::ResourceChunk(ContentResourceChunk {
            grant: GRANT,
            resource: RESOURCE,
            ordinal,
            offset: u64::from(ordinal) * 32768,
            bytes: vec![0; 32768],
        }));
    }
    // Supply an already-buffered pair so I/O's own bound cannot mask a broken
    // dispatch byte budget. Unknown resources produce ordinary typed refusals.
    peer.transport.poll_io(&mut r).unwrap();
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        1
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        1
    );
    peer.transport.poll_io(&mut r).unwrap();
    for _ in 0..2 {
        assert!(
            matches!(decode_shell_content_frame(&peer.read()).unwrap().1,ShellContentRecord::ResourceStatus(v) if v.status==3)
        );
    }
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        0
    );
}

#[test]
fn peer_eof_reports_disconnect_after_buffered_native_request_is_owned() {
    let mut r = empty();
    let mut peer = Peer::connected(&mut r);
    let c = catalog();
    peer.transport
        .publish_content_output_facts(&mut r, tx(1), 5, vec![facts()])
        .unwrap();
    peer.send(ShellNativeLauncherRecord::AllocationRequest(request(1)));
    peer.client.shutdown(std::net::Shutdown::Write).unwrap();
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0)
            .unwrap(),
        1
    );
    assert_eq!(
        peer.transport
            .next_content_allocation_request(&r)
            .unwrap()
            .1
            .allocation_request_id,
        1
    );
    assert_eq!(
        peer.transport
            .service_native_launcher_content(&mut r, context(&[]), native(&c), 0),
        Err(ShellTransportError::NotConnected)
    );
    assert!(peer.transport.next_content_allocation_request(&r).is_some());
    peer.transport.disconnect(&mut r).unwrap();
    r.collect();
    assert!(r.accounting().quiescent());
}

#[path = "support/native_launcher_focus.rs"]
mod focus;

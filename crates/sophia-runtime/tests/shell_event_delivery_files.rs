//! Proof-only counts for wire-wake design 516tpvf5 (W1): which `Rread`
//! carries a record's `Submitted` and which its domain reply, at the current
//! export ordering. A raw peer parks one event read before submitting, as the
//! SDK keeps exactly one outstanding. No export behaviour changes here.
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

static NEXT: AtomicU64 = AtomicU64::new(0);
const OUTPUT: ContentOutputId = ContentOutputId {
    id: 2,
    generation: 1,
};

fn transport() -> (ShellComponentTransport, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!(
        "shell-event-delivery-{}-{}",
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
    (transport, directory)
}

fn header(kind: ShellFileKind, id: u64) -> ShellFileHeader {
    ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id: id,
        sequence: 0,
    }
}

/// Raw 9P on a clone of the peer's socket, with tags no `Peer` call uses.
struct Raw(UnixStream);

impl Raw {
    fn send(&mut self, kind: u8, tag: u16, body: &[u8]) {
        let mut frame = ((7 + body.len()) as u32).to_le_bytes().to_vec();
        frame.push(kind);
        frame.extend(tag.to_le_bytes());
        frame.extend(body);
        self.0.write_all(&frame).unwrap();
    }

    fn read_events(&mut self, tag: u16, offset: u64) {
        let body = [
            2u32.to_le_bytes().as_slice(),
            &offset.to_le_bytes(),
            &65500u32.to_le_bytes(),
        ]
        .concat();
        self.send(116, tag, &body);
    }

    /// One reply frame: (type, tag, body).
    fn reply(&mut self) -> (u8, u16, Vec<u8>) {
        let mut head = [0; 7];
        self.0.read_exact(&mut head).unwrap();
        let size = u32::from_le_bytes(head[..4].try_into().unwrap()) as usize;
        let mut body = vec![0; size - 7];
        self.0.read_exact(&mut body).unwrap();
        (
            head[4],
            u16::from_le_bytes(head[5..].try_into().unwrap()),
            body,
        )
    }
}

/// Event kinds and sequences in one `Rread` body.
fn events(body: &[u8]) -> Vec<(ShellFileKind, u64)> {
    let mut rest = &body[4..];
    let mut events = Vec::new();
    while !rest.is_empty() {
        let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        let record =
            decode_shell_file_record_at_revision(&rest[..size], ShellFileClass::Event, 8).unwrap();
        events.push((record.header.kind, record.header.sequence));
        rest = &rest[size..];
    }
    events
}

/// What the peer saw for one FrameDemand submitted under a parked read.
#[derive(Debug, Default)]
struct Delivery {
    /// The parked read's events, answered alongside the submit.
    first: Vec<(ShellFileKind, u64)>,
    /// Events of the re-armed read, if the reply was not in the first.
    second: Vec<(ShellFileKind, u64)>,
}

/// Negotiates, parks one event read, submits a FrameDemand and collects its
/// events. The owner grants the demand in the same phase that received it
/// when `grant` is set, as the live content service does.
fn deliver(grant: bool) -> Delivery {
    let mut registry = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    let (mut transport, directory) = transport();
    let limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    });
    let content_grant = limits.grant;
    transport.reserve_content(&mut registry, limits).unwrap();
    let socket = transport.socket_path().to_owned();
    let (done_tx, done_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let stream = UnixStream::connect(&socket).unwrap();
        let mut raw = Raw(stream.try_clone().unwrap());
        let mut peer = Peer::from_stream(stream);
        peer.setup();
        let offer = encode_shell_file_negotiate(
            header(ShellFileKind::Negotiate, 1),
            ShellV1ClientHello {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let negotiated = peer.next_event();
        peer.ack(&negotiated);
        let demand = encode_shell_file_transaction(
            header(ShellFileKind::FrameDemand, 2),
            &ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(9),
                record: ShellContentRecord::FrameDemand(ContentFrameDemand {
                    grant: content_grant,
                    output: OUTPUT,
                    allocation: ContentAllocationId::default(),
                    demand_id: 1,
                    reason: 1,
                }),
            },
        )
        .unwrap();
        // The SDK's one outstanding read, parked before the record goes.
        let mut offset = peer.event_offset();
        raw.read_events(1001, offset);
        peer.open(5, b"transaction", 2);
        assert_eq!(peer.write(5, &demand).0, 119);
        let submit = encode_shell_file_submit(ShellFileSubmit {
            connection_epoch: 1,
            submission_id: 2,
            candidate_bytes: demand.len() as u32,
        })
        .unwrap();
        raw.send(
            118,
            1002,
            &[
                3u32.to_le_bytes().as_slice(),
                &0u64.to_le_bytes(),
                &(submit.len() as u32).to_le_bytes(),
                &submit,
            ]
            .concat(),
        );
        let mut delivery = Delivery::default();
        let mut submitted = false;
        while !submitted || delivery.first.is_empty() {
            match raw.reply() {
                (119, 1002, _) => submitted = true,
                (117, 1001, body) => {
                    offset += body.len() as u64 - 4;
                    delivery.first = events(&body);
                }
                other => panic!("unexpected reply {:?}", (other.0, other.1)),
            }
        }
        if grant
            && !delivery
                .first
                .iter()
                .any(|(kind, _)| *kind == ShellFileKind::FramePermit)
        {
            // The re-arm: one more client write to learn the reply.
            raw.read_events(1003, offset);
            let (kind, tag, body) = raw.reply();
            assert_eq!((kind, tag), (117, 1003));
            delivery.second = events(&body);
        }
        done_tx.send(()).unwrap();
        // Hold the socket until the owner stops polling it.
        release_rx.recv().unwrap();
        delivery
    });

    transport
        .begin_file_negotiation(&registry, 1, Duration::from_secs(2), {
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            }
        })
        .unwrap();
    let start = Instant::now();
    while transport
        .poll_negotiation(&mut registry, 64 * 1024)
        .unwrap()
        .is_none()
    {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    let mut granted = false;
    while done_rx.try_recv().is_err() {
        // One synchronous owner phase: receive, then grant, then flush.
        transport.poll_io(&mut registry).unwrap();
        transport
            .service_content_demands(&mut registry, &[OUTPUT], &[])
            .unwrap();
        if grant && !granted && transport.next_content_demand(&registry).is_some() {
            transport
                .grant_content_demand(&mut registry, TransactionId::from_raw(15), OUTPUT, 7, 10)
                .unwrap();
            transport.poll_io(&mut registry).unwrap();
            granted = true;
        }
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::yield_now();
    }
    release_tx.send(()).unwrap();
    let delivery = peer.join().unwrap();
    transport.disconnect(&mut registry).unwrap();
    registry.collect();
    std::fs::remove_dir_all(directory).unwrap();
    delivery
}

/// Case 1 at the current ordering: the parked read leaves with `Submitted`
/// alone, before the same owner phase grants the permit, so learning the
/// permit costs one more client read. W1 would make this one `Rread`.
#[test]
fn a_same_phase_reply_needs_a_second_read_after_submitted() {
    let delivery = deliver(true);
    let [(ShellFileKind::Submitted, submitted)] = delivery.first[..] else {
        panic!("parked read carried {:?}", delivery.first);
    };
    assert_eq!(
        delivery.second,
        [(ShellFileKind::FramePermit, submitted + 1)],
        "the permit is the next event, in its own read"
    );
}

/// Case 2: with no domain reply at all, `Submitted` still leaves at once.
/// Any deferral must keep this.
#[test]
fn submitted_without_any_reply_is_delivered_to_the_parked_read() {
    let delivery = deliver(false);
    assert!(matches!(
        delivery.first[..],
        [(ShellFileKind::Submitted, _)]
    ));
    assert!(delivery.second.is_empty());
}

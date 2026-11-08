//! Liveness poll counts on a real connected wire: a blind input turn, an
//! observed quiet turn, an observed turn that answers a request and a
//! publication flush each land their counts in their own entry rows, and
//! nothing lands outside. Peer replies prove the owner still processed the
//! request; served here means owner processing, not peer receipt.
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sophia_protocol::shell_files::*;
use sophia_protocol::*;
use sophia_runtime::liveness_counts::{self, Boundary, Entry, Field, Snapshot};
use sophia_runtime::*;

#[path = "support/shell_file_peer.rs"]
mod shell_file_peer;
use shell_file_peer::Peer;

const WAIT: Duration = Duration::from_secs(5);

/// Per-cell deltas between two snapshots of this thread's grid.
struct Delta(Snapshot, Snapshot);

impl Delta {
    fn since(before: Snapshot) -> Self {
        Self(before, liveness_counts::snapshot())
    }
    fn get(&self, entry: Entry, boundary: Boundary, field: Field) -> u64 {
        self.1.get(entry, boundary).get(field) - self.0.get(entry, boundary).get(field)
    }
    fn total(&self, field: Field) -> u64 {
        self.1.total(field) - self.0.total(field)
    }
    fn entry_total(&self, entry: Entry, field: Field) -> u64 {
        Boundary::ALL
            .iter()
            .map(|&b| self.get(entry, b, field))
            .sum()
    }
    fn print(&self, label: &str) {
        for (entry, boundary, _) in self.1.rows() {
            let cells: Vec<String> = Field::ALL
                .iter()
                .map(|&f| format!("{}={}", f.name(), self.get(entry, boundary, f)))
                .collect();
            if cells.iter().any(|c| !c.ends_with("=0")) {
                println!(
                    "{label}: {} {} {}",
                    entry.name(),
                    boundary.name(),
                    cells.join(" ")
                );
            }
        }
    }
    /// One input turn and no flush: the turn's pre and post export polls are
    /// the wrapper calls `service_turn` and `service` always make.
    fn assert_one_turn(&self, label: &str) {
        self.print(label);
        assert_eq!(
            self.get(Entry::Turn, Boundary::Other, Field::Entries),
            1,
            "{label}: turn entries"
        );
        assert_eq!(
            self.get(Entry::Turn, Boundary::PreTurn, Field::ExportPolls),
            1,
            "{label}: pre-turn poll"
        );
        assert_eq!(
            self.get(Entry::Turn, Boundary::PostTurn, Field::ExportPolls),
            1,
            "{label}: post-turn poll"
        );
        assert_eq!(
            self.entry_total(Entry::Flush, Field::Entries),
            0,
            "{label}: no flush"
        );
        for field in Field::ALL {
            assert_eq!(
                self.entry_total(Entry::Outside, field),
                0,
                "{label}: {} outside",
                field.name()
            );
            assert_eq!(
                self.entry_total(Entry::Flush, field),
                0,
                "{label}: {} in flush rows",
                field.name()
            );
        }
    }
}

#[test]
fn turns_and_flushes_count_in_their_own_rows_and_nothing_lands_outside() {
    let directory =
        std::env::temp_dir().join(format!("shell-liveness-counts-{}", std::process::id()));
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
    let mut epochs = ContentEpochRegistry::new(64 * 1024 * 1024).unwrap();
    transport
        .reserve_content(
            &mut epochs,
            ContentLimits::prototype(ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            }),
        )
        .unwrap();
    let socket = transport.socket_path().to_owned();
    let (go, proceed) = mpsc::channel();
    let (signal, arrived) = mpsc::channel();
    let peer = std::thread::spawn(move || {
        let mut peer = Peer::connect(&socket);
        peer.setup();
        let offer = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: 1,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 5,
                maximum_revision: 6,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&offer, 1);
        let event = peer.next_event();
        let offset = peer.event_offset();
        peer.ack(&event);
        signal.send(()).unwrap();
        proceed.recv_timeout(WAIT).unwrap();
        let body = [1u32.to_le_bytes().as_slice(), &0x7ffu64.to_le_bytes()].concat();
        assert_eq!(
            peer.rpc_with_prefix(24, &body, 7 + body.len(), || signal.send(()).unwrap())
                .unwrap()
                .0,
            25
        );
        signal.send(()).unwrap();
        proceed.recv_timeout(WAIT).unwrap();
        let body = [
            2u32.to_le_bytes().as_slice(),
            &offset.to_le_bytes(),
            &65500u32.to_le_bytes(),
        ]
        .concat();
        let (kind, bytes) = peer
            .rpc_with_prefix(116, &body, 7 + body.len(), || signal.send(()).unwrap())
            .unwrap();
        assert_eq!(kind, 117);
        let object = decode_shell_file_object_published(&bytes[4..]).unwrap();
        assert_eq!(object.object, ShellFileKind::Outputs);
        assert_eq!(object.generation, 1);
        signal.send(()).unwrap();
        proceed.recv_timeout(WAIT).unwrap();
    });
    transport
        .begin_file_negotiation(
            &epochs,
            1,
            WAIT,
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let started = Instant::now();
    while transport
        .poll_negotiation(&mut epochs, 64 * 1024)
        .unwrap()
        .is_none()
    {
        assert!(started.elapsed() < WAIT);
        std::thread::yield_now();
    }
    while arrived.try_recv().is_err() {
        transport.poll_io(&mut epochs).unwrap();
        assert!(started.elapsed() < WAIT);
        std::thread::yield_now();
    }

    let before = transport.wire_turn_count();
    let grid = liveness_counts::snapshot();
    transport.service_owner_turn(&mut epochs).unwrap();
    assert_eq!(transport.wire_turn_count(), before + 1);
    let blind = Delta::since(grid);
    let idle = liveness_counts::snapshot();
    for _ in 0..40 {
        assert_eq!(
            transport
                .service_content_allocation_requests(&mut epochs, &[], 1)
                .unwrap(),
            0
        );
        assert!(
            transport
                .poll_content_action_ack(&mut epochs)
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        transport.wire_turn_count(),
        before + 1,
        "idle roles re-served the wire"
    );
    let idle = Delta::since(idle);

    // A request arrives after the input pass. Role visits do not consume it;
    // readiness persists and the next owner pass replies without a timer.
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    transport.poll_io(&mut epochs).unwrap();
    assert_eq!(transport.wire_turn_count(), before + 1);
    let subscription = transport.readiness_subscription().unwrap();
    let mut fds = transport.poll_fds();
    assert!(rustix::event::poll(&mut fds, Some(&rustix::event::Timespec::default())).unwrap() > 0);
    let observed = subscription.observe(&fds).unwrap();
    drop(fds);
    let grid = liveness_counts::snapshot();
    transport
        .service_owner_observed(&mut epochs, Some(observed))
        .unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    let answered = Delta::since(grid);

    // Register an empty journal read, then publish during the same owner pass.
    go.send(()).unwrap();
    arrived.recv_timeout(WAIT).unwrap();
    transport.service_owner_turn(&mut epochs).unwrap();
    let before = transport.wire_turn_count();
    transport
        .publish_content_output_facts(
            &mut epochs,
            TransactionId::from_raw(9),
            1,
            vec![ContentOutputFactsEntry {
                output: ContentOutputId {
                    id: 2,
                    generation: 1,
                },
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    let grid = liveness_counts::snapshot();
    transport.poll_io(&mut epochs).unwrap();
    assert_eq!(transport.wire_turn_count(), before + 1);
    arrived.recv_timeout(WAIT).unwrap();
    let flush = Delta::since(grid);

    go.send(()).unwrap();
    peer.join().unwrap();
    transport.service_owner_turn(&mut epochs).unwrap();
    assert!(matches!(
        transport.poll_content_action_ack(&mut epochs),
        Err(ShellTransportError::NotConnected)
    ));
    transport.disconnect(&mut epochs).unwrap();
    std::fs::remove_dir_all(directory).unwrap();

    // The peer joined with every reply checked, so the owner processed each
    // request; only now are the counts judged. This wire has legacy admission
    // and no image channel: custody and accept counts stay zero here and are
    // proved at their own sites.
    blind.assert_one_turn("blind turn");
    assert_eq!(blind.total(Field::AcceptAttempts), 0, "blind accept");
    assert_eq!(
        blind.total(Field::CustodyPolls),
        0,
        "legacy admission polls custody"
    );
    assert_eq!(blind.total(Field::FileChecks), 0, "idle probe file checks");
    assert_eq!(idle.total(Field::Entries), 0, "idle role visits entered");
    answered.assert_one_turn("observed turn answering a request");
    assert_eq!(answered.total(Field::AcceptAttempts), 0, "observed accept");
    assert_eq!(
        answered.get(Entry::Turn, Boundary::FileCheck, Field::FileChecks),
        1,
        "request file check"
    );
    flush.print("publication flush");
    assert_eq!(
        flush.get(Entry::Flush, Boundary::Other, Field::Entries),
        1,
        "flush entries"
    );
    assert_eq!(
        flush.get(Entry::Flush, Boundary::PreTurn, Field::ExportPolls),
        1,
        "flush pre poll"
    );
    assert_eq!(
        flush.get(Entry::Flush, Boundary::PostTurn, Field::ExportPolls),
        1,
        "flush post poll"
    );
    assert_eq!(
        flush.get(Entry::Flush, Boundary::FileCheck, Field::FileChecks),
        1,
        "publication file check"
    );
    assert_eq!(
        flush.entry_total(Entry::Turn, Field::Entries),
        0,
        "a flush is not a turn"
    );
    for field in Field::ALL {
        assert_eq!(
            flush.entry_total(Entry::Outside, field),
            0,
            "{} outside",
            field.name()
        );
    }
}

//! Authenticated owned-ring integration over the real 9P and image-channel
//! wires, mounted as a child of the owned-copy owner: D4C1 registration,
//! D4F1 ring admission, an owner-serviced offer, a wire ResourceCancel
//! under its own transaction, failed-retire probes that must not spend the
//! held terminal status, and slot reuse.
//!
//! Determinism comes from completion custody, not scheduling: the test
//! owns the worker's completion queue and delivers no completion until the
//! store has applied both failed retires and the cancel (three resource
//! records). The tiny copy therefore cannot settle before cancellation,
//! however the owner service is scheduled.
use super::*;
use crate::owned_ring_protocol::{CONTROL_FILE, Event, EventKind};
use crate::raw_file_test_peer as raw;
use crate::{
    ContentAllocationSnapshot, ProtectionBackendKind, ProtectionDomainEvidence,
    ProtectionDomainRole, ShellContentAdmissionPolicy,
};
use sophia_protocol::{shell_files::*, *};
use std::collections::VecDeque;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

#[path = "image_test_authority.rs"]
mod authority;

const GRANT: ContentGrant = ContentGrant {
    connection_epoch: 1,
    content_grant_epoch: 1,
};
const RESOURCE_ONE: ContentResourceId = ContentResourceId {
    id: 1,
    generation: 1,
};
const RESOURCE_TWO: ContentResourceId = ContentResourceId {
    id: 2,
    generation: 1,
};
const ALLOCATION: ContentAllocationId = ContentAllocationId {
    id: 1,
    generation: 1,
};
const OUTPUT: ContentOutputId = ContentOutputId {
    id: 1,
    generation: 1,
};

fn identity() -> crate::owned_ring_protocol::Identity {
    crate::owned_ring_protocol::Identity {
        grant: GRANT,
        channel: 1,
        ring: 7,
        generation: 1,
    }
}

fn registration() -> crate::owned_ring_protocol::Registration {
    crate::owned_ring_protocol::Registration {
        identity: identity(),
        slots: 2,
        pixels: 8,
    }
}

fn offer(
    offer: u64,
    generation: u64,
    resource: ContentResourceId,
) -> crate::owned_ring_protocol::Offer {
    crate::owned_ring_protocol::Offer {
        identity: identity(),
        request: crate::OwnedCopyImageRequest {
            slot: crate::OwnedSlotIdentity {
                offer,
                slot: 0,
                generation,
            },
            resource,
            allocation: ALLOCATION,
            output: OUTPUT,
            topology_generation: 1,
            width: 2,
            height: 1,
        },
    }
}

fn config() -> (
    ShellImageDiscovery,
    ShellImageLimits,
    ShellImageFormats,
    ShellImageRoleBounds,
) {
    (
        ShellImageDiscovery {
            connection_epoch: 1,
            channel_generation: 1,
            endpoint: "image.sock".into(),
            source_kinds: 4,
            max_offers: 4,
            max_packets_per_turn: 1,
            hello_timeout_ms: 250,
            validation_timeout_ms: 2000,
            validation_chunk_bytes: 65536,
            capture_interest_timeout_ms: 500,
            formats_generation: 1,
            limits_generation: 1,
        },
        ShellImageLimits {
            channel_generation: 1,
            limits_generation: 1,
            max_resource_bytes: 1 << 20,
            max_sealed_backing_bytes: 4 << 20,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: 4,
            max_validation_jobs: 4,
            max_capture_jobs: 0,
            max_admitted_outputs: 1,
            max_format_rows_per_output: 256,
            max_width: 1920,
            max_height: 1024,
            validation_timeout_ms: 2000,
            capture_interest_timeout_ms: 500,
            validation_chunk_bytes: 65536,
            max_packets_per_turn: 1,
        },
        ShellImageFormats {
            channel_generation: 1,
            formats_generation: 1,
            limits_generation: 1,
            rows: vec![],
        },
        ShellImageRoleBounds {
            max_resource_bytes: 1 << 20,
            max_width: 1920,
            max_height: 1024,
            max_admitted_outputs: 1,
            transfer_slots: 4,
        },
    )
}

fn ack(peer: &mut raw::Peer, event: &[u8]) {
    let record = decode_shell_file_record_at_revision(event, ShellFileClass::Event, 10).unwrap();
    let bytes = encode_shell_file_ack(ShellFileAck {
        connection_epoch: record.header.connection_epoch,
        sequence: record.header.sequence,
    })
    .unwrap();
    assert_eq!(peer.write(4, &bytes).0, 119);
}

/// Journal reader that tolerates any record interleaving: owned events,
/// statuses and submission receipts are classified into separate queues.
struct Wire {
    peer: raw::Peer,
    // One raw arrival ordinal per pumped record, shared across queues, so
    // status-before-release is asserted on journal arrival, not queue order.
    next_ordinal: u64,
    owned: VecDeque<(u64, Event)>,
    statuses: VecDeque<(u64, TransactionId, ContentResourceStatus)>,
    submitted: Vec<u64>,
}

impl Wire {
    fn pump(&mut self) {
        let bytes = self.peer.next_event();
        let ordinal = self.next_ordinal;
        self.next_ordinal += 1;
        if bytes.get(4..8) == Some(b"D4E1") {
            let event = Event::decode(&bytes).expect("owned ring event record");
            let ack = encode_shell_file_ack(ShellFileAck {
                connection_epoch: 1,
                sequence: event.sequence,
            })
            .unwrap();
            assert_eq!(self.peer.write(4, &ack).0, 119);
            self.owned.push_back((ordinal, event));
            return;
        }
        let record =
            decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 10).unwrap();
        match record.header.kind {
            ShellFileKind::Submitted => {
                self.submitted.push(
                    decode_shell_file_submitted_at_revision(&bytes, 10)
                        .unwrap()
                        .submission_id,
                );
            }
            ShellFileKind::ResourceStatus => {
                let decoded = decode_shell_file_resource_status_at_revision(&bytes, 10).unwrap();
                let ShellContentRecord::ResourceStatus(status) = decoded.record else {
                    panic!("expected ResourceStatus record");
                };
                self.statuses
                    .push_back((ordinal, decoded.transaction, status));
            }
            kind => panic!("unexpected journal record {kind:?}"),
        }
        ack(&mut self.peer, &bytes);
    }

    fn submit(&mut self, bytes: &[u8], submission_id: u64) {
        assert_eq!(self.peer.submit(bytes).0, 119);
        while !self.submitted.contains(&submission_id) {
            self.pump();
        }
        self.submitted.retain(|id| *id != submission_id);
        self.peer.clear();
    }

    fn next_owned(&mut self) -> (u64, Event) {
        while self.owned.is_empty() {
            self.pump();
        }
        self.owned.pop_front().unwrap()
    }

    fn next_status(&mut self) -> (u64, TransactionId, ContentResourceStatus) {
        while self.statuses.is_empty() {
            self.pump();
        }
        self.statuses.pop_front().unwrap()
    }
}

fn resource_record(
    kind: ShellFileKind,
    submission_id: u64,
    transaction: u64,
    record: ShellContentRecord,
) -> Vec<u8> {
    let header = ShellFileHeader {
        kind,
        connection_epoch: 1,
        submission_id,
        sequence: 0,
    };
    let body = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(transaction),
        record,
    };
    match kind {
        ShellFileKind::ResourceCancel => encode_shell_file_resource_cancel(header, &body),
        ShellFileKind::ResourceRetire => encode_shell_file_resource_retire(header, &body),
        _ => unreachable!(),
    }
    .unwrap()
}

#[test]
fn owned_ring_cancel_releases_slot_and_reuse_succeeds() {
    let directory =
        std::env::temp_dir().join(format!("shell-owned-ring-wire-{}", std::process::id()));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    transport
        .authorize_authenticated_peer(
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Box::new(authority::Factory),
        )
        .unwrap();
    let (d, l, f, b) = config();
    transport.configure_image_channel(d, l, f, b).unwrap();
    let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
    transport
        .reserve_content(&mut epochs, ContentLimits::prototype(GRANT))
        .unwrap();
    transport
        .begin_file_negotiation(
            &epochs,
            1,
            Duration::from_secs(3),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let path = transport.socket_path().to_owned();
    let image_path = directory.join("image.sock");
    let (need_allocation, allocation_needed) = std::sync::mpsc::channel::<()>();
    let (give_allocation, allocation_supplied) = std::sync::mpsc::channel::<()>();
    let (offer_one, offer_one_written) = std::sync::mpsc::channel::<()>();
    let (admit_one, admitted_one) = std::sync::mpsc::channel::<()>();
    let (cancel_wire, _cancel_on_wire) = std::sync::mpsc::channel::<()>();
    let (cancel_done, cancel_applied) = std::sync::mpsc::channel::<()>();
    let peer_thread = std::thread::spawn(move || {
        let mut peer = raw::Peer::connect(&path);
        // The attach sequence this server accepts (as in shell_image_transport).
        assert_eq!(
            peer.rpc(
                100,
                &[
                    65536u32.to_le_bytes().as_slice(),
                    &8u16.to_le_bytes(),
                    b"9P2000.L"
                ]
                .concat()
            )
            .unwrap()
            .0,
            101
        );
        assert_eq!(
            peer.rpc(
                102,
                &[
                    10u32.to_le_bytes().as_slice(),
                    &[0; 4],
                    &u32::MAX.to_le_bytes()
                ]
                .concat()
            )
            .unwrap()
            .0,
            103
        );
        assert_eq!(
            peer.rpc(
                104,
                &[
                    1u32.to_le_bytes().as_slice(),
                    &10u32.to_le_bytes(),
                    &[0; 4],
                    &u32::MAX.to_le_bytes()
                ]
                .concat()
            )
            .unwrap()
            .0,
            105
        );
        peer.open(2, b"events", 0);
        peer.open(3, b"submit", 1);
        peer.open(4, b"ack", 1);
        let negotiate = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: 1,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 10,
                maximum_revision: 10,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | crate::owned_ring_protocol::CAPABILITY,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&negotiate, 1);
        let negotiated = peer.next_event();
        assert_eq!(
            decode_shell_file_negotiated(&negotiated)
                .unwrap()
                .welcome
                .selected_revision,
            10
        );
        ack(&mut peer, &negotiated);
        peer.open(6, b"image-channel", 0);
        let discovery =
            ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(6, 0)).unwrap()).unwrap();
        assert_eq!(discovery.channel_generation, 1);
        // Image-channel readiness precedes owned disclosure on this wire.
        let socket = rustix::net::socket_with(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::SEQPACKET,
            rustix::net::SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        rustix::net::connect(
            &socket,
            &rustix::net::SocketAddrUnix::new(&image_path).unwrap(),
        )
        .unwrap();
        let hello = ShellImagePacket::Hello {
            connection_epoch: 1,
            channel_generation: 1,
        }
        .encode()
        .unwrap();
        rustix::net::send(&socket, &hello, rustix::net::SendFlags::NOSIGNAL).unwrap();
        let ready = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&ready, ShellFileClass::Event, 10).unwrap();
        assert_eq!(record.header.kind, ShellFileKind::ChannelReady);
        ack(&mut peer, &ready);
        let mut wire = Wire {
            peer,
            next_ordinal: 0,
            owned: VecDeque::new(),
            statuses: VecDeque::new(),
            submitted: Vec::new(),
        };
        // D4C1 Register on the experimental control file.
        wire.peer.open(7, CONTROL_FILE, 1);
        let register = registration().encode();
        assert_eq!(wire.peer.write(7, &register).0, 119);
        let (_, admitted) = wire.next_owned();
        assert_eq!(admitted.kind, EventKind::RingAdmitted);
        assert_eq!(admitted.identity, identity());
        // D4F1: the sealed ring descriptor on the image channel.
        use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
        use std::os::unix::fs::FileExt;
        let ring = std::fs::File::from(
            memfd_create(
                c"owned-ring-wire",
                MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
            )
            .unwrap(),
        );
        ring.set_len(registration().geometry().unwrap().source_bytes() as u64)
            .unwrap();
        // Slot 0 carries generation 2 and eight payload bytes past its header.
        ring.write_at(&2u64.to_le_bytes(), 0).unwrap();
        ring.write_at(&[7; 8], 4096).unwrap();
        fcntl_add_seals(&ring, SealFlags::GROW | SealFlags::SHRINK).unwrap();
        let packet = registration().source_packet();
        let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = rustix::net::SendAncillaryBuffer::new(&mut space);
        let rights = [ring.as_fd()];
        assert!(control.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)));
        rustix::net::sendmsg(
            &socket,
            &[std::io::IoSlice::new(&packet)],
            &mut control,
            rustix::net::SendFlags::NOSIGNAL,
        )
        .unwrap();
        let (_, ready) = wire.next_owned();
        assert_eq!(ready.kind, EventKind::RingReady);
        // The main thread grants the exact allocation before any offer.
        need_allocation.send(()).unwrap();
        allocation_supplied
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let first = offer(1, 2, RESOURCE_ONE).encode();
        assert_eq!(wire.peer.write(7, &first).0, 119);
        offer_one.send(()).unwrap();
        // No worker completion is delivered until the cancel is applied.
        admitted_one.recv_timeout(Duration::from_secs(5)).unwrap();
        // Failed retire replies must not spend the held terminal status:
        // neither the offer's own transaction nor an unrelated one.
        for (submission, transaction) in [(10, 1), (11, 55)] {
            let retire = resource_record(
                ShellFileKind::ResourceRetire,
                submission,
                transaction,
                ShellContentRecord::ResourceRetire(ContentResourceRetire {
                    grant: GRANT,
                    resource: RESOURCE_ONE,
                }),
            );
            wire.submit(&retire, submission);
            let (_, _, status) = wire.next_status();
            assert_eq!(status.resource, RESOURCE_ONE);
            assert_eq!(status.status, 3, "failed retire reply, tx {transaction}");
        }
        // The wire cancel carries its own transaction, never the offer id.
        // The barrier holds completion delivery until the store has
        // applied it; only then may the retained result settle Cancelled.
        let cancel = resource_record(
            ShellFileKind::ResourceCancel,
            12,
            999,
            ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: GRANT,
                resource: RESOURCE_ONE,
            }),
        );
        cancel_wire.send(()).unwrap();
        wire.submit(&cancel, 12);
        cancel_applied.recv_timeout(Duration::from_secs(5)).unwrap();
        let (status_ordinal, tx, status) = wire.next_status();
        assert_eq!(tx.raw(), 999);
        assert_eq!(status.status, 4);
        assert_eq!(status.reason, ContentReason::Cancelled as u16);
        assert_eq!(status.resource, RESOURCE_ONE);
        let (released_ordinal, released) = wire.next_owned();
        assert_eq!(released.kind, EventKind::SlotReleased);
        assert_eq!(released.slot, offer(1, 2, RESOURCE_ONE).request.slot);
        assert_eq!(released.resource, RESOURCE_ONE);
        assert_eq!(released.reason, ContentReason::Cancelled as u16);
        assert!(
            status_ordinal < released_ordinal,
            "terminal status must arrive before its SlotReleased"
        );
        // Slot reuse: same slot, later generation, new resource, success.
        ring.write_at(&4u64.to_le_bytes(), 0).unwrap();
        ring.write_at(&[9; 8], 4096).unwrap();
        let second = offer(2, 4, RESOURCE_TWO).encode();
        assert_eq!(wire.peer.write(7, &second).0, 119);
        let (status_ordinal, tx, status) = wire.next_status();
        assert_eq!(tx.raw(), 2);
        assert_eq!(status.status, 2);
        assert_eq!(status.resource, RESOURCE_TWO);
        let (released_ordinal, released) = wire.next_owned();
        assert_eq!(released.kind, EventKind::SlotReleased);
        assert_eq!(released.slot, offer(2, 4, RESOURCE_TWO).request.slot);
        assert_eq!(released.reason, 0);
        assert!(
            status_ordinal < released_ordinal,
            "success status must arrive before its SlotReleased"
        );
    });
    let mut owner = OwnedCopyOwner::default();
    let mut worker = IsbValidationWorker::new(4, 4 << 20).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut negotiated = false;
    let mut allocation_done = false;
    let mut offer_one_seen = false;
    let mut admitted_sent = false;
    // Completions stay in the worker queue until the barrier: exactly the
    // two failed retires and the cancel have been applied by the store.
    let mut deliver = false;
    let mut total_processed = 0usize;
    while !peer_thread.is_finished() {
        assert!(Instant::now() < deadline, "owned ring wire stalled");
        if !negotiated {
            negotiated = transport
                .poll_negotiation(&mut epochs, 65536)
                .unwrap()
                .is_some();
        } else {
            match transport.service_content_resources(&mut epochs, 0) {
                Ok(processed) => total_processed += processed,
                Err(ShellTransportError::NotConnected) => {}
                Err(error) => panic!("resource service: {error}"),
            }
            if !deliver && total_processed >= 3 {
                deliver = true;
                cancel_done.send(()).unwrap();
            }
            if deliver {
                while let Some(completion) = worker.take_completed().unwrap() {
                    assert!(owner.receive(completion).is_ok());
                }
            }
            owner
                .service(
                    &mut transport,
                    &mut epochs,
                    &mut worker,
                    0,
                    Instant::now(),
                    0,
                )
                .unwrap();
            if !allocation_done && allocation_needed.try_recv().is_ok() {
                supply_allocation(&mut epochs);
                allocation_done = true;
                give_allocation.send(()).unwrap();
            }
            if offer_one_written.try_recv().is_ok() {
                offer_one_seen = true;
            }
            if offer_one_seen && !admitted_sent {
                // Admission is stable while its completion is withheld:
                // the transfer cannot leave staging before delivery.
                let staging = epochs
                    .resources(GRANT)
                    .map(|store| store.usage().staging)
                    .unwrap_or(0);
                if staging > 0 {
                    admitted_sent = true;
                    admit_one.send(()).unwrap();
                }
            }
        }
        std::thread::yield_now();
    }
    peer_thread.join().unwrap();
    assert_eq!(owner.pending(), 0);
    transport.disconnect(&mut epochs).unwrap();
    drop(transport);
    assert!(!directory.exists());
}

fn supply_allocation(epochs: &mut ContentEpochRegistry) {
    let store = epochs.allocations_mut(GRANT).unwrap();
    store
        .publish_outputs(
            TransactionId::from_raw(1),
            1,
            vec![ContentOutputFactsEntry {
                output: OUTPUT,
                local_width: 200,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
    store
        .request(
            TransactionId::from_raw(2),
            ContentAllocationRequest {
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
                desired_width: 200,
                desired_height: 20,
                margins: ContentMargins::default(),
            },
            &[],
            0,
        )
        .unwrap();
    store
        .grant(
            1,
            ContentAllocationSnapshot {
                native_opening: None,
                output: OUTPUT,
                allocation: ALLOCATION,
                scale_generation: 1,
                scale_numerator: 1,
                scale_denominator: 1,
                role: 1,
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
                allowed_reservation_extent: 20,
            },
            &[],
        )
        .unwrap();
    while store.take_event().is_some() {}
}

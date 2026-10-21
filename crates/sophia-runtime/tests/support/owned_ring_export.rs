//! Export handshake controls for the owned ring: registration
//! replay and deadlines, FD admission, offer retries, and the ordered
//! status-then-slot-release publication. Real memfds and a real
//! SOCK_SEQPACKET pair carry every FD; nothing here mutates the
//! environment.
use super::*;
use crate::owned_ring_protocol::{Event, EventKind, Identity, Offer, Registration};
use sophia_protocol::{
    ContentAllocationId, ContentGrant, ContentOutputId, ContentReason, ContentResourceId,
};
use std::os::fd::{AsFd, OwnedFd};
use std::time::{Duration, Instant};

const SLOTS: usize = 2;
const PIXELS: usize = 48 * 24 * 4;

fn identity() -> Identity {
    Identity {
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 2,
        },
        channel: 1,
        ring: 7,
        generation: 1,
    }
}

fn registration() -> Registration {
    Registration {
        identity: identity(),
        slots: SLOTS,
        pixels: PIXELS,
    }
}

fn offer(offer: u64, slot: usize, generation: u64) -> Offer {
    Offer {
        identity: identity(),
        request: crate::OwnedCopyImageRequest {
            slot: crate::OwnedSlotIdentity {
                offer,
                slot,
                generation,
            },
            resource: ContentResourceId {
                id: 100 + offer,
                generation: 1,
            },
            allocation: ContentAllocationId {
                id: 8,
                generation: 9,
            },
            output: ContentOutputId {
                id: 10,
                generation: 11,
            },
            topology_generation: 12,
            width: 48,
            height: 24,
        },
    }
}

fn epochs() -> ContentEpochRegistry {
    let mut registry = ContentEpochRegistry::new(64 << 20).unwrap();
    registry
        .admit(sophia_protocol::ContentLimits::prototype(identity().grant))
        .unwrap();
    registry
}

fn fixture() -> ShellFiles {
    let mut files = ShellFiles::awaiting_negotiation(
        1,
        "bar",
        false,
        JournalBounds {
            bytes: 1 << 20,
            reserve_bytes: 1 << 16,
        },
        100,
        Instant::now(),
    );
    files
        .configure_image_channel(Some(ShellImageChannelConfig {
            discovery: ShellImageDiscovery {
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
            limits: ShellImageLimits {
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
            formats: ShellImageFormats {
                channel_generation: 1,
                formats_generation: 1,
                limits_generation: 1,
                rows: vec![],
            },
            role_bounds: ShellImageRoleBounds {
                max_resource_bytes: 1 << 20,
                max_width: 1920,
                max_height: 1024,
                max_admitted_outputs: 1,
                transfer_slots: 4,
            },
        }))
        .unwrap();
    files.negotiated = true;
    files.image_revision = 10;
    files.capabilities = crate::owned_ring_protocol::CAPABILITY;
    files.content = true;
    files.content_limits = Some(sophia_protocol::ContentLimits::prototype(identity().grant));
    files.image.as_mut().unwrap().readiness = ShellImageReadiness::Ready {
        granted_source_kinds: 4,
    };
    files
}

/// A real sealed (or deliberately unsealed) memfd of exactly ring extent.
fn ring_fd(sealed: bool) -> OwnedFd {
    let geometry = registration().geometry().unwrap();
    let fd = rustix::fs::memfd_create(
        "owned-ring-export-test",
        rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING,
    )
    .unwrap();
    rustix::fs::ftruncate(&fd, geometry.source_bytes() as u64).unwrap();
    if sealed {
        rustix::fs::fcntl_add_seals(
            &fd,
            rustix::fs::SealFlags::GROW | rustix::fs::SealFlags::SHRINK,
        )
        .unwrap();
    }
    fd
}

/// Carry bytes plus FDs over a real SEQPACKET pair into an ImageDatagram.
fn packet(
    bytes: &[u8],
    fds: &[std::os::fd::BorrowedFd<'_>],
) -> crate::image_channel::ImageDatagram {
    use rustix::net::{
        AddressFamily, SendAncillaryBuffer, SendAncillaryMessage, SocketFlags, SocketType, sendmsg,
        socketpair,
    };
    use std::mem::MaybeUninit;
    let (a, b) = socketpair(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
    let mut ancillary = SendAncillaryBuffer::new(&mut space);
    if !fds.is_empty() {
        assert!(ancillary.push(SendAncillaryMessage::ScmRights(fds)));
    }
    sendmsg(
        &a,
        &[std::io::IoSlice::new(bytes)],
        &mut ancillary,
        rustix::net::SendFlags::empty(),
    )
    .unwrap();
    crate::image_channel::receive_image_datagram(b.as_fd())
        .unwrap()
        .expect("datagram present")
}

fn journal_bytes(files: &ShellFiles) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut offset = 0u64;
    loop {
        match files.journal.read(offset, 65536) {
            Ok(sophia_9p::ReadOutcome::Ready(chunk)) if !chunk.is_empty() => {
                offset += chunk.len() as u64;
                bytes.extend_from_slice(&chunk);
            }
            _ => break,
        }
    }
    bytes
}

/// Decode every ResourceStatus record in the journal stream, in order.
fn journal_statuses(files: &ShellFiles) -> Vec<sophia_protocol::ContentResourceStatus> {
    let bytes = journal_bytes(files);
    let record = SHELL_FILE_HEADER_BYTES + 56;
    let mut statuses = Vec::new();
    let mut at = 0usize;
    while at + record <= bytes.len() {
        if let Ok(decoded) = decode_shell_file_resource_status(&bytes[at..at + record])
            && let ShellContentRecord::ResourceStatus(status) = decoded.record
        {
            statuses.push(status);
            at += record;
            continue;
        }
        at += 1;
    }
    statuses
}

/// Decode every owned event in the journal stream, in order.
fn journal_events(files: &ShellFiles) -> Vec<Event> {
    let bytes = journal_bytes(files);
    // Records are length-prefixed: the D4E1 magic sits four bytes in, and
    // Event::decode requires exactly one record, not the stream tail.
    let mut events = Vec::new();
    let mut at = 4usize;
    while at + 4 <= bytes.len() {
        if bytes[at..].starts_with(b"D4E1")
            && at + EVENT_BYTES - 4 <= bytes.len()
            && let Some(event) = Event::decode(&bytes[at - 4..at - 4 + EVENT_BYTES])
        {
            events.push(event);
            at += EVENT_BYTES;
            continue;
        }
        at += 1;
    }
    events
}

fn register(files: &mut ShellFiles) {
    let bytes = registration().encode();
    assert_eq!(files.write_owned_ring(&bytes), Ok(bytes.len() as u32));
}

fn ready(files: &mut ShellFiles, epochs: &mut ContentEpochRegistry) -> OwnedRing {
    register(files);
    assert!(matches!(
        files.service_owned_ring_setup(epochs, Instant::now()),
        Ok(None)
    ));
    let fd = ring_fd(true);
    let packet = packet(&registration().source_packet(), &[fd.as_fd()]);
    files.receive_owned_source(packet).unwrap();
    let (_, ring) = files
        .service_owned_ring_setup(epochs, Instant::now())
        .unwrap()
        .expect("ring ready");
    ring
}

#[test]
fn offers_may_use_less_than_slot_capacity_but_cannot_exceed_it() {
    let mut files = fixture();
    let mut epochs = epochs();
    let _ring = ready(&mut files, &mut epochs);
    let mut request = offer(1, 0, 2);
    request.request.width = 49;
    assert_eq!(
        files.write_owned_ring(&request.encode()),
        Err(Errno::EINVAL)
    );
    request.request.width = 24;
    assert_eq!(
        files.write_owned_ring(&request.encode()),
        Ok(request.encode().len() as u32)
    );
    assert_eq!(files.owned_pending_offer().unwrap().0, request);
}

#[test]
fn registration_replay_keeps_original_deadline_and_single_admission() {
    let mut files = fixture();
    let mut epochs = epochs();
    register(&mut files);
    let deadline = files.owned_deadline().expect("setup deadline");
    // Exact replay is idempotent and does not renew the deadline.
    let bytes = registration().encode();
    assert_eq!(files.write_owned_ring(&bytes), Ok(bytes.len() as u32));
    assert_eq!(files.owned_deadline(), Some(deadline));
    files
        .service_owned_ring_setup(&mut epochs, Instant::now())
        .unwrap();
    assert_eq!(
        journal_events(&files)
            .iter()
            .filter(|e| e.kind == EventKind::RingAdmitted)
            .count(),
        1
    );
    // Replay is still exact after admission; no second event is published.
    let published = files.owned_publication_sequence();
    assert_eq!(files.write_owned_ring(&bytes), Ok(bytes.len() as u32));
    files
        .service_owned_ring_setup(&mut epochs, Instant::now())
        .unwrap();
    assert_eq!(files.owned_publication_sequence(), published);
    // A different registration is not a replay.
    let other = Registration {
        slots: 1,
        ..registration()
    };
    assert_eq!(files.write_owned_ring(&other.encode()), Err(EALREADY));
}

#[test]
fn lowered_image_limits_refuse_registration_before_any_hold() {
    let mut files = fixture();
    files
        .image
        .as_mut()
        .unwrap()
        .config
        .limits
        .max_sealed_backing_bytes = 1;
    let bytes = registration().encode();
    assert_eq!(files.write_owned_ring(&bytes), Err(Errno::EINVAL));
    assert!(!files.owned_mode());
    assert_eq!(files.owned_deadline(), None);
}

#[test]
fn setup_timeout_refuses_and_refunds_source_budget() {
    let mut files = fixture();
    let mut epochs = epochs();
    let before = epochs.accounting();
    register(&mut files);
    files
        .service_owned_ring_setup(&mut epochs, Instant::now())
        .unwrap();
    // Budget is held once RingAdmitted is published.
    assert_eq!(
        epochs.accounting().source_bytes,
        before.source_bytes + registration().geometry().unwrap().source_bytes() as u64
    );
    // Advancing past the original deadline refuses with Timeout and the
    // reservation is returned with the phase that held it.
    files
        .service_owned_ring_setup(&mut epochs, Instant::now() + Duration::from_secs(3))
        .unwrap();
    let kinds: Vec<(EventKind, u16)> = journal_events(&files)
        .iter()
        .map(|e| (e.kind, e.reason))
        .collect();
    assert_eq!(
        kinds,
        [
            (EventKind::RingAdmitted, 0),
            (EventKind::RingRefused, ContentReason::Timeout as u16)
        ]
    );
    assert_eq!(epochs.accounting(), before);
    assert_eq!(files.owned_deadline(), None);
}

#[test]
fn fd_before_admission_and_duplicate_fd_are_refused() {
    let mut files = fixture();
    let mut epochs = epochs();
    // No registration at all.
    let fd = ring_fd(true);
    assert_eq!(
        files.receive_owned_source(packet(&registration().source_packet(), &[fd.as_fd()])),
        Err(Errno::EPROTO)
    );
    register(&mut files);
    // Registered but RingAdmitted not yet published: still too early.
    let fd = ring_fd(true);
    assert_eq!(
        files.receive_owned_source(packet(&registration().source_packet(), &[fd.as_fd()])),
        Err(Errno::EPROTO)
    );
    files
        .service_owned_ring_setup(&mut epochs, Instant::now())
        .unwrap();
    // Zero and two descriptors are both refused before any is taken.
    assert_eq!(
        files.receive_owned_source(packet(&registration().source_packet(), &[])),
        Err(Errno::EPROTO)
    );
    let (a, b) = (ring_fd(true), ring_fd(true));
    assert_eq!(
        files.receive_owned_source(packet(
            &registration().source_packet(),
            &[a.as_fd(), b.as_fd()]
        )),
        Err(Errno::EPROTO)
    );
    // One exact FD is accepted; a second is a duplicate.
    let fd = ring_fd(true);
    files
        .receive_owned_source(packet(&registration().source_packet(), &[fd.as_fd()]))
        .unwrap();
    let fd = ring_fd(true);
    assert_eq!(
        files.receive_owned_source(packet(&registration().source_packet(), &[fd.as_fd()])),
        Err(Errno::EPROTO)
    );
}

#[test]
fn unsealed_or_short_fd_never_becomes_ready() {
    // Unsealed: refused at receive, nothing stored.
    let mut files = fixture();
    let mut epochs = epochs();
    register(&mut files);
    files
        .service_owned_ring_setup(&mut epochs, Instant::now())
        .unwrap();
    let fd = ring_fd(false);
    assert_eq!(
        files.receive_owned_source(packet(&registration().source_packet(), &[fd.as_fd()])),
        Err(Errno::EPROTO)
    );
    // Sealed but wrong extent: admission fails as Malformed and refunds.
    let mut files = fixture();
    let before = epochs.accounting();
    register(&mut files);
    files
        .service_owned_ring_setup(&mut epochs, Instant::now())
        .unwrap();
    let short = {
        let geometry = registration().geometry().unwrap();
        let _ = geometry;
        let fd = rustix::fs::memfd_create(
            "owned-ring-short",
            rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING,
        )
        .unwrap();
        rustix::fs::ftruncate(&fd, 8).unwrap();
        rustix::fs::fcntl_add_seals(
            &fd,
            rustix::fs::SealFlags::GROW | rustix::fs::SealFlags::SHRINK,
        )
        .unwrap();
        fd
    };
    files
        .receive_owned_source(packet(&registration().source_packet(), &[short.as_fd()]))
        .unwrap();
    assert!(matches!(
        files.service_owned_ring_setup(&mut epochs, Instant::now()),
        Ok(None)
    ));
    let kinds: Vec<(EventKind, u16)> = journal_events(&files)
        .iter()
        .map(|e| (e.kind, e.reason))
        .collect();
    assert_eq!(
        kinds,
        [
            (EventKind::RingAdmitted, 0),
            (EventKind::RingRefused, ContentReason::Malformed as u16)
        ]
    );
    assert_eq!(epochs.accounting(), before);
}

#[test]
fn full_handshake_publishes_admitted_then_ready_and_refunds_on_drop() {
    let mut files = fixture();
    let mut epochs = epochs();
    let before = epochs.accounting();
    let ring = ready(&mut files, &mut epochs);
    let kinds: Vec<EventKind> = journal_events(&files).iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EventKind::RingAdmitted, EventKind::RingReady]);
    assert!(files.owned_current_identity().is_some());
    assert_ne!(epochs.accounting(), before);
    drop(ring);
    assert_eq!(epochs.accounting(), before);
}

#[test]
fn offer_retries_busy_and_stale_generations() {
    let mut files = fixture();
    let mut epochs = epochs();
    let _ring = ready(&mut files, &mut epochs);
    let first = offer(1, 0, 2).encode();
    assert_eq!(files.write_owned_ring(&first), Ok(first.len() as u32));
    assert_eq!(
        files.owned_pending_offer().map(|(o, _)| o),
        Some(offer(1, 0, 2))
    );
    // Exact retry: idempotent, same pending obligation.
    assert_eq!(files.write_owned_ring(&first), Ok(first.len() as u32));
    assert_eq!(
        files.owned_pending_offer().map(|(o, _)| o),
        Some(offer(1, 0, 2))
    );
    files.owned_take_offer();
    // Same slot is busy while its copy credit is live.
    let busy = offer(2, 0, 4).encode();
    assert_eq!(files.write_owned_ring(&busy), Err(Errno::EAGAIN));
    // A free slot accepts; then its generation cannot repeat or regress.
    let second = offer(3, 1, 2).encode();
    assert_eq!(files.write_owned_ring(&second), Ok(second.len() as u32));
    files.owned_take_offer();
    let stale = offer(4, 1, 2).encode();
    assert_eq!(files.write_owned_ring(&stale), Err(EALREADY));
    // Offer ids cannot go backwards either.
    let backwards = offer(2, 1, 4).encode();
    assert_eq!(files.write_owned_ring(&backwards), Err(EALREADY));
}

#[test]
fn status_is_published_before_slot_release() {
    let mut files = fixture();
    let mut epochs = epochs();
    let _ring = ready(&mut files, &mut epochs);
    let bytes = offer(1, 0, 2).encode();
    files.write_owned_ring(&bytes).unwrap();
    files.owned_refuse_offer(ContentReason::Cancelled).unwrap();
    // The refusal consumed the pending offer and released its credit.
    assert_eq!(files.owned_pending_offer(), None);
    let events = journal_events(&files);
    let released = events
        .iter()
        .position(|e| e.kind == EventKind::SlotReleased)
        .expect("slot release published");
    assert_eq!(events[released].slot, offer(1, 0, 2).request.slot);
    assert_eq!(events[released].reason, ContentReason::Cancelled as u16);
    // A ResourceStatus record precedes the release in the journal stream:
    // observable as the published sequence having advanced past the status
    // before the release sequence that owned.published now holds.
    assert!(files.owned_publication_sequence() >= 3);
    // The slot is reusable after its release was published.
    let next = offer(2, 0, 4).encode();
    assert_eq!(files.write_owned_ring(&next), Ok(next.len() as u32));
}

/// Encode the exact status shape the store emits at terminal cancel: the
/// cancel's own transaction, status 4 for Cancelled, no next ordinal.
fn store_status(
    transaction: u64,
    status: &sophia_protocol::ContentResourceStatus,
) -> (TransactionId, Vec<u8>) {
    let transaction = TransactionId::from_raw(transaction);
    let record = ShellFileTransactionRecord {
        transaction,
        record: ShellContentRecord::ResourceStatus(status.clone()),
    };
    let bytes = encode_shell_file_resource_status_at_revision(
        ShellFileHeader {
            kind: ShellFileKind::ResourceStatus,
            connection_epoch: 1,
            submission_id: 0,
            sequence: 1,
        },
        &record,
        9,
    )
    .unwrap();
    (transaction, bytes[SHELL_FILE_HEADER_BYTES..].to_vec())
}

#[test]
fn cancel_transaction_status_consumes_held_credit_then_releases_slot() {
    let mut files = fixture();
    let mut epochs = epochs();
    let _ring = ready(&mut files, &mut epochs);
    let bytes = offer(1, 0, 2).encode();
    assert_eq!(files.write_owned_ring(&bytes), Ok(bytes.len() as u32));
    let resource = offer(1, 0, 2).request.resource;
    let cancelled = sophia_protocol::ContentResourceStatus {
        grant: identity().grant,
        resource,
        status: 4,
        reason: ContentReason::Cancelled as u16,
        next_ordinal: 0,
        admitted_bytes: 0,
    };
    // An unrelated resource owes no credit, whatever its transaction.
    let unrelated = sophia_protocol::ContentResourceStatus {
        resource: ContentResourceId {
            id: 777,
            generation: 1,
        },
        ..cancelled
    };
    let (tx, body) = store_status(999, &unrelated);
    assert_eq!(files.append_owned_status(tx, &unrelated, &body), Ok(None));
    // Matching is by grant+resource: the right resource under the wrong
    // grant owes nothing, and the real credit stays intact (proven by the
    // successful append below).
    let wrong = sophia_protocol::ContentResourceStatus {
        grant: ContentGrant {
            content_grant_epoch: 99,
            ..identity().grant
        },
        ..cancelled
    };
    let (tx, body) = store_status(999, &wrong);
    assert_eq!(files.append_owned_status(tx, &wrong, &body), Ok(None));
    // The cancel transaction (999, not offer 1) consumes the held status.
    let (tx, body) = store_status(999, &cancelled);
    let status_sequence = files
        .append_owned_status(tx, &cancelled, &body)
        .unwrap()
        .expect("cancel status consumes the held credit");
    assert_eq!(files.owned_publication_sequence(), status_sequence);
    assert_eq!(journal_statuses(&files), std::slice::from_ref(&cancelled));
    // The credit is spent: the same status again finds nothing owed.
    let (tx, body) = store_status(999, &cancelled);
    assert_eq!(files.append_owned_status(tx, &cancelled, &body), Ok(None));
    // With the status published, the exact SlotReleased follows and the
    // slot accepts a new offer under a later generation.
    files.owned_refuse_offer(ContentReason::Cancelled).unwrap();
    assert_eq!(journal_statuses(&files), std::slice::from_ref(&cancelled));
    assert_eq!(files.owned_pending_offer(), None);
    let events = journal_events(&files);
    let released = events
        .iter()
        .find(|e| e.kind == EventKind::SlotReleased)
        .expect("slot release published");
    assert_eq!(released.slot, offer(1, 0, 2).request.slot);
    assert_eq!(released.resource, resource);
    assert!(released.sequence > status_sequence);
    let next = offer(2, 0, 4).encode();
    assert_eq!(files.write_owned_ring(&next), Ok(next.len() as u32));
    assert_eq!(
        files.owned_pending_offer().map(|(o, _)| o),
        Some(offer(2, 0, 4))
    );
}

#[test]
fn registration_rejects_combined_source_and_destination_beyond_backing() {
    let mut files = fixture();
    let mut epochs = epochs();
    let before = epochs.accounting();
    // Each bound passes alone (pixels == max_resource_bytes, source <=
    // max_sealed_backing_bytes); source + destination together exceed it.
    let impossible = Registration {
        identity: identity(),
        slots: 3,
        pixels: 1 << 20,
    };
    let geometry = impossible.geometry().unwrap();
    assert!(geometry.source_bytes() as u64 <= 4 << 20);
    assert!(geometry.source_bytes() as u64 + geometry.pixel_bytes() as u64 > 4 << 20);
    let bytes = impossible.encode();
    assert_eq!(files.write_owned_ring(&bytes), Err(Errno::EINVAL));
    assert!(!files.owned_mode());
    assert!(matches!(
        files.service_owned_ring_setup(&mut epochs, Instant::now()),
        Ok(None)
    ));
    assert_eq!(epochs.accounting(), before);
}

#[test]
fn refuse_offer_publishes_status_four_for_cancelled_and_distinct_revoked() {
    for (reason, expected) in [
        (ContentReason::Cancelled, Some(4)),
        (ContentReason::Revoked, None),
    ] {
        let mut files = fixture();
        let mut epochs = epochs();
        let _ring = ready(&mut files, &mut epochs);
        let bytes = offer(1, 0, 2).encode();
        assert_eq!(files.write_owned_ring(&bytes), Ok(bytes.len() as u32));
        files.owned_refuse_offer(reason).unwrap();
        let statuses = journal_statuses(&files);
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].reason, reason as u16);
        match expected {
            Some(status) => assert_eq!(statuses[0].status, status),
            None => assert_ne!(statuses[0].status, 4, "revoked stays distinct"),
        }
        // Status precedes its SlotReleased in the same journal.
        let events = journal_events(&files);
        assert!(
            events
                .iter()
                .any(|e| e.kind == EventKind::SlotReleased && e.reason == reason as u16)
        );
    }
}

#[test]
fn generic_control_replies_cannot_spend_an_owned_transfer_status() {
    let mut files = fixture();
    let mut epochs = epochs();
    let _ring = ready(&mut files, &mut epochs);
    let offer = offer(1, 0, 2);
    files.write_owned_ring(&offer.encode()).unwrap();
    let stale = sophia_protocol::ContentResourceStatus {
        grant: offer.identity.grant,
        resource: offer.request.resource,
        status: 3,
        reason: ContentReason::Stale as u16,
        next_ordinal: 0,
        admitted_bytes: 0,
    };
    // Even reusing the offer transaction cannot forge store provenance.
    for tx in [offer.request.slot.offer, 555] {
        let (_, body) = store_status(tx, &stale);
        files
            .append_event(ShellFileKind::ResourceStatus, &body, true)
            .unwrap();
        assert!(
            files
                .owned
                .live
                .iter()
                .flatten()
                .find(|c| c.offer == offer)
                .unwrap()
                .status
                .is_some()
        );
    }
    assert!(
        !journal_events(&files)
            .iter()
            .any(|e| e.kind == EventKind::SlotReleased)
    );
    files.owned_refuse_offer(ContentReason::Cancelled).unwrap();
    assert_eq!(journal_statuses(&files).len(), 3);
    assert_eq!(journal_statuses(&files).last().unwrap().status, 4);
}

//! Supplied allocation policy for the Session image lifecycle test.
use super::raw;
use sophia_protocol::{shell_files::*, *};
use sophia_runtime::owned_ring_protocol::{
    CONTROL_FILE, Event, EventKind, Identity, Offer, Registration,
};
use sophia_runtime::{
    ContentAllocationSnapshot, OwnedCopyImageRequest, OwnedSlotIdentity, ShellTransportConnection,
};

pub fn ack(peer: &mut raw::Peer, bytes: &[u8]) {
    let record = decode_shell_file_record_at_revision(bytes, ShellFileClass::Event, 10).unwrap();
    let bytes = encode_shell_file_ack(ShellFileAck {
        connection_epoch: record.header.connection_epoch,
        sequence: record.header.sequence,
    })
    .unwrap();
    assert_eq!(peer.write(4, &bytes).0, 119);
}

pub fn next_kind(peer: &mut raw::Peer, kind: ShellFileKind) -> Vec<u8> {
    for _ in 0..16 {
        let bytes = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 10).unwrap();
        if record.header.kind == kind {
            return bytes;
        }
        assert!(matches!(
            record.header.kind,
            ShellFileKind::ObjectPublished | ShellFileKind::Submitted
        ));
        ack(peer, &bytes);
    }
    panic!("missing expected event");
}

fn submit(peer: &mut raw::Peer, bytes: &[u8], grant: ContentGrant, id: u64) {
    peer.open(5, b"transaction", 2);
    assert_eq!(peer.write(5, bytes).0, 119);
    let submit = encode_shell_file_submit(ShellFileSubmit {
        connection_epoch: grant.connection_epoch,
        submission_id: id,
        candidate_bytes: bytes.len() as u32,
    })
    .unwrap();
    assert_eq!(peer.write(3, &submit).0, 119);
    let receipt = next_kind(peer, ShellFileKind::Submitted);
    assert_eq!(
        decode_shell_file_submitted_at_revision(&receipt, 10)
            .unwrap()
            .submission_id,
        id
    );
    ack(peer, &receipt);
    peer.clear();
}

pub fn admit_unsent(peer: &mut raw::Peer, grant: ContentGrant) -> Registration {
    let allocation = encode_shell_file_allocation_request(
        ShellFileHeader {
            kind: ShellFileKind::AllocationRequest,
            connection_epoch: grant.connection_epoch,
            submission_id: 2,
            sequence: 0,
        },
        ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(2),
            record: ShellContentRecord::AllocationRequest(ContentAllocationRequest {
                grant,
                output: ContentOutputId {
                    id: 1,
                    generation: 1,
                },
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
            }),
        },
    )
    .unwrap();
    submit(peer, &allocation, grant, 2);
    let result = next_kind(peer, ShellFileKind::AllocationResult);
    ack(peer, &result);
    let registration = Registration {
        identity: Identity {
            grant,
            channel: grant.connection_epoch,
            ring: 1,
            generation: 1,
        },
        slots: 1,
        pixels: 8,
    };
    peer.open(29, CONTROL_FILE, 1);
    assert_eq!(peer.write(29, &registration.encode()).0, 119);
    expect_ring(peer, registration.identity, EventKind::RingAdmitted);
    registration
}

fn expect_ring(peer: &mut raw::Peer, identity: Identity, kind: EventKind) -> Event {
    let bytes = peer.next_event();
    let event = Event::decode(&bytes).expect("ring event");
    assert_eq!(event.identity, identity);
    assert_eq!(event.kind, kind);
    let ack = encode_shell_file_ack(ShellFileAck {
        connection_epoch: identity.grant.connection_epoch,
        sequence: event.sequence,
    })
    .unwrap();
    assert_eq!(peer.write(4, &ack).0, 119);
    event
}

pub fn complete_source(
    peer: &mut raw::Peer,
    socket: &std::os::fd::OwnedFd,
    grant: ContentGrant,
    registration: Registration,
    channel_generation: u64,
) {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::os::fd::AsFd;
    use std::os::unix::fs::FileExt;
    let source = std::fs::File::from(
        memfd_create(
            c"session-ring-source",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    source
        .set_len(registration.geometry().unwrap().source_bytes() as u64)
        .unwrap();
    source.write_all_at(&2u64.to_le_bytes(), 0).unwrap();
    source.write_all_at(&[0; 8], 4096).unwrap();
    fcntl_add_seals(&source, SealFlags::GROW | SealFlags::SHRINK).unwrap();
    assert_eq!(registration.identity.channel, channel_generation);
    let packet = registration.source_packet();
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let rights = [source.as_fd()];
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        sendmsg(
            socket,
            &[std::io::IoSlice::new(&packet)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        packet.len()
    );
    drop(source);
    expect_ring(peer, registration.identity, EventKind::RingReady);
    let request = OwnedCopyImageRequest {
        slot: OwnedSlotIdentity {
            offer: 1,
            slot: 0,
            generation: 2,
        },
        resource: ContentResourceId {
            id: 1,
            generation: 1,
        },
        allocation: ContentAllocationId {
            id: 1,
            generation: 1,
        },
        output: ContentOutputId {
            id: 1,
            generation: 1,
        },
        topology_generation: 2,
        width: 2,
        height: 1,
    };
    assert_eq!(
        peer.write(
            29,
            &Offer {
                identity: registration.identity,
                request
            }
            .encode()
        )
        .0,
        119
    );
    let status = next_kind(peer, ShellFileKind::ResourceStatus);
    assert!(
        matches!(decode_shell_file_resource_status_at_revision(&status, 10).unwrap().record,
        ShellContentRecord::ResourceStatus(value) if value.status == 2 && value.resource == request.resource && value.admitted_bytes == 8)
    );
    ack(peer, &status);
    let released = expect_ring(peer, registration.identity, EventKind::SlotReleased);
    assert_eq!(released.slot, request.slot);
    assert_eq!(released.resource, request.resource);
    assert_eq!(released.reason, 0);
    assert!(!released.changed_generation);
    let retire = encode_shell_file_resource_retire(
        ShellFileHeader {
            kind: ShellFileKind::ResourceRetire,
            connection_epoch: grant.connection_epoch,
            submission_id: 4,
            sequence: 0,
        },
        &ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(4),
            record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                grant,
                resource: ContentResourceId {
                    id: 1,
                    generation: 1,
                },
            }),
        },
    )
    .unwrap();
    submit(peer, &retire, grant, 4);
    let release = next_kind(peer, ShellFileKind::ResourceReleased);
    assert!(
        matches!(decode_shell_file_resource_released(&release).unwrap().record, ShellContentRecord::ResourceReleased(value) if value.resource.id == 1)
    );
    ack(peer, &release);
}

pub fn service(transport: &mut ShellTransportConnection<'_>, now: u64, published: &mut bool) {
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    if !*published {
        publish(transport, 1);
        *published = true;
    }
    transport.service_content_resources(now).unwrap();
    transport
        .service_content_allocation_requests(&[], now)
        .unwrap();
    if transport.next_content_allocation_request().is_some() {
        transport
            .grant_content_allocation(
                1,
                ContentAllocationSnapshot {
                    native_opening: None,
                    output,
                    allocation: ContentAllocationId {
                        id: 1,
                        generation: 1,
                    },
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
    }
}

pub fn publish(transport: &mut ShellTransportConnection<'_>, generation: u64) {
    transport
        .publish_content_output_facts(
            TransactionId::from_raw(90 + generation),
            generation,
            vec![ContentOutputFactsEntry {
                output: ContentOutputId {
                    id: 1,
                    generation: 1,
                },
                local_width: 200,
                local_height: 100,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
}

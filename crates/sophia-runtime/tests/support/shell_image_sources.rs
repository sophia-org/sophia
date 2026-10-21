//! Ring registration, malformed sources, immutable accepted copy and retirement.
use super::*;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::FileExt;

fn registration() -> Registration {
    Registration {
        identity: Identity {
            grant: GRANT,
            channel: 2,
            ring: 1,
            generation: 1,
        },
        slots: 1,
        pixels: 8,
    }
}

fn event(peer: &mut raw::Peer, kind: EventKind) -> Event {
    let bytes = peer.next_event();
    let value = Event::decode(&bytes).unwrap();
    assert_eq!(value.identity, registration().identity);
    assert_eq!(value.kind, kind);
    ack(peer, &bytes);
    value
}

pub(super) fn run(
    peer: &mut raw::Peer,
    socket: &OwnedFd,
    case: Case,
    expire: std::sync::mpsc::Sender<Signal>,
) {
    peer.open(7, CONTROL_FILE, 1);
    assert_eq!(peer.write(7, &registration().encode()).0, 119);
    event(peer, EventKind::RingAdmitted);
    if case == Case::Timeout {
        expire.send(Signal::Expire).unwrap();
        let refused = event(peer, EventKind::RingRefused);
        assert_eq!(refused.reason, ContentReason::Timeout as u16);
        // Refusal is final: no second Register can recover this connection.
        let reply = peer.write(
            7,
            &Registration {
                identity: Identity {
                    ring: 2,
                    ..registration().identity
                },
                ..registration()
            }
            .encode(),
        );
        assert_eq!(reply.0, 7);
        return;
    }
    let source = std::fs::File::from(
        rustix::fs::memfd_create(
            c"ring-transport-source",
            rustix::fs::MemfdFlags::CLOEXEC | rustix::fs::MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    let extent = registration().geometry().unwrap().source_bytes() as u64;
    source
        .set_len(if case == Case::WrongExtent {
            extent + 4096
        } else {
            extent
        })
        .unwrap();
    source
        .write_all_at(
            &if case == Case::StaleGeneration {
                4u64
            } else {
                2u64
            }
            .to_le_bytes(),
            0,
        )
        .unwrap();
    source.write_all_at(&[7; 8], 4096).unwrap();
    if case != Case::Unsealed {
        rustix::fs::fcntl_add_seals(
            &source,
            rustix::fs::SealFlags::GROW | rustix::fs::SealFlags::SHRINK,
        )
        .unwrap();
    }
    let bytes = registration().source_packet();
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = rustix::net::SendAncillaryBuffer::new(&mut space);
    let rights = [source.as_fd()];
    if case != Case::MissingFd {
        assert!(control.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)));
    }
    assert_eq!(
        rustix::net::sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            rustix::net::SendFlags::NOSIGNAL
        )
        .unwrap(),
        bytes.len()
    );
    if matches!(case, Case::MissingFd | Case::Unsealed) {
        let bytes = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 10).unwrap();
        assert_eq!(record.header.kind, ShellFileKind::ChannelClosed);
        assert_eq!(
            ShellChannelClosed::decode(record.body).unwrap().reason,
            ShellChannelClosedReason::ProtocolFault
        );
        ack(peer, &bytes);
        return;
    }
    if case == Case::WrongExtent {
        assert_eq!(
            event(peer, EventKind::RingRefused).reason,
            ContentReason::Malformed as u16
        );
        return;
    }
    event(peer, EventKind::RingReady);
    if case == Case::DuplicateSource {
        let mut control = rustix::net::SendAncillaryBuffer::new(&mut space);
        assert!(control.push(rustix::net::SendAncillaryMessage::ScmRights(&rights)));
        rustix::net::sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            rustix::net::SendFlags::NOSIGNAL,
        )
        .unwrap();
        let bytes = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 10).unwrap();
        assert_eq!(record.header.kind, ShellFileKind::ChannelClosed);
        assert_eq!(
            ShellChannelClosed::decode(record.body).unwrap().reason,
            ShellChannelClosedReason::ProtocolFault
        );
        ack(peer, &bytes);
        return;
    }
    if case == Case::Disconnect {
        return;
    }
    let offer = Offer {
        identity: registration().identity,
        request: OwnedCopyImageRequest {
            slot: OwnedSlotIdentity {
                offer: 1,
                slot: 0,
                generation: 2,
            },
            resource: RESOURCE,
            allocation: ContentAllocationId {
                id: 1,
                generation: 1,
            },
            output: ContentOutputId {
                id: 1,
                generation: 1,
            },
            topology_generation: 1,
            width: 2,
            height: 1,
        },
    };
    assert_eq!(peer.write(7, &offer.encode()).0, 119);
    let bytes = peer.next_event();
    let status = decode_shell_file_resource_status_at_revision(&bytes, 10).unwrap();
    let ShellContentRecord::ResourceStatus(value) = status.record else {
        panic!("status expected")
    };
    assert_eq!(value.resource, RESOURCE);
    assert_eq!(status.transaction.raw(), 1);
    assert_eq!(
        value.status,
        if case == Case::StaleGeneration { 3 } else { 2 }
    );
    ack(peer, &bytes);
    // Reading in wire order, rather than separate typed queues, proves the
    // status precedes the producer's permission to overwrite the slot.
    let release = event(peer, EventKind::SlotReleased);
    assert_eq!(release.slot, offer.request.slot);
    assert_eq!(release.resource, RESOURCE);
    assert_eq!(release.changed_generation, case == Case::StaleGeneration);
    if case == Case::StaleGeneration {
        return;
    }
    assert_eq!(release.reason, 0);
    source.write_all_at(&[0xff; 8], 4096).unwrap();
    let (done, checked) = std::sync::mpsc::channel();
    expire.send(Signal::VerifyCopy(done)).unwrap();
    checked.recv_timeout(Duration::from_secs(3)).unwrap();
    // The source can change after release; accepted content remains owned.
    // Retire then proves the independent resource release remains usable.
    let retire = encode_shell_file_resource_retire(
        ShellFileHeader {
            kind: ShellFileKind::ResourceRetire,
            connection_epoch: 1,
            submission_id: 2,
            sequence: 0,
        },
        &ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(99),
            record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                grant: GRANT,
                resource: RESOURCE,
            }),
        },
    )
    .unwrap();
    peer.submit_acknowledged(&retire, 2);
    let bytes = peer.next_event();
    let released = decode_shell_file_resource_released(&bytes).unwrap();
    assert!(
        matches!(released.record,ShellContentRecord::ResourceReleased(value) if value.resource == RESOURCE)
    );
    ack(peer, &bytes);
    byte_upload_is_refused(peer);
}

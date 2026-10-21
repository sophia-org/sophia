//! Accepted sealed sources, retirement and late-packet controls.
use super::*;

pub(super) fn accepted_source(
    peer: &mut raw::Peer,
    socket: &std::os::fd::OwnedFd,
    publication: Option<std::sync::mpsc::Receiver<()>>,
) {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::os::fd::AsFd;
    let admitted = peer.next_event();
    let record = decode_shell_file_record_at_revision(&admitted, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ImageAdmitted);
    let metadata = ShellImageMetadata::decode(record.body).unwrap();
    assert_eq!(metadata.offer_id, 1);
    ack(peer, &admitted);
    let source = std::fs::File::from(
        memfd_create(
            c"live-sealed-source",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    source.set_len(8).unwrap();
    fcntl_add_seals(
        &source,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    let mut planes = [ShellImagePlane {
        offset: 0,
        stride: 0,
        fd_index: 0,
    }; 4];
    planes[0].stride = 8;
    let bytes = ShellImagePacket::Source {
        connection_epoch: 1,
        channel_generation: 2,
        metadata,
        planes,
    }
    .encode()
    .unwrap();
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let rights = [source.as_fd()];
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        200
    );
    drop(source);
    // No further 9P request or ACK until the owner proves local eligibility.
    if let Some(publication) = publication {
        publication.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    let (mut status, mut released) = (false, false);
    for _ in 0..2 {
        let event = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&event, ShellFileClass::Event, 9).unwrap();
        match record.header.kind {
            ShellFileKind::ResourceStatus => {
                assert!(!status);
                let value = decode_shell_file_resource_status(&event).unwrap();
                assert!(
                    matches!(value.record, ShellContentRecord::ResourceStatus(value) if value.status == 2 && value.resource.id == 1)
                );
                status = true;
            }
            ShellFileKind::SourceReleased => {
                assert!(!released);
                assert!(status, "terminal status must precede SourceReleased");
                assert_eq!(
                    ShellSourceReleased::decode(record.body)
                        .unwrap()
                        .disposition,
                    ShellSourceDisposition::Retained
                );
                released = true;
            }
            _ => panic!("unexpected image event"),
        }
        ack(peer, &event);
    }
    assert!(status && released);
    // An expired/completed offer is a silent drain, even with a new source FD.
    // The following ordinary Retire must see no spurious ChannelClosed first.
    let late = std::fs::File::open("/dev/null").unwrap();
    let rights = [late.as_fd()];
    let mut control = SendAncillaryBuffer::new(&mut space);
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        200
    );
    let retire = encode_shell_file_resource_retire(
        ShellFileHeader {
            kind: ShellFileKind::ResourceRetire,
            connection_epoch: 1,
            submission_id: 3,
            sequence: 0,
        },
        &ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(3),
            record: ShellContentRecord::ResourceRetire(ContentResourceRetire {
                grant: ContentGrant {
                    connection_epoch: 1,
                    content_grant_epoch: 1,
                },
                resource: ContentResourceId {
                    id: 1,
                    generation: 1,
                },
            }),
        },
    )
    .unwrap();
    peer.submit_acknowledged(&retire, 3);
    let event = peer.next_event();
    assert!(
        matches!(decode_shell_file_resource_released(&event).unwrap().record,
        ShellContentRecord::ResourceReleased(value) if value.resource.id == 1)
    );
    ack(peer, &event);
    // Even a stale offer must pass descriptor shape first. Extra rights cannot
    // hide behind the silent-drain rule, and both received references close.
    let rights = [late.as_fd(), late.as_fd()];
    let mut larger = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut control = SendAncillaryBuffer::new(&mut larger);
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        sendmsg(
            socket,
            &[std::io::IoSlice::new(&bytes)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        200
    );
    let closed = peer.next_event();
    let record = decode_shell_file_record_at_revision(&closed, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ChannelClosed);
    assert_eq!(
        ShellChannelClosed::decode(record.body).unwrap().reason,
        ShellChannelClosedReason::ProtocolFault
    );
    ack(peer, &closed);
}

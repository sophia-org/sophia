//! Real wire cancellation with caller-controlled completion collection.
use super::*;

pub(super) fn cancelled_source(peer: &mut raw::Peer) {
    let admitted = peer.next_event();
    let record = decode_shell_file_record_at_revision(&admitted, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ImageAdmitted);
    let metadata = ShellImageMetadata::decode(record.body).unwrap();
    ack(peer, &admitted);
    cancel(peer, &metadata);
    settled(peer);
}

fn cancel(peer: &mut raw::Peer, metadata: &ShellImageMetadata) {
    let cancel = encode_shell_file_resource_cancel(
        ShellFileHeader {
            kind: ShellFileKind::ResourceCancel,
            connection_epoch: 1,
            submission_id: 3,
            sequence: 0,
        },
        &ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(metadata.offer_id),
            record: ShellContentRecord::ResourceCancel(ContentResourceCancel {
                grant: ContentGrant {
                    connection_epoch: 1,
                    content_grant_epoch: 1,
                },
                resource: ContentResourceId {
                    id: metadata.resource,
                    generation: metadata.resource_generation,
                },
            }),
        },
    )
    .unwrap();
    peer.submit_acknowledged(&cancel, 3);
}

fn settled(peer: &mut raw::Peer) {
    let (mut status, mut release) = (false, false);
    for _ in 0..2 {
        let bytes = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 9).unwrap();
        match record.header.kind {
            ShellFileKind::ResourceStatus => {
                assert!(!status);
                assert!(
                    matches!(decode_shell_file_resource_status(&bytes).unwrap().record,
                    ShellContentRecord::ResourceStatus(value) if value.status == 4)
                );
                status = true;
            }
            ShellFileKind::SourceReleased => {
                assert!(!release);
                assert_eq!(
                    ShellSourceReleased::decode(record.body)
                        .unwrap()
                        .disposition,
                    ShellSourceDisposition::CancelledBeforeImport
                );
                release = true;
            }
            _ => panic!("unexpected cancellation event"),
        }
        ack(peer, &bytes);
    }
    assert!(status && release);
}

pub(super) fn submitted_source(
    peer: &mut raw::Peer,
    socket: &std::os::fd::OwnedFd,
    submitted: std::sync::mpsc::Receiver<()>,
    collect: std::sync::mpsc::Sender<()>,
) {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let admitted = peer.next_event();
    let record = decode_shell_file_record_at_revision(&admitted, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ImageAdmitted);
    let metadata = ShellImageMetadata::decode(record.body).unwrap();
    ack(peer, &admitted);
    let source = std::fs::File::from(
        memfd_create(
            c"cancelled-sealed-source",
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
    send(socket, &bytes, &source);
    submitted.recv_timeout(Duration::from_secs(3)).unwrap();
    cancel(peer, &metadata);
    // The real worker may have finished scanning. The supplied owner schedule
    // withholds collection, so cancellation cannot invent its completion.
    peer.no_event();
    collect.send(()).unwrap();
    settled(peer);
    let late = std::fs::File::open("/dev/null").unwrap();
    send(socket, &bytes, &late);
    peer.no_event();
}

fn send(socket: &std::os::fd::OwnedFd, bytes: &[u8], source: &std::fs::File) {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::os::fd::AsFd;
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let rights = [source.as_fd()];
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    assert_eq!(
        sendmsg(
            socket,
            &[std::io::IoSlice::new(bytes)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        200
    );
}

//! Real wire cancellation with caller-controlled completion collection.
use super::*;

pub(super) fn isolated_shutdown() {
    const CHILD: &str = "SOPHIA_SHELL_IMAGE_SHUTDOWN_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "disconnected_transport_reclaims_uncollected_source_custody",
                "--exact",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("transport shutdown child stalled");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    // The child runs this test alone, so unrelated test FDs cannot alter the
    // baseline. The worker may have completed, but collection is withheld.
    let descriptors = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let baseline = descriptors();
    run_image_epoch(false, false, true, 6);
    let deadline = Instant::now() + Duration::from_secs(3);
    while descriptors() != baseline {
        assert!(
            Instant::now() < deadline,
            "transport/worker retained descriptors"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn cancelled_source(peer: &mut raw::Peer) {
    let admitted = peer.next_event();
    let record = decode_shell_file_record_at_revision(&admitted, ShellFileClass::Event, 9).unwrap();
    assert_eq!(record.header.kind, ShellFileKind::ImageAdmitted);
    let metadata = ShellImageMetadata::decode(record.body).unwrap();
    ack(peer, &admitted);
    cancel(peer, &metadata);
    settled(peer, false);
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

fn settled(peer: &mut raw::Peer, expired: bool) {
    let (mut status, mut release) = (false, false);
    for _ in 0..2 {
        let bytes = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&bytes, ShellFileClass::Event, 9).unwrap();
        match record.header.kind {
            ShellFileKind::ResourceStatus => {
                assert!(!status);
                let ShellContentRecord::ResourceStatus(value) =
                    decode_shell_file_resource_status(&bytes).unwrap().record
                else {
                    unreachable!()
                };
                assert_eq!(value.status, if expired { 3 } else { 4 });
                if expired {
                    assert_eq!(value.reason, ContentReason::Timeout as u16);
                }
                status = true;
            }
            ShellFileKind::SourceReleased => {
                assert!(!release);
                assert!(status, "terminal status must precede SourceReleased");
                assert_eq!(
                    ShellSourceReleased::decode(record.body)
                        .unwrap()
                        .disposition,
                    if expired {
                        ShellSourceDisposition::RefusedBeforeUse
                    } else {
                        ShellSourceDisposition::CancelledBeforeImport
                    }
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
    expired: bool,
) {
    let (metadata, bytes) = submit_source(peer, socket, submitted);
    cancel(peer, &metadata);
    // The real worker may have finished scanning. The supplied owner schedule
    // withholds collection, so cancellation cannot invent its completion.
    peer.no_event();
    collect.send(()).unwrap();
    settled(peer, expired);
    let late = std::fs::File::open("/dev/null").unwrap();
    send(socket, &bytes, &late);
    peer.no_event();
}

pub(super) fn submit_source(
    peer: &mut raw::Peer,
    socket: &std::os::fd::OwnedFd,
    submitted: std::sync::mpsc::Receiver<()>,
) -> (ShellImageMetadata, Vec<u8>) {
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
    (metadata, bytes)
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

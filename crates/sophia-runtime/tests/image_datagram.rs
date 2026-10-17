#![cfg(target_os = "linux")]

use std::io::IoSlice;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;

use rustix::fs::{MemfdFlags, memfd_create};
use rustix::io::{Errno, FdFlags, fcntl_getfd};
use rustix::net::*;
use sophia_runtime::image_channel::{ImageChannelListener, receive_image_datagram};

fn send(socket: &OwnedFd, bytes: &[u8], descriptors: &[OwnedFd]) {
    let fds: Vec<_> = descriptors.iter().map(AsFd::as_fd).collect();
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(8))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !fds.is_empty() {
        assert!(control.push(SendAncillaryMessage::ScmRights(&fds)));
    }
    assert_eq!(
        sendmsg(
            socket,
            &[IoSlice::new(bytes)],
            &mut control,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        bytes.len()
    );
}

#[test]
fn bounded_receive_closes_truncated_rights_and_transfers_valid_references_once() {
    // A single test makes the FD-table baseline independent of test concurrency.
    let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let baseline = count();
    {
        let (sender, receiver) = socketpair(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            None,
        )
        .unwrap();
        assert!(receive_image_datagram(receiver.as_fd()).unwrap().is_none());
        let sources: Vec<_> = (0..8)
            .map(|_| memfd_create("image-datagram", MemfdFlags::CLOEXEC).unwrap())
            .collect();
        let stable = count();
        for (bytes, fds) in [(vec![0; 201], 1), (vec![0; 200], 8), (vec![], 1)] {
            send(&sender, &bytes, &sources[..fds]);
            assert!(matches!(
                receive_image_datagram(receiver.as_fd()),
                Err(Errno::PROTO)
            ));
            assert_eq!(count(), stable);
        }
        send(&sender, &[7; 200], &sources[..4]);
        let mut packet = receive_image_datagram(receiver.as_fd()).unwrap().unwrap();
        assert_eq!(packet.bytes(), &[7; 200]);
        assert_eq!(packet.descriptor_count(), 4);
        let fd = packet.take_descriptor(0).unwrap();
        assert!(fcntl_getfd(&fd).unwrap().contains(FdFlags::CLOEXEC));
        assert!(packet.take_descriptor(0).is_none());
        drop(packet);
        assert_eq!(count(), stable + 1);
        drop(fd);
        assert_eq!(count(), stable);
        send(&sender, &[1; 40], &[]);
        assert_eq!(
            receive_image_datagram(receiver.as_fd())
                .unwrap()
                .unwrap()
                .bytes(),
            &[1; 40]
        );
        drop(sender);
        assert!(matches!(
            receive_image_datagram(receiver.as_fd()),
            Err(Errno::PROTO)
        ));
    }
    assert_eq!(count(), baseline);
    let path = std::env::temp_dir().join(format!("sophia-image-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(ImageChannelListener::bind(&path).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    {
        let listener = ImageChannelListener::bind(&path).unwrap();
        assert!(listener.accept().unwrap().is_none());
        // A second bind must not unlink the first listener.
        assert!(ImageChannelListener::bind(&path).is_err());
        let client = socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            None,
        )
        .unwrap();
        connect(
            &client,
            &SocketAddrUnix::new(path.join("image.sock")).unwrap(),
        )
        .unwrap();
        let accepted = listener.accept().unwrap().unwrap();
        send(&client, &[3; 40], &[]);
        assert_eq!(
            receive_image_datagram(accepted.as_fd())
                .unwrap()
                .unwrap()
                .bytes(),
            &[3; 40]
        );
    }
    assert!(!path.join("image.sock").exists());
    std::fs::remove_dir(path).unwrap();
    assert_eq!(count(), baseline);
}

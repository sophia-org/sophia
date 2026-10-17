//! Kernel-generated control messages must not bypass the SCM_RIGHTS policy.
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

#[test]
fn unexpected_credentials_and_pidfds_are_refused_without_installed_fd_leaks() {
    let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let before = count();
    for option in [libc::SO_PASSCRED, libc::SO_PASSPIDFD] {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        let enabled: libc::c_int = 1;
        // SAFETY: setsockopt reads a live c_int of the exact advertised size.
        let result = unsafe {
            libc::setsockopt(
                receiver.as_raw_fd(),
                libc::SOL_SOCKET,
                option,
                (&enabled as *const libc::c_int).cast(),
                std::mem::size_of_val(&enabled) as libc::socklen_t,
            )
        };
        assert_eq!(result, 0, "{}", std::io::Error::last_os_error());
        let baseline = count();
        sender.write_all(b"packet").unwrap();
        let mut bytes = [0; 16];
        let error = match sophia_linux_peer::receive_rights(receiver.as_fd(), &mut bytes) {
            Err(error) => error,
            Ok(_) => panic!("unexpected ancillary message accepted"),
        };
        assert_eq!(error.raw_os_error(), Some(libc::EPROTO));
        assert_eq!(count(), baseline);
    }
    seqpacket_refusals();
    assert_eq!(count(), before);
}

fn seqpacket_refusals() {
    let mut pair = [-1; 2];
    // SAFETY: pair has room for both newly owned descriptors.
    assert_eq!(
        unsafe {
            libc::socketpair(
                libc::AF_UNIX,
                libc::SOCK_SEQPACKET | libc::SOCK_CLOEXEC,
                0,
                pair.as_mut_ptr(),
            )
        },
        0
    );
    // SAFETY: successful socketpair installed two unique references.
    let (sender, receiver) =
        unsafe { (OwnedFd::from_raw_fd(pair[0]), OwnedFd::from_raw_fd(pair[1])) };
    let source = std::fs::File::open("/dev/null").unwrap();
    let baseline = std::fs::read_dir("/proc/self/fd").unwrap().count();
    for (size, rights) in [(201, 1), (40, 5)] {
        let mut payload = vec![0_u8; size];
        let mut control = [0_u64; 8];
        let mut iov = libc::iovec {
            iov_base: payload.as_mut_ptr().cast(),
            iov_len: size,
        };
        // SAFETY: zero initializes an empty header; all pointers below remain
        // live through sendmsg, and the aligned control buffer holds five FDs.
        unsafe {
            let mut msg: libc::msghdr = std::mem::zeroed();
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen =
                libc::CMSG_SPACE((rights * std::mem::size_of::<libc::c_int>()) as _) as _;
            let header = libc::CMSG_FIRSTHDR(&msg);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len =
                libc::CMSG_LEN((rights * std::mem::size_of::<libc::c_int>()) as _) as _;
            for index in 0..rights {
                std::ptr::write_unaligned(
                    libc::CMSG_DATA(header).cast::<libc::c_int>().add(index),
                    source.as_raw_fd(),
                );
            }
            assert_eq!(
                libc::sendmsg(sender.as_raw_fd(), &msg, libc::MSG_NOSIGNAL),
                size as isize
            );
        }
        let error = match sophia_linux_peer::receive_rights(receiver.as_fd(), &mut [0; 200]) {
            Err(error) => error,
            Ok(_) => panic!("oversized datagram or rights set accepted"),
        };
        assert_eq!(error.raw_os_error(), Some(libc::EPROTO));
        assert_eq!(
            std::fs::read_dir("/proc/self/fd").unwrap().count(),
            baseline
        );
    }
}

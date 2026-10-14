use std::io::{Read, Write};
use std::os::{fd::AsRawFd, unix::net::UnixStream};

#[test]
fn inherited_duplicate_survives_original_close_and_is_close_on_exec() {
    let (original, mut peer) = UnixStream::pair().unwrap();
    let duplicate = sophia_linux_peer::duplicate_inherited_fd(original.as_raw_fd()).unwrap();
    assert_ne!(duplicate.as_raw_fd(), original.as_raw_fd());
    // SAFETY: F_GETFD only queries this live owned descriptor.
    assert_ne!(
        unsafe { libc::fcntl(duplicate.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
        0
    );
    drop(original);
    let mut duplicate = UnixStream::from(duplicate);
    peer.write_all(b"launch").unwrap();
    let mut bytes = [0; 6];
    duplicate.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"launch");
}

#[test]
fn invalid_inherited_number_is_rejected() {
    assert_eq!(
        sophia_linux_peer::duplicate_inherited_fd(-1)
            .unwrap_err()
            .raw_os_error(),
        Some(libc::EBADF),
    );
}

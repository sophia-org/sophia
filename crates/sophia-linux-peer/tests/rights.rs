//! Kernel-generated control messages must not bypass the SCM_RIGHTS policy.
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd};
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
    assert_eq!(count(), before);
}

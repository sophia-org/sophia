use sophia_linux_peer::SealedMapping;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

fn source(seals: libc::c_int) -> OwnedFd {
    // SAFETY: the name is NUL-terminated; success transfers a new descriptor.
    let fd = unsafe {
        libc::memfd_create(
            c"sealed-mapping-test".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    assert!(fd >= 0);
    // SAFETY: memfd_create returned a unique owned descriptor.
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    file.write_all(&[1, 2, 3, 255, 4, 5, 6, 255]).unwrap();
    assert_eq!(
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) },
        0
    );
    file.into()
}

#[test]
fn sealed_mapping_survives_producer_close_and_refuses_mutability_and_wrong_sizes() {
    let count = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let baseline = count();
    let required = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    {
        let producer = source(required);
        let received = producer.try_clone().unwrap();
        let mapping = SealedMapping::map(received, 8).unwrap();
        drop(producer);
        assert_eq!(mapping.bytes(), &[1, 2, 3, 255, 4, 5, 6, 255]);
        // Moving the immutable mapping to the validation worker keeps its view.
        std::thread::spawn(move || assert_eq!(mapping.bytes().len(), 8))
            .join()
            .unwrap();
    }
    for missing in [
        libc::F_SEAL_WRITE,
        libc::F_SEAL_GROW,
        libc::F_SEAL_SHRINK,
        libc::F_SEAL_SEAL,
    ] {
        assert!(SealedMapping::map(source(required & !missing), 8).is_err());
        assert_eq!(count(), baseline);
    }
    for length in [0, 7, 9, usize::MAX] {
        assert!(SealedMapping::map(source(required), length).is_err());
        assert_eq!(count(), baseline);
    }
    assert!(SealedMapping::map(std::fs::File::open("/dev/null").unwrap().into(), 8).is_err());
    assert_eq!(count(), baseline);
}

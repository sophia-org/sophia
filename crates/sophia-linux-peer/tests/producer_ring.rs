use sophia_linux_peer::ProducerRing;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;

#[test]
fn retained_mapping_writes_reach_readers_but_fd_cannot_write_or_resize() {
    let mut ring = ProducerRing::create(8192).unwrap();
    let file = std::fs::File::from(ring.source_fd().try_clone_to_owned().unwrap());
    ring.bytes_mut()[4096..4100].copy_from_slice(&[1, 2, 3, 255]);
    let mut pixel = [0; 4];
    file.read_exact_at(&mut pixel, 4096).unwrap();
    assert_eq!(pixel, [1, 2, 3, 255]);
    assert_eq!(
        file.write_at(&[0], 4096).unwrap_err().raw_os_error(),
        Some(libc::EPERM)
    );
    assert!(file.set_len(4096).is_err());
    assert!(file.set_len(16384).is_err());
    // SAFETY: valid live fd and bounded range; FUTURE_WRITE must also refuse
    // hole punching, which would otherwise mutate an outstanding Rust view.
    assert_eq!(
        unsafe {
            libc::fallocate(
                file.as_raw_fd(),
                libc::FALLOC_FL_PUNCH_HOLE | libc::FALLOC_FL_KEEP_SIZE,
                4096,
                4096,
            )
        },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
    // SAFETY: fcntl only queries a live descriptor.
    let seals = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GET_SEALS) };
    assert_eq!(
        seals,
        libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_FUTURE_WRITE | libc::F_SEAL_SEAL
    );
    // SAFETY: kernel-chosen address; this mapping must fail under FUTURE_WRITE.
    let alias = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            8192,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    };
    if alias != libc::MAP_FAILED {
        // SAFETY: dispose the unexpected mapping before failing the test.
        unsafe {
            libc::munmap(alias, 8192);
        }
        panic!("an externally writable mapping was admitted");
    }
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
    ring.bytes_mut()[4096] = 9;
    file.read_exact_at(&mut pixel, 4096).unwrap();
    assert_eq!(pixel, [9, 2, 3, 255]);
    drop(ring);
    file.read_exact_at(&mut pixel, 4096).unwrap();
    assert_eq!(pixel, [9, 2, 3, 255]);
}

#[test]
fn producer_mapping_rejects_empty_and_excessive_allocations() {
    assert!(ProducerRing::create(0).is_err());
    assert!(ProducerRing::create(32 * 1024 * 1024 + 1).is_err());
}

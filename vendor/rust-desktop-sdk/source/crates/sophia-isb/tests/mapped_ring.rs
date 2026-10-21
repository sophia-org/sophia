//! MappedRing custody checks against the public builder only.
use sophia_isb::MappedRing;

#[test]
fn mapped_ring_is_writable_and_size_sealed() {
    let mut ring = MappedRing::create(8192).unwrap();
    ring.bytes_mut()[4096..4100].copy_from_slice(&[1, 2, 3, 255]);
    assert_eq!(&ring.bytes_mut()[4096..4100], &[1, 2, 3, 255]);
    // A descriptor write is refused after FUTURE_WRITE sealing.
    assert!(rustix::io::write(ring.source_fd(), b"x").is_err());
    assert!(MappedRing::create(0).is_err());
}

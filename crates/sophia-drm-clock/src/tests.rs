use super::*;

#[test]
fn sequence_uapi_layout_and_full_width_are_preserved() {
    assert_eq!(std::mem::size_of::<RawSequence>(), 24);
    assert_eq!(std::mem::offset_of!(RawSequence, active), 4);
    assert_eq!(std::mem::offset_of!(RawSequence, sequence), 8);
    assert_eq!(std::mem::offset_of!(RawSequence, sequence_ns), 16);
    let sample = decode(RawSequence {
        crtc_id: 42,
        active: 1,
        sequence: u64::MAX,
        sequence_ns: i64::MAX,
    })
    .unwrap();
    assert_eq!(sample.sequence, u64::MAX);
    assert_eq!(sample.timestamp_nsec, i64::MAX as u64);
    assert!(sample.active);
    assert!(!decode(RawSequence::default()).unwrap().active);
    assert!(
        decode(RawSequence {
            sequence_ns: -1,
            ..RawSequence::default()
        })
        .is_err()
    );
    assert!(
        decode(RawSequence {
            active: 2,
            ..RawSequence::default()
        })
        .is_err()
    );
}

#[test]
fn non_drm_descriptor_is_refused_without_consuming_its_input() {
    use std::io::{Read, Write};
    let (mut a, mut b) = std::os::unix::net::UnixStream::pair().unwrap();
    b.write_all(b"intact").unwrap();
    assert_eq!(
        query(&a, 0).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(query(&a, 42).is_err());
    let mut bytes = [0; 6];
    a.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"intact");
}

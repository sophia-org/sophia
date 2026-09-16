//! Descriptor ownership only: ordinary sockets stand in for plane descriptors.
//! No DMA-BUF import, EGL context, device, or rendering is exercised.
use super::*;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

#[test]
fn snapshot_duplicate_keeps_original_on_failed_import_and_survives_original_drop() {
    let (plane, mut peer) = UnixStream::pair().unwrap();
    let original = NativeRendererImageSnapshot {
        image_id: NativeRendererImageId::from_raw(7),
        width: 32,
        height: 8,
        format: 0x3432_5258,
        modifier: 0,
        plane_count: 1,
        planes: [
            Some(NativeOwnedDmaBufPlane {
                fd: plane.into(),
                offset: 16,
                stride: 128,
            }),
            None,
            None,
            None,
        ],
    };
    let refused = original.try_clone().unwrap();
    assert_ne!(
        original.planes[0].as_ref().unwrap().fd.as_raw_fd(),
        refused.planes[0].as_ref().unwrap().fd.as_raw_fd()
    );
    drop(refused); // A refused import must not consume the retained source.
    let mut restored = original.try_clone().unwrap();
    assert_eq!(restored.image_id(), original.image_id());
    assert_eq!(
        (
            restored.width,
            restored.height,
            restored.format,
            restored.modifier,
            restored.plane_count
        ),
        (32, 8, 0x3432_5258, 0, 1)
    );
    assert!(restored.planes[1..].iter().all(Option::is_none));
    let plane = restored.planes[0].take().unwrap();
    assert_eq!((plane.offset, plane.stride), (16, 128));
    drop(original);
    let mut reader = UnixStream::from(plane.fd);
    peer.write_all(b"held").unwrap();
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"held");
}

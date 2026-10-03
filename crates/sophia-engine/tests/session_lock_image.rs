//! t294: a lock provider image is a content image source of its own. It is
//! never equal to shell content, and its identity names the output, the
//! provider connection and the resource, so a replaced provider's image is
//! always a different image.
use std::sync::Arc;

use sophia_engine::*;
use sophia_protocol::*;

fn lock_image(connection_epoch: u64, resource_generation: u64) -> SessionLockImage {
    SessionLockImage {
        identity: SessionLockImageIdentity {
            output: OutputId::from_raw(1),
            connection_epoch,
            resource_id: 7,
            resource_generation,
        },
        width_px: 2,
        height_px: 1,
        pixels: Arc::from(vec![1u8; 8]),
    }
}

fn content(resource: CompositorImageSource) -> CompositorContentImage {
    CompositorContentImage {
        node: CompositorNodeId::SessionLockImage {
            output: OutputId::from_raw(1),
            epoch: 3,
        },
        generation: 1,
        output_size_px: Size {
            width: 2,
            height: 1,
        },
        geometry_px: Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 1,
        },
        size_px: Size {
            width: 2,
            height: 1,
        },
        stride: 8,
        format: u32::from_le_bytes(*b"AR24"),
        resource,
    }
}

#[test]
fn a_lock_image_is_its_own_source_and_identity() {
    let first = CompositorImageSource::Lock(lock_image(4, 1));
    assert_eq!(first.bytes(), &[1u8; 8]);
    assert!(first.shell().is_none(), "never shell content");
    let identity = first.identity();
    assert!(identity.shell().is_none());
    assert_eq!(
        content(first.clone()),
        content(CompositorImageSource::Lock(lock_image(4, 1)))
    );
    // A replacement provider or a new resource generation is a new image.
    for other in [lock_image(5, 1), lock_image(4, 2)] {
        assert_ne!(
            content(first.clone()),
            content(CompositorImageSource::Lock(other))
        );
    }
    // Identities never retain pixels.
    let recorded = content(first).content_identity();
    assert_eq!(recorded.source_bytes, 8);
    assert!(matches!(
        recorded.resource,
        CompositorImageSourceIdentity::Lock(SessionLockImageIdentity {
            connection_epoch: 4,
            ..
        })
    ));
}

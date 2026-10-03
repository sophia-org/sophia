//! t294: lock provider images take a texture handle space of their own, and
//! every part of their identity moves the handle.
use sophia_engine::SessionLockImageIdentity;
use sophia_protocol::OutputId;
use sophia_renderer_live::lock_image_handle;

fn identity(output: u64, epoch: u64, id: u64, generation: u64) -> SessionLockImageIdentity {
    SessionLockImageIdentity {
        output: OutputId::from_raw(output),
        connection_epoch: epoch,
        resource_id: id,
        resource_generation: generation,
    }
}

#[test]
fn lock_handles_keep_to_their_space_and_follow_the_identity() {
    let base = lock_image_handle(identity(1, 4, 7, 1));
    assert_eq!(base >> 62, 0b11, "both top bits mark a lock image");
    for moved in [
        identity(2, 4, 7, 1),
        identity(1, 5, 7, 1),
        identity(1, 4, 8, 1),
        identity(1, 4, 7, 2),
    ] {
        let handle = lock_image_handle(moved);
        assert_eq!(handle >> 62, 0b11);
        assert_ne!(handle, base, "{moved:?}");
    }
    // Shell handles from small resource ids never reach both top bits.
    for id in 1..64u64 {
        for generation in 1..8u64 {
            let shell = id.rotate_left(17) ^ generation ^ (1 << 63);
            assert_ne!(shell >> 62, 0b11);
            assert_ne!(id >> 62, 0b11, "the CPU path's raw shell ids");
        }
    }
}

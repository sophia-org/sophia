use crate::live_session::renderer_cpu_buffer_update;
use sophia_backend_live::LiveCpuBufferUpdate;
use sophia_protocol::{Rect, Size};
use sophia_x_authority::{
    XAuthorityCpuBufferPatch, XAuthorityCpuBufferPatchBatch, XAuthorityCpuBufferPatchRegion,
    XAuthorityCpuBufferUpdate, XResourceId,
};
use std::sync::Arc;

fn update(batch: bool) -> XAuthorityCpuBufferUpdate {
    let patch = XAuthorityCpuBufferPatch {
        handle: 17,
        drawable: XResourceId::new(17, 1),
        size: Size {
            width: 2,
            height: 2,
        },
        stride: 8,
        format: u32::from_le_bytes(*b"XR24"),
        generation: 2,
        rect: Rect {
            x: 1,
            y: 0,
            width: 1,
            height: 2,
        },
        bytes: vec![1, 2, 3, 4, 5, 6, 7, 8].into(),
    };
    if batch {
        XAuthorityCpuBufferUpdate::PatchBatch(XAuthorityCpuBufferPatchBatch {
            handle: patch.handle,
            drawable: patch.drawable,
            size: patch.size,
            stride: patch.stride,
            format: patch.format,
            generation: patch.generation,
            patches: vec![XAuthorityCpuBufferPatchRegion {
                rect: patch.rect,
                bytes: patch.bytes,
            }],
        })
    } else {
        XAuthorityCpuBufferUpdate::Patch(patch)
    }
}

fn authority_bytes(update: &XAuthorityCpuBufferUpdate) -> &[u8] {
    match update {
        XAuthorityCpuBufferUpdate::Patch(patch) => &patch.bytes,
        XAuthorityCpuBufferUpdate::PatchBatch(batch) => &batch.patches[0].bytes,
        _ => panic!("expected a patch"),
    }
}

fn renderer_bytes(update: &LiveCpuBufferUpdate) -> &[u8] {
    match update {
        LiveCpuBufferUpdate::Patch(patch) => &patch.bytes,
        LiveCpuBufferUpdate::PatchBatch(batch) => &batch.patches[0].bytes,
        _ => panic!("expected a patch"),
    }
}

#[test]
fn shared_cpu_patch_authority_clone_keeps_owned_allocation() {
    for batch in [false, true] {
        let original = update(batch);
        let forwarded = original.clone();
        assert_eq!(original, forwarded);
        assert_eq!(original.payload_bytes(), 8);
        assert_eq!(
            authority_bytes(&original).as_ptr(),
            authority_bytes(&forwarded).as_ptr(),
            "authority forwarding must not copy patch pixels"
        );
    }
}

#[test]
fn shared_cpu_patch_conversion_keeps_owned_allocation() {
    for batch in [false, true] {
        let original = update(batch);
        let converted = renderer_cpu_buffer_update(&original);
        assert_eq!(authority_bytes(&original), renderer_bytes(&converted));
        assert_eq!(
            authority_bytes(&original).as_ptr(),
            renderer_bytes(&converted).as_ptr(),
            "session conversion must not copy patch pixels"
        );
        drop(original);
        assert_eq!(renderer_bytes(&converted), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}

#[test]
fn shared_cpu_patch_renderer_clone_keeps_owned_allocation() {
    for batch in [false, true] {
        let original = renderer_cpu_buffer_update(&update(batch));
        let forwarded = original.clone();
        assert_eq!(original, forwarded);
        assert_eq!(
            renderer_bytes(&original).as_ptr(),
            renderer_bytes(&forwarded).as_ptr(),
            "renderer forwarding must not copy patch pixels"
        );
        drop(original);
        assert_eq!(renderer_bytes(&forwarded), &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}

#[test]
fn shared_cpu_patch_application_keeps_generations_and_releases_payload() {
    use sophia_backend_live::{LiveCpuBufferRegistry, LiveCpuBufferSource};
    for batch in [false, true] {
        let authority = update(batch);
        let original = renderer_cpu_buffer_update(&authority);
        let shared = match &original {
            LiveCpuBufferUpdate::Patch(patch) => &patch.bytes,
            LiveCpuBufferUpdate::PatchBatch(batch) => &batch.patches[0].bytes,
            _ => unreachable!(),
        };
        let lifetime = Arc::downgrade(shared);
        let mut registry = LiveCpuBufferRegistry::new();
        let base = LiveCpuBufferSource {
            handle: 17,
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 8,
            format: u32::from_le_bytes(*b"XR24"),
            generation: 1,
            bytes: Arc::new(vec![0; 16]),
        };
        registry
            .apply(LiveCpuBufferUpdate::Replace(base.clone()))
            .unwrap();
        registry.apply(original.clone()).unwrap();
        let presented = registry.get(17).unwrap().clone();
        assert_eq!(base.bytes.as_slice(), &[0; 16]);
        let expected = [0, 0, 0, 0, 1, 2, 3, 4, 0, 0, 0, 0, 5, 6, 7, 8];
        assert_eq!(presented.bytes.as_slice(), &expected);
        assert_eq!(presented.generation, 2);

        let mut later = original.clone();
        let (generation, bytes) = match &mut later {
            LiveCpuBufferUpdate::Patch(patch) => (&mut patch.generation, &mut patch.bytes),
            LiveCpuBufferUpdate::PatchBatch(batch) => {
                (&mut batch.generation, &mut batch.patches[0].bytes)
            }
            _ => unreachable!(),
        };
        *generation = 3;
        Arc::make_mut(bytes).fill(29);
        assert_eq!(renderer_bytes(&original), &[1, 2, 3, 4, 5, 6, 7, 8]);
        registry.apply(later).unwrap();
        assert_eq!(registry.get(17).unwrap().generation, 3);
        assert_eq!(&registry.get(17).unwrap().bytes[4..8], &[29; 4]);
        assert_eq!(presented.bytes.as_slice(), &expected);
        drop(authority);
        assert!(lifetime.upgrade().is_some());
        drop(original);
        assert!(
            lifetime.upgrade().is_none(),
            "registry must not retain the applied patch"
        );
    }
}

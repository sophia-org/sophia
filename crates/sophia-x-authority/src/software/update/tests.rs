#![cfg(test)]

use super::*;

#[test]
fn packed_cpu_patch_is_independent_of_later_drawable_writes() {
    let mut source = XAuthorityCpuBufferSnapshot {
        handle: 1,
        drawable: XResourceId::new(1, 1),
        size: Size {
            width: 2,
            height: 2,
        },
        stride: 8,
        format: u32::from_le_bytes(*b"XR24"),
        generation: 1,
        bytes: Arc::new((0..16).collect()),
    };
    let rect = Rect {
        x: 1,
        y: 0,
        width: 1,
        height: 2,
    };
    let patch = packed_patch(&source, rect).unwrap();
    let region = packed_patch_region(&source, rect).unwrap();
    let forwarded = patch.clone();
    assert!(Arc::ptr_eq(&patch.bytes, &forwarded.bytes));
    let lifetime = Arc::downgrade(&patch.bytes);
    Arc::make_mut(&mut source.bytes).fill(29);
    assert_eq!(patch.bytes.as_slice(), &[4, 5, 6, 7, 12, 13, 14, 15]);
    assert_eq!(region.bytes, patch.bytes);
    assert_eq!(forwarded, patch);
    drop(patch);
    assert!(lifetime.upgrade().is_some());
    drop(forwarded);
    assert!(lifetime.upgrade().is_none());
}

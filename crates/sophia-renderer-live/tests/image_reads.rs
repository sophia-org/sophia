use sophia_renderer_live::{LiveRendererImageId, LiveRendererImageReads};
#[test]
fn image_read_custody_ends_after_both_the_frame_and_frozen_source_drop() {
    let reads = LiveRendererImageReads::default();
    let image = LiveRendererImageId::from_raw(7);
    let frame = reads.acquire(image);
    let sibling = frame.clone();
    let frozen = reads.acquire(image);
    assert!(!reads.request_eviction(image));
    drop(frame);
    reads.prune();
    assert!(reads.contains(image));
    assert!(reads.ready_evictions().is_empty());
    drop(frozen);
    assert!(reads.contains(image));
    drop(sibling);
    assert!(!reads.contains(image));
    assert_eq!(reads.ready_evictions(), vec![image]);
    assert!(reads.request_eviction(image));
    assert!(reads.ready_evictions().is_empty());
    reads.prune();
    assert!(!reads.has_readers());
}

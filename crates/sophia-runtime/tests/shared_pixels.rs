use sophia_runtime::SharedPixels;
use std::sync::Arc;

#[test]
fn watch_does_not_retain_storage_and_waits_for_every_consumer() {
    let original: Arc<[u8]> = Arc::from([1, 2, 3, 4]);
    let backing = Arc::downgrade(&original);
    let pixels = SharedPixels::from(original.clone());
    let consumer = pixels.clone();
    let watch = pixels.watch();
    drop(pixels);
    assert!(!watch.released());
    drop(original);
    assert!(!watch.released());
    assert_eq!(consumer.as_ref(), &[1, 2, 3, 4]);
    drop(consumer);
    assert!(watch.released());
    assert!(backing.upgrade().is_none());
    assert!(watch.released());
}

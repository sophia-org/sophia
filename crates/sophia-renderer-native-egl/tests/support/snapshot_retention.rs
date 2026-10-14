#![cfg(test)]
use super::*;

#[test]
fn native_reservations_bound_bytes_images_and_final_clone_lifetime() {
    // No GL or devices: reserve large byte counts without allocating pixels.
    let first = SnapshotReservation::reserve(NATIVE_SNAPSHOT_RETAINED_BYTES).unwrap();
    assert!(SnapshotReservation::reserve(1).is_none());
    drop(first);
    let frames: Vec<_> = (0..NATIVE_SNAPSHOT_RETAINED_IMAGES)
        .map(|_| NativeCompositionSnapshot::from_rgba(1, 1, vec![17; 4]).unwrap())
        .collect();
    assert!(NativeCompositionSnapshot::from_rgba(1, 1, vec![0; 4]).is_none());
    let retained = frames[0].clone();
    drop(frames);
    assert_eq!(retained.rgba(), &[17; 4]);
    assert_eq!(*RETAINED.lock().unwrap(), (4, 1));
    drop(retained);
    assert_eq!(*RETAINED.lock().unwrap(), (0, 0));
    let invalid = SnapshotReservation::reserve(4).unwrap();
    assert!(NativeCompositionSnapshot::from_reserved(1, 1, vec![0; 3], invalid).is_none());
    assert_eq!(*RETAINED.lock().unwrap(), (0, 0));
}

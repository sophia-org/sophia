use super::*;

#[test]
fn opened_and_cloned_card_owners_never_share_a_property_identity() {
    // Construction alone opens the supplied file; no DRM query is made.
    let first = RealAtomicScanoutCard::open_nonblocking(Path::new("/dev/null")).unwrap();
    let second = RealAtomicScanoutCard::open_nonblocking(Path::new("/dev/null")).unwrap();
    let clone = first.try_clone().unwrap();
    let id = first.property_identity();
    assert_ne!(id, 0);
    assert_eq!(first.property_identity(), id);
    assert_ne!(id, second.property_identity());
    assert_ne!(id, clone.property_identity());
    assert_ne!(second.property_identity(), clone.property_identity());
}

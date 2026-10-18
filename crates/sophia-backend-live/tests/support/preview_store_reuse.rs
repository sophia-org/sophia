use super::PreviewImages;
use sophia_renderer_live::LiveRendererImageId;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn unchanged_store_inventory_reuses_storage_and_still_reconciles_owners() {
    let mut images = PreviewImages::default();
    let stores = BTreeMap::from([(11, 0), (22, 1)]);
    images.synchronize(1, stores.iter().map(|(&store, &group)| (store, group)));
    // A clone must allocate its replacement while the old map is still live.
    let original_key = std::ptr::from_ref(images.live_stores.first_key_value().unwrap().0);
    let stale = LiveRendererImageId::from_raw(1);
    images.owners.insert(stale, BTreeSet::from([99]));
    images.synchronize(1, stores.iter().map(|(&store, &group)| (store, group)));
    assert_eq!(
        original_key,
        std::ptr::from_ref(images.live_stores.first_key_value().unwrap().0),
        "an unchanged inventory must retain its map allocation"
    );
    assert!(!images.owners.contains_key(&stale));

    images.demand_sources.insert(
        stale,
        (sophia_protocol::SurfaceId::new(3, 1), BTreeSet::new()),
    );
    images.synchronize(2, stores.iter().map(|(&store, &group)| (store, group)));
    assert!(
        images.demand_sources.is_empty(),
        "a new generation still revokes foreign demand"
    );
    assert_eq!(
        original_key,
        std::ptr::from_ref(images.live_stores.first_key_value().unwrap().0),
    );

    // Identity alone is insufficient: a store can change its renderer group.
    let regrouped = BTreeMap::from([(11, 2), (22, 1)]);
    images.synchronize(2, regrouped.iter().map(|(&store, &group)| (store, group)));
    assert_eq!(images.live_stores, regrouped);
    images.synchronize(2, [(22, 1)].into_iter());
    assert_eq!(images.live_stores, BTreeMap::from([(22, 1)]));
    assert_eq!(images.inventory_generation, Some(2));
    images.owners.insert(stale, BTreeSet::from([22]));
    images.retired_imports.insert(22, Vec::new());
    images.synchronize(2, [].into_iter());
    assert!(images.live_stores.is_empty());
    assert!(images.owners.is_empty());
    assert!(images.retired_imports.is_empty());
}

use super::preview_custody::PreviewImages;
use sophia_renderer_live::LiveRendererImageId;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn inventory_refresh_keeps_live_local_owners_and_pending_evictions() {
    let mut images = PreviewImages::default();
    let a = LiveRendererImageId::from_raw(1);
    let b = LiveRendererImageId::from_raw(2);
    let stores = BTreeMap::from([(11, 0), (22, 1)]);
    images.synchronize(1, &stores);
    images.owners.insert(a, BTreeSet::from([11]));
    images.owners.insert(b, BTreeSet::from([22]));
    let queued = images.reads.acquire(a);
    assert!(!images.reads.request_eviction(a));
    images.synchronize(2, &stores);
    assert!(
        images.owners[&a].contains(&11),
        "idle local frame still has its store"
    );
    assert!(images.owners[&b].contains(&22));
    drop(queued);
    assert_eq!(images.reads.ready_evictions(), vec![a]);
    assert!(images.reads.request_eviction(a));
    assert!(images.reads.ready_evictions().is_empty());
    images
        .cold_misses
        .insert(a, BTreeSet::from([sophia_protocol::OutputId::from_raw(2)]));
    images.synchronize(3, &BTreeMap::from([(22, 1)]));
    assert!(
        !images.owners.contains_key(&a),
        "removed store cannot remain a donor"
    );
    assert!(images.owners.contains_key(&b));
    assert!(!images.cold_misses.contains_key(&a));
    let queued = images.reads.acquire(b);
    assert!(!images.reads.request_eviction(b));
    images.invalidate();
    drop(queued);
    assert!(images.owners.is_empty());
    assert!(
        images.reads.ready_evictions().is_empty(),
        "destroyed stores owe no eviction"
    );
}

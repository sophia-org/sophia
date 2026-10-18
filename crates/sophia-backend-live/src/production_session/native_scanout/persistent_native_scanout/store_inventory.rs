use std::collections::BTreeMap;

/// Borrow unchanged inventories, including mirrored heads sharing one store.
/// Conflicting duplicate groups fall back to the existing last-entry-wins map.
pub(super) fn replacement(
    current: &BTreeMap<u64, usize>,
    stores: impl Iterator<Item = (u64, usize)> + Clone,
) -> Option<BTreeMap<u64, usize>> {
    let unchanged = stores
        .clone()
        .all(|(store, group)| current.get(&store) == Some(&group))
        && current
            .keys()
            .all(|store| stores.clone().any(|(candidate, _)| candidate == *store));
    if unchanged {
        None
    } else {
        Some(stores.collect())
    }
}

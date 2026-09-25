//! t100 production allowance selection and registry/resource admission. These
//! controls do not claim transport or native completion.
use sophia_protocol::*;
use sophia_runtime::*;

const MIB: u64 = 1024 * 1024;
const CAP: u64 = 64 * MIB;

fn grant(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}

fn nominal(epoch: u64, staging: u64, resident: u64, retiring: u64) -> ContentLimits {
    let mut limits = ContentLimits::prototype(grant(epoch));
    limits.max_staging_bytes = staging * MIB;
    limits.max_resident_bytes = resident * MIB;
    limits.max_retiring_bytes = retiring * MIB;
    limits.validate().unwrap();
    limits
}

fn source(limits: &ContentLimits) -> u64 {
    limits.max_staging_bytes + limits.max_resident_bytes + limits.max_retiring_bytes
}

// Supply boundary inventories to the production selector. No test-owned copy
// of the selection algorithm remains.
fn proposed_fit(
    nominal: &ContentLimits,
    own_retired_source: u64,
    global_source_free: u64,
    global_backing_free: u64,
) -> Option<ContentLimits> {
    select_reconnect_limits(
        nominal,
        ContentReconnectBudget {
            capacity_bytes: CAP,
            capacity_backing_bytes: CAP,
            reserved_bytes: CAP.checked_sub(global_source_free)?,
            reserved_backing_bytes: CAP.checked_sub(global_backing_free)?,
            own_retired_bytes: own_retired_source,
            ..Default::default()
        },
    )
    .ok()
}

fn begin(g: ContentGrant, id: u64) -> ContentResourceBegin {
    ContentResourceBegin {
        grant: g,
        resource: ContentResourceId { id, generation: 1 },
        width_px: 1,
        height_px: 1,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes: 4,
    }
}

fn upload(store: &mut ContentResourceStore, id: u64) -> ContentResourceLease {
    let g = store.grant();
    let resource = begin(g, id).resource;
    let tx = TransactionId::from_raw(id);
    store.begin(tx, begin(g, id), 0).unwrap();
    store
        .chunk(
            tx,
            &ContentResourceChunk {
                grant: g,
                resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![0; 4],
            },
            0,
        )
        .unwrap();
    store
        .end(
            tx,
            &ContentResourceEnd {
                grant: g,
                resource,
                total_bytes: 4,
                chunk_count: 1,
            },
            0,
        )
        .unwrap();
    while store.take_event().is_some() {}
    store.lease(g, resource).unwrap()
}

#[test]
fn proposed_limits_preserve_nominal_profiles_and_every_nonbyte_relation() {
    for (s, r, t) in [(8, 16, 16), (4, 12, 8), (4, 8, 8)] {
        let nominal = nominal(1, s, r, t);
        assert_eq!(proposed_fit(&nominal, 0, CAP, CAP), Some(nominal.clone()));
        for debt in [
            1,
            3,
            4,
            MIB,
            8 * MIB,
            16 * MIB,
            24 * MIB,
            32 * MIB,
            CAP,
            u64::MAX,
        ] {
            for available in [
                0,
                16 * MIB - 4,
                16 * MIB,
                16 * MIB + 1,
                16 * MIB + 2,
                16 * MIB + 3,
                24 * MIB,
                CAP,
            ] {
                for backing in [0, 12 * MIB - 4, 12 * MIB, 12 * MIB + 1, 20 * MIB, CAP] {
                    if let Some(fit) = proposed_fit(&nominal, debt, available, backing) {
                        fit.validate().unwrap();
                        assert!(source(&fit) <= available);
                        assert!(source(&fit) + debt <= source(&nominal));
                        assert!(fit.max_resident_bytes + fit.max_retiring_bytes <= backing);
                        assert!(fit.max_staging_bytes >= nominal.max_resource_bytes);
                        assert!(fit.max_resident_bytes >= 2 * nominal.max_resource_bytes);
                        assert!(fit.max_retiring_bytes >= nominal.max_resource_bytes);
                        assert_eq!(fit.max_staging_bytes % 4, 0);
                        assert_eq!(fit.max_resident_bytes % 4, 0);
                        assert_eq!(fit.max_retiring_bytes % 4, 0);
                        let mut restored = fit;
                        restored.max_staging_bytes = nominal.max_staging_bytes;
                        restored.max_resident_bytes = nominal.max_resident_bytes;
                        restored.max_retiring_bytes = nominal.max_retiring_bytes;
                        assert_eq!(restored, nominal);
                    }
                }
            }
        }
    }
}

#[test]
fn revocation_aborts_staging_but_keeps_both_pinned_source_classes() {
    let mut registry = ContentEpochRegistry::new(CAP).unwrap();
    registry.admit(nominal(1, 8, 16, 16)).unwrap();
    let store = registry.resources_mut(grant(1)).unwrap();
    let retiring = upload(store, 1);
    store
        .retire(
            TransactionId::from_raw(4),
            &ContentResourceRetire {
                grant: grant(1),
                resource: begin(grant(1), 1).resource,
            },
        )
        .unwrap();
    let resident = upload(store, 2);
    store
        .begin(TransactionId::from_raw(5), begin(grant(1), 3), 0)
        .unwrap();
    assert_eq!(
        store.usage(),
        ContentMemoryUsage {
            staging: 4,
            resident: 4,
            retiring: 4,
            reserved_resident: 4,
            backing: 12,
        }
    );
    assert!(registry.disconnect(grant(1)));
    assert_eq!(
        registry.accounting().memory,
        ContentMemoryUsage {
            staging: 0,
            resident: 4,
            retiring: 4,
            reserved_resident: 0,
            backing: 8,
        }
    );
    assert_eq!(registry.retired_bytes(), 8);
    assert_eq!(registry.retired_backing_bytes(), 8);
    drop(retiring);
    registry.collect();
    assert_eq!(registry.retired_bytes(), 4);
    assert_eq!(resident.bytes(), &[0; 4]);
    drop(resident);
    registry.collect();
    assert!(registry.accounting().quiescent());
}

#[test]
fn both_full_profiles_can_admit_proposed_successors_without_freeing_old_pixels() {
    for reverse in [false, true] {
        let mut registry = ContentEpochRegistry::new(CAP).unwrap();
        let profiles = [
            ContentStoreProfile::Legacy,
            ContentStoreProfile::NativeLauncher,
        ];
        let limits = [nominal(1, 8, 16, 16), nominal(2, 4, 12, 8)];
        let mut leases = Vec::new();
        for (limits, profile) in limits.iter().zip(profiles) {
            registry
                .admit_with_profile(limits.clone(), profile)
                .unwrap();
            leases.push(upload(registry.resources_mut(limits.grant).unwrap(), 1));
        }
        assert_eq!(registry.reserved_bytes(), CAP);
        registry.disconnect(grant(1));
        registry.disconnect(grant(2));
        for (index, slot) in if reverse { [1, 0] } else { [0, 1] }
            .into_iter()
            .enumerate()
        {
            let mut requested = limits[slot].clone();
            requested.grant = grant(3 + index as u64);
            let budget = registry.reconnect_budget(profiles[slot]);
            assert_eq!(
                (budget.own_retired_bytes, budget.own_retired_epochs),
                (4, 1)
            );
            let fit = select_reconnect_limits(&requested, budget).unwrap();
            assert_eq!(fit.max_resource_bytes, 4 * MIB);
            assert_eq!(fit.max_retiring_bytes, requested.max_retiring_bytes - 4);
            registry.admit_with_profile(fit, profiles[slot]).unwrap();
        }
        assert_eq!(registry.reserved_bytes(), CAP);
        assert_eq!(registry.accounting().active_epochs, 2);
        assert_eq!(registry.accounting().retired_epochs, 2);
        assert!(registry.resources(grant(1)).is_none());
        assert!(!registry.disconnect(grant(1)));
        assert_eq!(leases[0].bytes(), &[0; 4]);
        drop(leases);
        registry.collect();
        assert_eq!(registry.accounting().retired_epochs, 0);
        // Active limits do not grow after collection.
        assert_eq!(registry.reserved_bytes(), CAP - 8);
    }
}

#[test]
fn saturation_refuses_useful_floor_without_consuming_an_identity() {
    // Protocol-valid does not imply enough resident capacity for replacement.
    let one_resident = nominal(2, 8, 4, 8);
    assert!(proposed_fit(&one_resident, 0, CAP, CAP).is_none());
    let requested = nominal(2, 8, 16, 16);
    for (reserved_bytes, reserved_backing_bytes, own_retired_bytes) in
        [(u64::MAX, 0, 0), (0, u64::MAX, 0), (0, 0, u64::MAX)]
    {
        assert_eq!(
            select_reconnect_limits(
                &requested,
                ContentReconnectBudget {
                    capacity_bytes: CAP,
                    capacity_backing_bytes: CAP,
                    reserved_bytes,
                    reserved_backing_bytes,
                    own_retired_bytes,
                    ..Default::default()
                }
            ),
            Err(ContentStoreError::Budget)
        );
    }
    assert!(proposed_fit(&requested, 24 * MIB + 4, CAP, CAP).is_none());
    assert!(proposed_fit(&requested, 0, CAP, 12 * MIB - 4).is_none());
    let fit = proposed_fit(&requested, 24 * MIB, CAP, CAP).unwrap();
    assert_eq!(
        (
            fit.max_staging_bytes,
            fit.max_resident_bytes,
            fit.max_retiring_bytes
        ),
        (4 * MIB, 8 * MIB, 4 * MIB)
    );
    let mut registry = ContentEpochRegistry::new(CAP).unwrap();
    registry.admit(nominal(1, 8, 16, 16)).unwrap();
    let before = registry.accounting();
    assert_eq!(
        registry.admit(requested.clone()),
        Err(ContentStoreError::Budget)
    );
    assert_eq!(registry.accounting(), before);
    registry.disconnect(grant(1));
    registry.admit(requested).unwrap();
}

#[test]
fn all_three_slots_keep_their_envelopes_in_every_reconnect_order() {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut registry = ContentEpochRegistry::with_active_capacity(CAP, 3).unwrap();
        let profiles = [
            ContentStoreProfile::Legacy,
            ContentStoreProfile::NativeLauncher,
            ContentStoreProfile::PersistentCatalog,
        ];
        let limits = [
            nominal(1, 4, 12, 8),
            nominal(2, 4, 8, 8),
            nominal(3, 4, 8, 8),
        ];
        let mut leases = Vec::new();
        for (limit, profile) in limits.iter().zip(profiles) {
            registry.admit_with_profile(limit.clone(), profile).unwrap();
            leases.push(upload(registry.resources_mut(limit.grant).unwrap(), 1));
        }
        for epoch in 1..=3 {
            registry.disconnect(grant(epoch));
        }
        for (index, slot) in order.into_iter().enumerate() {
            let mut requested = limits[slot].clone();
            requested.grant = grant(4 + index as u64);
            let fitted =
                select_reconnect_limits(&requested, registry.reconnect_budget(profiles[slot]))
                    .unwrap();
            assert_eq!(source(&fitted) + 4, source(&requested));
            registry.admit_with_profile(fitted, profiles[slot]).unwrap();
        }
        assert_eq!(registry.accounting().active_epochs, 3);
        assert_eq!(registry.reserved_bytes(), CAP);
        assert!(registry.reserved_backing_bytes() <= CAP);
        assert!(leases.iter().all(|lease| lease.bytes() == [0; 4]));
    }
}

#[test]
fn repeated_tightened_grants_need_only_actual_retired_inventory_and_keep_epoch_bound() {
    let mut registry = ContentEpochRegistry::new(CAP).unwrap();
    let mut leases = Vec::new();
    for epoch in 1..=ContentEpochRegistry::MAX_RETAINED_EPOCHS as u64 {
        let requested = nominal(epoch, 8, 16, 16);
        let fit = select_reconnect_limits(
            &requested,
            registry.reconnect_budget(ContentStoreProfile::Legacy),
        )
        .unwrap();
        assert_eq!(source(&fit) + registry.retired_bytes(), source(&requested));
        registry.admit(fit).unwrap();
        leases.push(upload(registry.resources_mut(grant(epoch)).unwrap(), 1));
        registry.disconnect(grant(epoch));
        assert!(registry.retired_bytes() <= source(&requested));
    }
    let requested = nominal(17, 8, 16, 16);
    let fit = select_reconnect_limits(
        &requested,
        registry.reconnect_budget(ContentStoreProfile::Legacy),
    )
    .unwrap();
    let before = registry.accounting();
    assert_eq!(registry.admit(fit.clone()), Err(ContentStoreError::Budget));
    assert_eq!(registry.accounting(), before);
    drop(leases.pop());
    registry.collect();
    registry.admit(fit).unwrap();
    assert_eq!(
        registry.accounting().active_epochs + registry.accounting().retired_epochs,
        16
    );
}

#[test]
fn byte_saturation_recovers_only_after_a_real_source_consumer_ends() {
    let mut registry = ContentEpochRegistry::new(CAP).unwrap();
    let bar = nominal(1, 8, 16, 16);
    registry.admit(bar.clone()).unwrap();
    registry
        .admit_with_profile(nominal(2, 4, 12, 8), ContentStoreProfile::NativeLauncher)
        .unwrap();
    let store = registry.resources_mut(grant(1)).unwrap();
    let mut leases = Vec::new();
    for id in 1..=7 {
        let mut description = begin(grant(1), id);
        description.width_px = 1024;
        description.height_px = 1024;
        description.total_bytes = 4 * MIB;
        // Dense rows: 15 rows fit in the unchanged 65,488-byte chunk ceiling.
        description.chunk_count = 69;
        let layout = description.layout(&bar).unwrap();
        let tx = TransactionId::from_raw(id);
        store.begin(tx, description.clone(), 0).unwrap();
        let mut offset = 0;
        for ordinal in 0..layout.chunk_count {
            let bytes = (layout.total_bytes - offset)
                .min(u64::from(layout.row_bytes) * u64::from(layout.rows_per_chunk));
            store
                .chunk(
                    tx,
                    &ContentResourceChunk {
                        grant: grant(1),
                        resource: description.resource,
                        ordinal,
                        offset,
                        bytes: vec![0; bytes as usize],
                    },
                    0,
                )
                .unwrap();
            offset += bytes;
        }
        store
            .end(
                tx,
                &ContentResourceEnd {
                    grant: grant(1),
                    resource: description.resource,
                    total_bytes: description.total_bytes,
                    chunk_count: layout.chunk_count,
                },
                0,
            )
            .unwrap();
        leases.push(store.lease(grant(1), description.resource).unwrap());
        if id <= 4 {
            store
                .retire(
                    tx,
                    &ContentResourceRetire {
                        grant: grant(1),
                        resource: description.resource,
                    },
                )
                .unwrap();
        }
        while store.take_event().is_some() {}
    }
    registry.disconnect(grant(1));
    assert_eq!(registry.retired_bytes(), 28 * MIB);
    let requested = nominal(3, 8, 16, 16);
    let fit = |registry: &ContentEpochRegistry| {
        select_reconnect_limits(
            &requested,
            registry.reconnect_budget(ContentStoreProfile::Legacy),
        )
        .ok()
    };
    let before = registry.accounting();
    assert!(fit(&registry).is_none());
    registry.collect();
    assert_eq!(registry.accounting(), before);
    assert!(fit(&registry).is_none());
    drop(leases.pop());
    registry.collect();
    assert_eq!(registry.retired_bytes(), 24 * MIB);
    registry.admit(fit(&registry).unwrap()).unwrap();
    assert_eq!(registry.reserved_bytes(), CAP);
    assert!(registry.resources(grant(1)).is_none());
    assert_eq!(leases[0].bytes().len(), 4 * MIB as usize);
}

//! Private native capture lifecycle controls; no device access.
use super::*;

#[test]
fn generation_output_and_store_withdrawal_preserve_custody_and_close_intent() {
    for cause in 0..3 {
        let epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
        let preview_epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
        let mut owner = CaptureOwner::default();
        let custody = owner.custody.clone();
        owner.active = Some(Install {
            output: OutputId::from_raw(1),
            store: 3,
            generation: 2,
            epoch: epoch.clone(),
        });
        owner.observe(2, |_, _| true);
        assert!(!owner.closing);
        owner.observe(if cause == 0 { 3 } else { 2 }, |output, store| {
            if cause == 1 {
                output == OutputId::from_raw(2)
            } else if cause == 2 {
                store == 4
            } else {
                true
            }
        });
        assert!(owner.closing);
        assert!(!epoch.is_valid());
        assert!(preview_epoch.is_valid());
        assert!(
            owner.active.is_some(),
            "queue pressure must retain exact close target"
        );
        assert!(owner.custody.same_owner(&custody));
        owner.observe(2, |_, _| true);
        assert!(
            owner.closing,
            "restored topology is not a close acknowledgment"
        );
    }
}

#[test]
fn retirement_visits_all_inline_and_distinct_worker_stores_without_a_frame() {
    let mut seen = Vec::new();
    evict_all_stores([Some(7), None, Some(7), Some(9), None], |index| {
        seen.push(index);
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(seen, [0, 1, 3, 4]);
}

#[test]
fn failed_store_keeps_acknowledgment_pending_and_retry_revisits_every_store() {
    let mut seen = Vec::new();
    let stores = [Some(7), None, Some(9)];
    assert_eq!(
        evict_all_stores(stores, |index| {
            seen.push(index);
            if index == 1 { Err("busy") } else { Ok(()) }
        }),
        Err("busy")
    );
    assert_eq!(seen, [0, 1]);
    seen.clear();
    evict_all_stores(stores, |index| {
        seen.push(index);
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(seen, [0, 1, 2]);
}

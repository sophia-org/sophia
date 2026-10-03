#![cfg(feature = "gbm-platform")]
use sophia_renderer_native_egl::{
    NativeCompositionDamageRect as Rect, NativeCompositionDamageStats as Stats,
    NativeCompositionRepaintTable as Table, NativeFullRepaintReason as R, NativeRepaintPlan as P,
};

#[test]
fn only_the_selected_age_counts_and_full_reasons_conserve_frames() {
    let rect = Rect {
        x: 1,
        y: 1,
        width: 2,
        height: 3,
    };
    let table = Table::with_evidence(vec![P::Partial(vec![rect]), P::Full(R::PlanFull)], true);
    let mut stats = Stats::default();
    // Plans for two ages existed, but only age one actually rendered.
    stats.observe(
        table.full_reason_for_age(1),
        table.stable_geometry(),
        6,
        100,
    );
    assert_eq!(stats.stable_geometry_partial, 1);
    assert_eq!(stats.stable_geometry_full, 0);
    assert_eq!(stats.full_plan, 0);
    assert_eq!(stats.stable_geometry_repaint_pixels, 6);
    for (age, expected) in [(0, R::UnknownAge), (2, R::PlanFull), (3, R::BeyondHistory)] {
        assert_eq!(table.full_reason_for_age(age), Some(expected));
        assert!(table.damage_for_age(age).is_none());
        stats.observe(
            table.full_reason_for_age(age),
            table.stable_geometry(),
            100,
            100,
        );
    }
    assert_eq!(stats.stable_geometry_frames, 4);
    assert_eq!(stats.stable_geometry_full, 3);
    assert_eq!(
        stats.full_unknown_age + stats.full_plan + stats.full_beyond_history,
        3
    );
    assert_eq!(stats.stable_geometry_repaint_pixels, 306);
    assert_eq!(stats.stable_geometry_target_pixels, 400);
}

#[test]
fn all_full_and_disabled_tables_preserve_the_same_render_decision() {
    for reason in [R::NoTable, R::Disabled, R::NoHistory, R::DamageUnavailable] {
        let table = Table::full(reason);
        for age in [0, 1, 100] {
            assert!(table.damage_for_age(age).is_none());
            assert_eq!(table.full_reason_for_age(age), Some(reason));
            assert!(!table.stable_geometry());
        }
    }
    let table = Table::from_ages(vec![None, None]);
    assert_eq!(table.full_reason_for_age(1), Some(R::PlanFull));
    assert_eq!(table.full_reason_for_age(2), Some(R::PlanFull));
    assert_eq!(table.full_reason_for_age(3), Some(R::BeyondHistory));
}

use super::*;
use sophia_backend_live::{
    LibdrmNativeOutputCapability, LibdrmNativeOutputTiming, LibdrmNativeVrrPropertyDiscoveryStatus,
};
use sophia_protocol::OutputId;

fn capability(
    output: sophia_engine::HeadlessOutput,
    connector: &str,
) -> LibdrmNativeOutputCapability {
    let mode =
        LibdrmNativeOutputTiming::new(output.size.width as u32, output.size.height as u32, 60_000);
    LibdrmNativeOutputCapability::new(
        output.id,
        output.id.raw() as u32,
        connector,
        [mode],
        Some(mode),
        mode,
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported,
    )
    .unwrap()
}

pub(super) fn moved_policy(
    output: sophia_engine::HeadlessOutput,
) -> output_realization::OutputPolicyLayout {
    let mut bounds = wm_output_bounds(&[output]);
    bounds[0].1.x = 8;
    output_realization::OutputPolicyLayout {
        bounds,
        primary: output.id,
        keys: [("DP-2".into(), 1)].into(),
        capabilities: vec![capability(output, "DP-2")],
        timings: BTreeMap::new(),
    }
}

#[test]
fn moved_connector_affinity_and_geometry_share_one_scene_and_invalidate_reused_output_identity() {
    let mut fixture = ReloadFixture::new();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let public = fixture.wm.public.as_mut().unwrap();
    public.output_capabilities = vec![capability(output, "DP-1")];
    public.output_policy_keys = [("DP-1".into(), 1)].into();
    let policy = moved_policy(output);
    let layout = PersistentLiveLayout::default();
    assert_eq!(
        fixture
            .wm
            .update_output_work_areas_for_realization(&layout, &[output], &policy)
            .unwrap(),
        LiveWmRequestAdmission::Admitted
    );
    let public = fixture.wm.public.as_ref().unwrap();
    let scene = public.reducer.scene();
    assert_eq!(scene.outputs[0].policy_key, Some(1));
    assert_eq!(scene.outputs[0].bounds.x, 8);
    assert_eq!(scene.outputs[0].work_area.x, 8);
    assert_eq!(scene.outputs[0].generation, 2);
    assert_eq!(
        public.output_capabilities[0].connector_key(),
        "DP-1",
        "hardware authority still awaits presentation"
    );
    assert_eq!(
        public.output_policy_capabilities.as_ref().unwrap()[0].connector_key(),
        "DP-2"
    );
    let before = scene.clone();
    assert_eq!(
        fixture
            .wm
            .update_output_work_areas_for_realization(&layout, &[output], &policy)
            .unwrap(),
        LiveWmRequestAdmission::Duplicate
    );
    assert_eq!(*fixture.wm.public.as_ref().unwrap().reducer.scene(), before);
}

#[test]
fn preferred_return_moves_the_key_once_without_stealing_live_focus() {
    let mut fixture = ReloadFixture::new();
    let fallback = sophia_engine::HeadlessOutput::deterministic();
    let preferred = sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(fallback.id.raw() + 1),
        ..fallback
    };
    let public = fixture.wm.public.as_mut().unwrap();
    public.output_capabilities = vec![capability(fallback, "DP-2")];
    public.output_policy_keys = [("DP-2".into(), 1)].into();
    let outputs = [fallback, preferred];
    let policy = output_realization::OutputPolicyLayout {
        bounds: wm_output_bounds(&outputs),
        primary: fallback.id,
        keys: [("DP-1".into(), 1)].into(),
        capabilities: vec![capability(fallback, "DP-2"), capability(preferred, "DP-1")],
        timings: BTreeMap::new(),
    };
    fixture
        .wm
        .update_output_work_areas_for_realization(
            &PersistentLiveLayout::default(),
            &outputs,
            &policy,
        )
        .unwrap();
    let scene = fixture.wm.public.as_ref().unwrap().reducer.scene();
    assert_eq!(scene.active_output, fallback.id);
    assert_eq!(
        scene
            .outputs
            .iter()
            .map(|output| (output.output, output.policy_key))
            .collect::<Vec<_>>(),
        [(fallback.id, None), (preferred.id, Some(1))]
    );
}

#[test]
fn duplicate_affinity_is_refused_before_mutating_the_wm_scene() {
    let mut fixture = ReloadFixture::new();
    let first = sophia_engine::HeadlessOutput::deterministic();
    let second = sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(first.id.raw() + 1),
        ..first
    };
    let outputs = [first, second];
    let policy = output_realization::OutputPolicyLayout {
        bounds: wm_output_bounds(&outputs),
        primary: first.id,
        keys: [("DP-1".into(), 1), ("DP-2".into(), 1)].into(),
        capabilities: vec![capability(first, "DP-1"), capability(second, "DP-2")],
        timings: BTreeMap::new(),
    };
    let before = fixture.wm.public.as_ref().unwrap().reducer.scene().clone();
    assert!(
        fixture
            .wm
            .update_output_work_areas_for_realization(
                &PersistentLiveLayout::default(),
                &outputs,
                &policy
            )
            .is_err()
    );
    let public = fixture.wm.public.as_ref().unwrap();
    assert_eq!(*public.reducer.scene(), before);
    assert!(public.output_policy_keys.is_empty());
    assert!(public.output_policy_capabilities.is_none());
}

#[test]
fn same_connector_recovery_preserves_live_focus_instead_of_reapplying_startup_focus() {
    let mut fixture = ReloadFixture::new();
    let first = sophia_engine::HeadlessOutput::deterministic();
    let second = sophia_engine::HeadlessOutput {
        id: OutputId::from_raw(first.id.raw() + 1),
        ..first
    };
    let outputs = [first, second];
    let mut policy = output_realization::OutputPolicyLayout {
        bounds: wm_output_bounds(&outputs),
        primary: second.id,
        keys: [("DP-1".into(), 1), ("DP-2".into(), 2)].into(),
        capabilities: vec![capability(first, "DP-1"), capability(second, "DP-2")],
        timings: BTreeMap::new(),
    };
    fixture
        .wm
        .update_output_work_areas_for_realization(
            &PersistentLiveLayout::default(),
            &outputs,
            &policy,
        )
        .unwrap();
    assert_eq!(fixture.wm.reference_output(), Some(second.id));
    policy.primary = first.id;
    fixture
        .wm
        .update_output_work_areas_for_realization(
            &PersistentLiveLayout::default(),
            &outputs,
            &policy,
        )
        .unwrap();
    assert_eq!(fixture.wm.reference_output(), Some(second.id));
}

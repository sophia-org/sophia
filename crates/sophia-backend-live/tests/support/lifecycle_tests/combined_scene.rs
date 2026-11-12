//! G3-3 through the real runtime: provisional composition of a candidate
//! over installed content, its eligibility, the scene key at consumption and
//! the setter's preflight. The native target is the shared deterministic
//! one; no device.
#![cfg(feature = "experimental-combined-frame-job")]
use super::super::compositor_graphics::ShellContentPreflight;
use super::super::compositor_graphics::combined_scene::{
    CombinedIneligible, CombinedRecompose, CombinedSceneKey, LiveProvisionalHeadFrame,
    combined_eligibility,
};
use super::*;
use sophia_engine::{CompositorImageSource, CompositorImageSourceIdentity};
use sophia_renderer_live::LiveOwnedMixedCompositionLayer;

const DOCK: LiveShellContentLayer = LiveShellContentLayer::Dock;
const SHELL: LiveShellContentLayer = LiveShellContentLayer::Shell;

fn grant_of(epoch: u64) -> ContentGrant {
    ContentGrant {
        connection_epoch: epoch,
        content_grant_epoch: epoch,
    }
}

fn store(grant: ContentGrant) -> sophia_runtime::ContentResourceStore {
    sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap()
}

fn resource(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

struct Fixture {
    output: HeadlessOutput,
    runtime: LiveProductionVisualRuntime,
    scene: LiveProductionCpuScene,
    target: Target,
    dock: sophia_runtime::ContentResourceStore,
    /// The candidate's real frame: what the job will install.
    candidate: LiveShellContentFrame,
    _shell: sophia_runtime::ContentResourceStore,
}

/// A Dock component installed and presented; the Shell candidate is not.
fn fixture() -> Fixture {
    let outputs = outputs();
    let output = outputs[0];
    let mut runtime = LiveProductionVisualRuntime::new(&outputs, None).unwrap();
    let scene = LiveProductionCpuScene::new(output.size);
    let mut target = Target::new(&outputs);
    let mut dock = store(grant_of(100));
    let installed = shell_frame(output, 1, upload(&mut dock, grant_of(100), resource(1)));
    runtime
        .set_shell_component_content_on_target(installed, DOCK, &scene, Some(&mut target))
        .unwrap();
    target.complete(output.id);
    let mut shell = store(grant_of(200));
    let candidate = shell_frame(output, 1, upload(&mut shell, grant_of(200), resource(1)));
    Fixture {
        output,
        runtime,
        scene,
        target,
        dock,
        candidate,
        _shell: shell,
    }
}

/// The candidate as the job sees it at dispatch: placed by its admitted
/// description, no pixels.
fn provisional(frame: &LiveShellContentFrame) -> LiveShellContentFrame {
    let mut frame = frame.clone();
    for image in &mut frame.images {
        let description = image.resource.shell().unwrap().description().clone();
        image.resource = CompositorImageSource::Provisional(description);
    }
    frame
}

fn ids(frame: &LiveShellContentFrame) -> Vec<CompositorImageSourceIdentity> {
    frame
        .images
        .iter()
        .map(|image| image.resource.identity())
        .collect()
}

fn compose(fixture: &Fixture, frame: &LiveShellContentFrame) -> Vec<LiveProvisionalHeadFrame> {
    fixture
        .runtime
        .compose_provisional_head_frames(&fixture.target, &fixture.scene, SHELL, frame)
        .unwrap()
}

fn key(frames: &[LiveProvisionalHeadFrame], candidate: &LiveShellContentFrame) -> CombinedSceneKey {
    CombinedSceneKey::of(&frames[0], &ids(candidate)).expect("an eligible frame has a key")
}

/// The dispatch key, and the classification of a recomposition with the
/// candidate's real frame against it.
fn dispatched(fixture: &Fixture) -> CombinedSceneKey {
    let frames = compose(fixture, &provisional(&fixture.candidate));
    assert_eq!(
        combined_eligibility(&frames, &provisional(&fixture.candidate)),
        Ok(())
    );
    key(&frames, &fixture.candidate)
}

fn recompose(fixture: &Fixture, dispatched: &CombinedSceneKey) -> CombinedRecompose {
    let frames = compose(fixture, &fixture.candidate);
    dispatched.recompose(CombinedSceneKey::of(&frames[0], &ids(&fixture.candidate)).as_ref())
}

/// The layer an installed (non-placeholder) image lowered to.
fn installed_layer(frame: &mut LiveProvisionalHeadFrame) -> &mut LiveOwnedMixedCompositionLayer {
    let composition = &mut frame.composition;
    let image = composition
        .images
        .iter()
        .find(|image| !image.provisional)
        .expect("the dock's image");
    let before = composition
        .images
        .iter()
        .filter(|other| other.provisional && other.index < image.index)
        .count();
    &mut composition.frame.layers[image.index - before]
}

type Observed = (
    BTreeMap<ShellContentKey, AdmittedShellContent>,
    BTreeMap<ShellContentKey, ContentGrant>,
    u64,
    sophia_renderer_live::CompositorTextCacheStats,
    sophia_renderer_live::IndicatorStripCacheStats,
    u64,
);

fn observed(fixture: &Fixture) -> Observed {
    let runtime = &fixture.runtime;
    (
        runtime.shell_content.clone(),
        runtime.retained_projection_retirements.clone(),
        runtime.content_layout_generation,
        runtime.text_cache.borrow().stats(),
        runtime.indicator_strip_cache.borrow().stats(),
        fixture.target.next,
    )
}

#[test]
fn a_candidate_over_a_layer_with_no_installed_frame_composes_as_placeholders() {
    let fixture = fixture();
    assert!(
        !fixture
            .runtime
            .shell_content
            .contains_key(&(fixture.output.id, SHELL))
    );
    let frames = compose(&fixture, &provisional(&fixture.candidate));
    assert_eq!(frames.len(), 1);
    let images = &frames[0].composition.images;
    assert_eq!(
        images
            .iter()
            .map(|image| image.provisional)
            .collect::<Vec<_>>(),
        [true, false],
        "the Shell layer's placeholder stacks below the installed Dock image"
    );
    assert!(images[0].index < images[1].index);
    assert_eq!(
        recompose(&fixture, &dispatched(&fixture)),
        CombinedRecompose::Match
    );
}

#[test]
fn provisional_composition_writes_no_installed_state() {
    let fixture = fixture();
    let before = observed(&fixture);
    let first = dispatched(&fixture);
    let second = dispatched(&fixture);
    let _ = recompose(&fixture, &first);
    assert_eq!(first, second, "the same metadata twice");
    assert!(
        observed(&fixture) == before,
        "nothing installed, cached or queued moved"
    );
}

#[test]
fn another_resource_with_the_same_generation_and_geometry_is_churn() {
    let mut fixture = fixture();
    let dispatched = dispatched(&fixture);
    // The resource identity alone, its handle and version unchanged.
    let mut frames = compose(&fixture, &fixture.candidate);
    let image = frames[0]
        .composition
        .images
        .iter_mut()
        .find(|image| !image.provisional)
        .unwrap();
    let CompositorImageSourceIdentity::Shell(description) = &mut image.resource else {
        panic!("the dock is shell content");
    };
    description.resource.id += 1;
    assert_eq!(
        dispatched.recompose(CombinedSceneKey::of(&frames[0], &ids(&fixture.candidate)).as_ref()),
        CombinedRecompose::SceneChurn,
        "another resource under the same handle and version is churn"
    );
    let replacement = shell_frame(
        fixture.output,
        1,
        upload(&mut fixture.dock, grant_of(100), resource(2)),
    );
    let key = (fixture.output.id, DOCK);
    fixture.runtime.shell_content.get_mut(&key).unwrap().frame = replacement;
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::SceneChurn
    );
}

#[test]
fn another_grant_replaced_or_revoked_is_churn() {
    let mut fixture = fixture();
    let dispatched = dispatched(&fixture);
    let key = (fixture.output.id, DOCK);
    fixture
        .runtime
        .shell_content
        .get_mut(&key)
        .unwrap()
        .interaction_revoked = true;
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::SceneChurn
    );

    let mut fixture = self::fixture();
    let dispatched = self::dispatched(&fixture);
    let mut other = store(grant_of(300));
    let replaced = shell_frame(
        fixture.output,
        1,
        upload(&mut other, grant_of(300), resource(1)),
    );
    fixture.runtime.shell_content.get_mut(&key).unwrap().frame = replaced;
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::SceneChurn
    );
}

#[test]
fn a_pixel_version_change_with_the_same_generation_is_churn() {
    let fixture = fixture();
    let dispatched = dispatched(&fixture);
    let mut frames = compose(&fixture, &fixture.candidate);
    let LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } = installed_layer(&mut frames[0]) else {
        panic!("the dock lowers to a CPU layer");
    };
    let version = buffer.pixel_version.expect("a lease carries its version");
    buffer.pixel_version = Some(version.checked_add(1).expect("the next version fits"));
    assert_eq!(
        dispatched.recompose(CombinedSceneKey::of(&frames[0], &ids(&fixture.candidate)).as_ref()),
        CombinedRecompose::SceneChurn
    );
}

#[test]
fn a_transform_or_layout_generation_change_is_churn() {
    let mut fixture = fixture();
    let dispatched = dispatched(&fixture);
    fixture.runtime.content_layout_generation += 1;
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::SceneChurn
    );

    let mut fixture = self::fixture();
    let dispatched = self::dispatched(&fixture);
    let key = (fixture.output.id, DOCK);
    fixture
        .runtime
        .shell_content
        .get_mut(&key)
        .unwrap()
        .transform
        .viewport
        .x += 1;
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::SceneChurn
    );
}

#[test]
fn a_cursor_move_is_churn() {
    let fixture = fixture();
    let cursor = |x| sophia_engine::OutputSceneCursor {
        geometry: Rect {
            x,
            y: 0,
            width: 8,
            height: 8,
        },
        source: BufferSource::CpuBuffer { handle: 5 },
        generation: 1,
    };
    let mut dispatch = compose(&fixture, &provisional(&fixture.candidate));
    dispatch[0].cursor = Some(cursor(0));
    let dispatched = key(&dispatch, &fixture.candidate);
    let mut frames = compose(&fixture, &fixture.candidate);
    frames[0].cursor = Some(cursor(1));
    assert_eq!(
        dispatched.recompose(CombinedSceneKey::of(&frames[0], &ids(&fixture.candidate)).as_ref()),
        CombinedRecompose::SceneChurn
    );
}

#[test]
fn a_target_generation_or_native_owner_change_is_a_topology_change() {
    let mut fixture = fixture();
    let dispatched = dispatched(&fixture);
    let mut frames = compose(&fixture, &fixture.candidate);
    frames[0].target_generation += 1;
    assert_eq!(
        dispatched.recompose(CombinedSceneKey::of(&frames[0], &ids(&fixture.candidate)).as_ref()),
        CombinedRecompose::TopologyChange
    );
    // A rebuilt native owner: the same heads under a new owner identity.
    fixture.target = Target::new(&outputs());
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::TopologyChange,
        "a new native owner is a topology change"
    );
}

#[test]
fn the_candidates_own_grant_loss_is_a_candidate_failure() {
    let mut fixture = fixture();
    let earlier = provisional(&fixture.candidate);
    let dispatched = dispatched(&fixture);
    // The candidate's grant already holds the layer and is now closing.
    fixture.runtime.shell_content.insert(
        (fixture.output.id, SHELL),
        AdmittedShellContent {
            frame: earlier,
            retained_content_epoch: None,
            interaction_revoked: true,
            transform: sophia_engine::PresentedContentTransform {
                viewport: Rect::default(),
                layout_generation: 1,
            },
        },
    );
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::CandidateFailure,
        "the candidate's own revoked grant is its failure"
    );
}

#[test]
fn a_moved_candidate_with_the_same_node_and_description_is_a_candidate_failure() {
    let mut fixture = fixture();
    let dispatched = dispatched(&fixture);
    fixture.candidate.images[0].geometry_px.x += 4;
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::CandidateFailure,
        "the candidate's own placement moved"
    );
}

#[test]
fn a_repeated_resource_keys_every_occurrence_and_a_dropped_one_is_a_candidate_failure() {
    let mut fixture = fixture();
    let single = fixture.candidate.clone();
    let mut second = single.images[0].clone();
    let CompositorNodeId::ShellContent { placement, .. } = &mut second.node else {
        unreachable!();
    };
    *placement = 1;
    second.geometry_px.x = 8;
    fixture.candidate.images.push(second);
    let dispatched = dispatched(&fixture);
    let frames = compose(&fixture, &provisional(&fixture.candidate));
    assert_eq!(
        frames[0]
            .composition
            .images
            .iter()
            .filter(|image| image.provisional)
            .count(),
        2,
        "one placeholder per occurrence"
    );
    assert_eq!(recompose(&fixture, &dispatched), CombinedRecompose::Match);
    fixture.candidate = single;
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::CandidateFailure,
        "a dropped occurrence is the candidate's failure"
    );
}

#[test]
fn each_disallowed_layer_class_and_image_reads_are_ineligible() {
    let fixture = fixture();
    let candidate = provisional(&fixture.candidate);
    let mut frames = compose(&fixture, &candidate);
    frames[0]
        .composition
        .frame
        .layers
        .push(LiveOwnedMixedCompositionLayer::RendererImage {
            image_id: sophia_renderer_live::LiveRendererImageId::from_raw(9),
            size: Size {
                width: 4,
                height: 2,
            },
            format: u32::from_le_bytes(*b"AR24"),
            placement: sophia_renderer_live::LiveCompositionPlacement {
                target: Rect::default(),
                clip: None,
                transform: sophia_protocol::Transform::IDENTITY,
                alpha: 1.0,
                sampling: sophia_engine::HeadSamplingClass::Exact,
            },
        });
    assert_eq!(
        combined_eligibility(&frames, &candidate),
        Err(CombinedIneligible::LayerKind)
    );
    let mut two = compose(&fixture, &candidate);
    two.extend(compose(&fixture, &candidate));
    assert_eq!(
        combined_eligibility(&two, &candidate),
        Err(CombinedIneligible::HeadCount)
    );
    let mut cursor = compose(&fixture, &candidate);
    cursor[0].cursor = Some(sophia_engine::OutputSceneCursor {
        geometry: Rect::default(),
        source: BufferSource::CpuBuffer { handle: 5 },
        generation: 1,
    });
    assert_eq!(
        combined_eligibility(&cursor, &candidate),
        Err(CombinedIneligible::SoftwareCursor)
    );
    let real = compose(&fixture, &fixture.candidate);
    assert_eq!(
        combined_eligibility(&real, &fixture.candidate),
        Err(CombinedIneligible::CandidateNotProvisional)
    );
}

#[test]
fn an_unversioned_cpu_layer_is_ineligible() {
    let fixture = fixture();
    let candidate = provisional(&fixture.candidate);
    let mut frames = compose(&fixture, &candidate);
    let LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } = installed_layer(&mut frames[0]) else {
        panic!("the dock lowers to a CPU layer");
    };
    buffer.pixel_version = None;
    assert_eq!(
        combined_eligibility(&frames, &candidate),
        Err(CombinedIneligible::UnversionedPixels),
        "no version, no exact key"
    );
}

#[test]
fn the_install_preflight_refuses_without_changing_state() {
    let mut fixture = fixture();
    let before = observed(&fixture);
    let placeholder = provisional(&fixture.candidate);
    let refused = fixture
        .runtime
        .preflight_shell_component_content(&placeholder, SHELL, Some(&fixture.target))
        .err()
        .map(|error| error.to_string());
    assert_eq!(refused.as_deref(), Some("shell content frame is malformed"));
    assert!(matches!(
        fixture.runtime.preflight_shell_component_content(
            &fixture.candidate,
            SHELL,
            Some(&fixture.target)
        ),
        Ok(ShellContentPreflight::Admit(_))
    ));
    assert!(observed(&fixture) == before, "the preflight writes nothing");
    // The ordinary setter refuses a placeholder the same way.
    let refused = fixture
        .runtime
        .set_shell_component_content_on_target(
            placeholder,
            SHELL,
            &fixture.scene,
            Some(&mut fixture.target),
        )
        .err()
        .map(|error| error.to_string());
    assert_eq!(refused.as_deref(), Some("shell content frame is malformed"));
    assert!(observed(&fixture) == before);
}

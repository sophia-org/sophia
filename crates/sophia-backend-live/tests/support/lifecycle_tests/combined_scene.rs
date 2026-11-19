//! G3-3 through the real runtime: provisional composition of a candidate
//! over installed content, its eligibility, the scene key at consumption and
//! the setter's preflight. The native target is the shared deterministic
//! one; no device.
#![cfg(feature = "experimental-combined-frame-job")]
use super::super::compositor_graphics::ShellContentPreflight;
use super::super::compositor_graphics::combined_scene::{
    CombinedCursor, CombinedIneligible, CombinedOwnerState, CombinedRecompose, CombinedSceneKey,
    LiveProvisionalHeadFrame, ProvisionalComposeError, combined_eligibility,
};
use super::*;
use sophia_engine::{CompositorImageSource, CompositorImageSourceIdentity};
use sophia_protocol::Point;
use sophia_renderer_live::{LiveOwnedMixedCompositionLayer, LiveProvisionalDecline};

// G3-4: the visual runtime's half of the combined accept, on this fixture.
#[path = "combined_accept.rs"]
mod accept;

const DOCK: LiveShellContentLayer = LiveShellContentLayer::Dock;
const SHELL: LiveShellContentLayer = LiveShellContentLayer::Shell;
/// The owner's plane cursor, shown, as every ordinary control composes.
const PLANE: CombinedCursor = CombinedCursor::Plane(Some(Point { x: 10.0, y: 4.0 }));
const IDLE: CombinedOwnerState = CombinedOwnerState {
    capture_active: false,
    worker_idle: true,
};

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

fn compose_with(
    fixture: &Fixture,
    frame: &LiveShellContentFrame,
    cursor: CombinedCursor,
) -> Result<Vec<LiveProvisionalHeadFrame>, ProvisionalComposeError> {
    fixture.runtime.compose_provisional_head_frames(
        &fixture.target,
        &fixture.scene,
        SHELL,
        frame,
        cursor,
    )
}

fn compose(fixture: &Fixture, frame: &LiveShellContentFrame) -> Vec<LiveProvisionalHeadFrame> {
    compose_with(fixture, frame, PLANE).unwrap()
}

fn key(frames: &[LiveProvisionalHeadFrame], candidate: &LiveShellContentFrame) -> CombinedSceneKey {
    CombinedSceneKey::of(&frames[0], &ids(candidate))
}

/// The dispatch key of an eligible provisional composition.
fn dispatched(fixture: &Fixture) -> CombinedSceneKey {
    let frames = compose(fixture, &provisional(&fixture.candidate));
    assert_eq!(
        combined_eligibility(&frames, &provisional(&fixture.candidate), IDLE),
        Ok(())
    );
    key(&frames, &fixture.candidate)
}

/// The classification of a recomposition with the candidate's real frame.
fn recompose_with(
    fixture: &Fixture,
    dispatched: &CombinedSceneKey,
    cursor: CombinedCursor,
) -> CombinedRecompose {
    dispatched
        .classify(
            compose_with(fixture, &fixture.candidate, cursor),
            &ids(&fixture.candidate),
        )
        .unwrap()
}

fn recompose(fixture: &Fixture, dispatched: &CombinedSceneKey) -> CombinedRecompose {
    recompose_with(fixture, dispatched, PLANE)
}

/// The classification of frames a control edited after composing them.
fn classified(
    fixture: &Fixture,
    dispatched: &CombinedSceneKey,
    frames: Vec<LiveProvisionalHeadFrame>,
) -> CombinedRecompose {
    dispatched
        .classify(Ok(frames), &ids(&fixture.candidate))
        .unwrap()
}

/// The Dock's occurrence, chosen by its exact grant: at consumption the
/// candidate's images are real too, and the Shell candidate comes first.
fn dock(frame: &LiveProvisionalHeadFrame) -> usize {
    let docks = frame
        .composition
        .images
        .iter()
        .enumerate()
        .filter(|(_, image)| {
            matches!(image.node, CompositorNodeId::ShellContent { grant, .. } if grant == grant_of(100))
        })
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    let [position] = docks.as_slice() else {
        panic!("exactly one Dock occurrence");
    };
    let image = &frame.composition.images[*position];
    assert!(
        !image.provisional,
        "the Dock is installed, never a placeholder"
    );
    assert!(matches!(
        &image.resource,
        CompositorImageSourceIdentity::Shell(description) if description.grant == grant_of(100)
    ));
    *position
}

/// The layer the Dock's image lowered to. A placeholder before it holds a
/// final index but no lowered layer, so dispatch and consumption differ.
fn installed_layer(frame: &mut LiveProvisionalHeadFrame) -> &mut LiveOwnedMixedCompositionLayer {
    let image = &frame.composition.images[dock(frame)];
    let (index, handle) = (image.index, image.handle);
    let before = frame
        .composition
        .images
        .iter()
        .filter(|other| other.provisional && other.index < index)
        .count();
    let layer = &mut frame.composition.frame.layers[index - before];
    assert!(
        matches!(layer, LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } if buffer.handle == handle),
        "the Dock's own layer"
    );
    layer
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
    let position = dock(&frames[0]);
    let CompositorImageSourceIdentity::Shell(description) =
        &mut frames[0].composition.images[position].resource
    else {
        panic!("the dock is shell content");
    };
    description.resource.id += 1;
    assert_eq!(
        classified(&fixture, &dispatched, frames),
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
        classified(&fixture, &dispatched, frames),
        CombinedRecompose::SceneChurn,
        "the Dock's new version under its old generation is churn"
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
    let plane = |x| {
        CombinedCursor::new(
            crate::LiveProductionCursorPresentation::HardwarePlane,
            Some(Point { x, y: 4.0 }),
        )
    };
    assert_eq!(plane(10.0), PLANE, "the owner's plane presentation");
    let dispatched = dispatched(&fixture);
    assert_eq!(
        recompose_with(&fixture, &dispatched, plane(10.0)),
        CombinedRecompose::Match,
        "an unchanged plane cursor matches"
    );
    assert_eq!(
        recompose_with(&fixture, &dispatched, plane(11.0)),
        CombinedRecompose::SceneChurn,
        "a moved plane cursor is churn"
    );
    assert_eq!(
        recompose_with(&fixture, &dispatched, CombinedCursor::Plane(None)),
        CombinedRecompose::SceneChurn,
        "a hidden plane cursor is churn"
    );
}

#[test]
fn a_software_cursor_is_ineligible() {
    let fixture = fixture();
    let candidate = provisional(&fixture.candidate);
    let software = CombinedCursor::new(
        crate::LiveProductionCursorPresentation::Software(Some(Point { x: 10.0, y: 4.0 })),
        None,
    );
    let frames = compose_with(&fixture, &candidate, software).unwrap();
    assert_eq!(
        combined_eligibility(&frames, &candidate, IDLE),
        Err(CombinedIneligible::SoftwareCursor),
        "the software compositor draws it into the frame"
    );
    let mut frames = compose(&fixture, &candidate);
    frames[0].plan_cursor = Some(sophia_engine::OutputSceneCursor {
        geometry: Rect::default(),
        source: BufferSource::CpuBuffer { handle: 5 },
        generation: 1,
    });
    assert_eq!(
        combined_eligibility(&frames, &candidate, IDLE),
        Err(CombinedIneligible::SoftwareCursor),
        "a cursor the head plan composites"
    );
}

#[test]
fn a_target_generation_or_native_owner_change_is_a_topology_change() {
    let mut fixture = fixture();
    let dispatched = dispatched(&fixture);
    let mut frames = compose(&fixture, &fixture.candidate);
    frames[0].target_generation += 1;
    assert_eq!(
        classified(&fixture, &dispatched, frames),
        CombinedRecompose::TopologyChange
    );
    // A rebuilt native owner: the same heads under a new owner identity.
    fixture.target = Target::new(&outputs());
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::TopologyChange,
        "a new native owner is a topology change"
    );
    // The output lost its head.
    fixture.target = Target::new(&outputs()[1..]);
    assert!(matches!(
        compose_with(&fixture, &fixture.candidate, PLANE),
        Err(ProvisionalComposeError::Topology)
    ));
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::TopologyChange,
        "a missing head is a topology change"
    );
}

#[test]
fn a_changed_noncandidate_layer_kind_with_the_same_heads_is_churn() {
    let fixture = fixture();
    let dispatched = dispatched(&fixture);
    let mut frames = compose(&fixture, &fixture.candidate);
    let layer = installed_layer(&mut frames[0]);
    let LiveOwnedMixedCompositionLayer::Cpu { buffer, placement } = &*layer else {
        unreachable!("installed_layer checked the kind");
    };
    let (size, format, placement) = (buffer.size, buffer.format, *placement);
    *layer = LiveOwnedMixedCompositionLayer::RendererImage {
        image_id: sophia_renderer_live::LiveRendererImageId::from_raw(9),
        size,
        format,
        placement,
    };
    assert_eq!(
        classified(&fixture, &dispatched, frames),
        CombinedRecompose::SceneChurn,
        "another surface's new layer kind under the same heads is churn"
    );
    assert!(
        dispatched
            .classify(
                Err(ProvisionalComposeError::Composition(
                    "a display list fault".into()
                )),
                &ids(&fixture.candidate)
            )
            .is_err(),
        "an unclassified failure is returned, not mapped"
    );
}

/// The consumption composition as if it had also met `decline`: every
/// other fact is the real recomposition's.
fn declined(
    fixture: &Fixture,
    dispatched: &CombinedSceneKey,
    decline: LiveProvisionalDecline,
) -> CombinedRecompose {
    let mut frames = compose(fixture, &fixture.candidate);
    frames[0].composition.declined = Some(decline);
    classified(fixture, dispatched, frames)
}

/// The candidate's grant already holds the layer and is now closing.
fn revoke_candidate(fixture: &mut Fixture) {
    let earlier = provisional(&fixture.candidate);
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
}

#[test]
fn a_scene_only_decline_does_not_hide_candidate_or_topology_changes() {
    for decline in [
        LiveProvisionalDecline::ColdTextRaster,
        LiveProvisionalDecline::ColdIndicatorRaster,
        LiveProvisionalDecline::TranslucentRect,
    ] {
        let mut fixture = fixture();
        let dispatched = dispatched(&fixture);
        assert_eq!(
            declined(&fixture, &dispatched, decline),
            CombinedRecompose::SceneChurn,
            "a decline alone is scene churn: {decline:?}"
        );
        let mut frames = compose(&fixture, &fixture.candidate);
        frames[0].composition.declined = Some(decline);
        let unbound = key(&frames, &fixture.candidate);
        assert_eq!(
            unbound.recompose(&unbound.clone()),
            CombinedRecompose::SceneChurn,
            "a declined frame never matches, even itself: {decline:?}"
        );
        fixture.target = Target::new(&outputs());
        assert_eq!(
            declined(&fixture, &dispatched, decline),
            CombinedRecompose::TopologyChange,
            "a decline does not hide a rebuilt native owner: {decline:?}"
        );
        let mut frames = compose(&fixture, &fixture.candidate);
        frames[0].target_generation += 1;
        frames[0].composition.declined = Some(decline);
        assert_eq!(
            classified(&fixture, &dispatched, frames),
            CombinedRecompose::TopologyChange,
            "a decline does not hide a target generation change: {decline:?}"
        );

        let mut fixture = self::fixture();
        let dispatched = self::dispatched(&fixture);
        revoke_candidate(&mut fixture);
        assert_eq!(
            declined(&fixture, &dispatched, decline),
            CombinedRecompose::CandidateFailure,
            "a decline does not hide the candidate's revocation: {decline:?}"
        );
    }
}

#[test]
fn the_candidates_own_grant_loss_is_a_candidate_failure() {
    let mut fixture = fixture();
    let dispatched = dispatched(&fixture);
    revoke_candidate(&mut fixture);
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
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::Match,
        "every occurrence matches its placeholder"
    );
    fixture.candidate = single;
    assert_eq!(
        recompose(&fixture, &dispatched),
        CombinedRecompose::CandidateFailure,
        "a dropped occurrence is the candidate's failure"
    );
}

/// A device-free DMA-BUF: one plane over /dev/null, never imported.
fn dma_frame() -> sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame {
    sophia_renderer_live::LiveOwnedMultiPlaneDmaBufFrame {
        width: 4,
        height: 2,
        format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_ARGB8888,
        modifier: 0,
        plane_count: 1,
        planes: [
            Some(sophia_renderer_live::LiveOwnedDmaBufPlane {
                fd: std::fs::File::open("/dev/null").unwrap().into(),
                offset: 0,
                stride: 16,
            }),
            None,
            None,
            None,
        ],
    }
}

/// RendererImage and DmaBuf are the classes a device-free frame can hold.
/// Snapshot needs a native EGL snapshot and ManagedCapture the capture
/// feature; both reach the same refusing arm.
#[test]
fn each_disallowed_layer_class_and_image_reads_are_ineligible() {
    let fixture = fixture();
    let candidate = provisional(&fixture.candidate);
    let image_id = sophia_renderer_live::LiveRendererImageId::from_raw(9);
    let placement = sophia_renderer_live::LiveCompositionPlacement {
        target: Rect::default(),
        clip: None,
        transform: sophia_protocol::Transform::IDENTITY,
        alpha: 1.0,
        sampling: sophia_engine::HeadSamplingClass::Exact,
    };
    let size = Size {
        width: 4,
        height: 2,
    };
    for layer in [
        LiveOwnedMixedCompositionLayer::RendererImage {
            image_id,
            size,
            format: u32::from_le_bytes(*b"AR24"),
            placement,
        },
        LiveOwnedMixedCompositionLayer::DmaBuf {
            image_id,
            frame: dma_frame(),
            placement,
        },
    ] {
        let mut frames = compose(&fixture, &candidate);
        frames[0].composition.frame.layers.push(layer);
        assert_eq!(
            combined_eligibility(&frames, &candidate, IDLE),
            Err(CombinedIneligible::LayerKind)
        );
    }
    // A real device-free read guard, with the frame otherwise eligible.
    let reads = sophia_renderer_live::LiveRendererImageReads::default();
    let mut frames = compose(&fixture, &candidate);
    assert_eq!(combined_eligibility(&frames, &candidate, IDLE), Ok(()));
    frames[0].composition.frame.image_reads = vec![reads.acquire(image_id)].into_boxed_slice();
    assert_eq!(
        combined_eligibility(&frames, &candidate, IDLE),
        Err(CombinedIneligible::ImageReads),
        "a held renderer-image read refuses"
    );
    let mut two = compose(&fixture, &candidate);
    two.extend(compose(&fixture, &candidate));
    assert_eq!(
        combined_eligibility(&two, &candidate, IDLE),
        Err(CombinedIneligible::HeadCount)
    );
    let mut declined = compose(&fixture, &candidate);
    declined[0].composition.declined = Some(LiveProvisionalDecline::ColdTextRaster);
    assert_eq!(
        combined_eligibility(&declined, &candidate, IDLE),
        Err(CombinedIneligible::Declined(
            LiveProvisionalDecline::ColdTextRaster
        )),
        "a declined composition is ineligible"
    );
    let frames = compose(&fixture, &candidate);
    assert_eq!(
        combined_eligibility(
            &frames,
            &candidate,
            CombinedOwnerState {
                capture_active: true,
                ..IDLE
            }
        ),
        Err(CombinedIneligible::CaptureActive),
        "a capture in flight refuses"
    );
    assert_eq!(
        combined_eligibility(
            &frames,
            &candidate,
            CombinedOwnerState {
                worker_idle: false,
                ..IDLE
            }
        ),
        Err(CombinedIneligible::WorkerBusy),
        "a busy renderer worker refuses"
    );
    let real = compose(&fixture, &fixture.candidate);
    assert_eq!(
        combined_eligibility(&real, &fixture.candidate, IDLE),
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
        combined_eligibility(&frames, &candidate, IDLE),
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
        .preflight_shell_component_content(
            &placeholder,
            SHELL,
            Some(fixture.target.frame_service_available()),
        )
        .err()
        .map(|error| error.to_string());
    assert_eq!(refused.as_deref(), Some("shell content frame is malformed"));
    assert!(matches!(
        fixture.runtime.preflight_shell_component_content(
            &fixture.candidate,
            SHELL,
            Some(fixture.target.frame_service_available())
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

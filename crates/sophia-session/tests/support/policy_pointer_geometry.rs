use super::*;
use sophia_protocol::{
    LayoutNodeCapabilities, PolicyInteractionPhase as Phase, PolicyPresentationState,
    PolicyRequestCause, PolicySurfaceKind, PolicySurfaceSnapshot, SurfaceConstraints,
    WmPointerGestureMode as Mode, WmPointerPosition,
};

const SURFACE: SurfaceId = SurfaceId::new(91, 1);
const CONTENT: Rect = Rect {
    x: 103,
    y: 83,
    width: 294,
    height: 194,
};
const OUTER: Rect = Rect {
    x: 100,
    y: 80,
    width: 300,
    height: 200,
};

fn fixture() -> ReloadFixture {
    let mut fixture = ReloadFixture::new();
    let output = sophia_engine::HeadlessOutput::deterministic();
    let mut scene = LivePublicPolicyState::initial_scene(&[output], output.id, vec![]);
    scene.surfaces.push(PolicySurfaceSnapshot {
        surface: SURFACE,
        generation: 1,
        current_output: Some(output.id),
        kind: PolicySurfaceKind::Toplevel,
        capabilities: LayoutNodeCapabilities::STANDARD_TOPLEVEL,
        constraints: SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        exact_size: None,
        requested_state: PolicyPresentationState::default(),
        current_state: PolicyPresentationState::default(),
        transient_owner: None,
        geometry: OUTER,
    });
    let public = fixture.wm.public.as_mut().unwrap();
    public.reducer = sophia_engine::PolicyProjectionReducer::new(scene).unwrap();
    public.reducer.connect(1).unwrap();
    public.queue.clear();
    fixture.wm.visual_chrome.focus_ring.width = 3;
    fixture.wm.visual_chrome.frame.width = 1;
    let mut pending = fixture.wm.visual_chrome;
    pending.focus_ring.width = 7;
    fixture.wm.pending_visual_chrome = Some(pending);
    fixture
}

fn interaction(mode: Mode, phase: Phase, geometry: Rect) -> FloatingPointerPolicyInteraction {
    FloatingPointerPolicyInteraction {
        surface: SURFACE,
        mode,
        phase,
        start: WmPointerPosition { x: 120, y: 100 },
        current: WmPointerPosition { x: 120, y: 100 },
        geometry,
    }
}

#[test]
fn pointer_admission_uses_committed_chrome_for_every_interaction_phase() {
    for mode in [Mode::Move, Mode::Resize] {
        for phase in [Phase::Begin, Phase::Update, Phase::End, Phase::Cancel] {
            let mut fixture = fixture();
            assert_eq!(
                fixture
                    .wm
                    .enqueue_pointer_interaction(
                        interaction(mode, phase, CONTENT),
                        &PersistentLiveLayout::default(),
                    )
                    .unwrap(),
                LiveWmRequestAdmission::Admitted,
            );
            let public = fixture.wm.public.as_ref().unwrap();
            assert_eq!(public.queue.len(), 1);
            assert!(
                matches!(public.queue.front().unwrap().cause,
                    PolicyRequestCause::Interaction { geometry: OUTER, phase: p, target: SURFACE, .. }
                    if p == phase
                ),
                "stationary content must retain its outer allocation, ignoring pending chrome"
            );
            assert_eq!(
                public.reducer.committed()[0].placements[0].geometry,
                OUTER,
                "admission does not commit policy"
            );
        }
    }
}

#[test]
fn pointer_geometry_overflow_drops_only_the_invalid_interaction() {
    let mut fixture = fixture();
    for geometry in [
        Rect {
            x: i32::MIN,
            ..CONTENT
        },
        Rect {
            width: i32::MAX,
            ..CONTENT
        },
    ] {
        assert_eq!(
            fixture
                .wm
                .enqueue_pointer_interaction(
                    interaction(Mode::Move, Phase::Begin, geometry),
                    &PersistentLiveLayout::default(),
                )
                .unwrap(),
            LiveWmRequestAdmission::Duplicate
        );
        assert!(fixture.wm.public.as_ref().unwrap().queue.is_empty());
    }
    assert_eq!(
        fixture
            .wm
            .enqueue_pointer_interaction(
                interaction(Mode::Move, Phase::Begin, CONTENT),
                &PersistentLiveLayout::default(),
            )
            .unwrap(),
        LiveWmRequestAdmission::Admitted
    );
}

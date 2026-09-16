/// Stable names for the revision-1 black-box behavior corpus. Every public
/// policy client must accept the same complete snapshots and return a proposal
/// admitted by the canonical reducer for each entry before revision 1 freezes.
pub const SOPHIA_WM_V1_BEHAVIOR_SCENARIOS: [&str; 11] = [
    "single-output-constraints",
    "two-output-partition",
    "output-loss",
    "returned-output-generation",
    "ordered-action",
    "timeout-discard",
    "post-timeout-recovery",
    "stale-discard",
    "post-stale-recovery",
    "invalid-discard",
    "post-invalid-recovery",
];

/// Returns the reduced cause paired with one behavior scene.
pub fn sophia_wm_v1_behavior_cause(scenario: &str) -> Option<crate::PolicyRequestCause> {
    match scenario {
        "ordered-action" => Some(crate::PolicyRequestCause::Action {
            activation_serial: 41,
            action: crate::WmActionId::from_raw(1),
        }),
        scenario if SOPHIA_WM_V1_BEHAVIOR_SCENARIOS.contains(&scenario) => {
            Some(crate::PolicyRequestCause::SceneChanged)
        }
        _ => None,
    }
}

/// Returns the canonical complete scene for one revision-1 behavior scenario.
/// The sequence is intentionally stateful when consumed in declaration order.
pub fn sophia_wm_v1_behavior_scene(scenario: &str) -> Option<crate::PolicySceneSnapshot> {
    let first = crate::OutputId::from_raw(1);
    let second = crate::OutputId::from_raw(2);
    let (generation, active_output, outputs, surfaces) = match scenario {
        "single-output-constraints" => (
            1,
            first,
            vec![output(first, 1, 0, Some(crate::SurfaceId::new(3, 1)))],
            vec![surface(3, 1, first), surface(4, 1, first)],
        ),
        "two-output-partition" => (
            2,
            second,
            vec![
                output(first, 1, 0, Some(crate::SurfaceId::new(3, 1))),
                output(second, 1, 1200, Some(crate::SurfaceId::new(5, 1))),
            ],
            vec![
                surface(3, 1, first),
                surface(4, 1, first),
                surface(5, 1, second),
            ],
        ),
        "output-loss" => (
            3,
            first,
            vec![output(first, 1, 0, Some(crate::SurfaceId::new(3, 3)))],
            vec![
                surface(3, 3, first),
                surface(4, 3, first),
                surface(5, 3, first),
            ],
        ),
        "returned-output-generation" => (
            4,
            second,
            vec![
                output(first, 1, 0, Some(crate::SurfaceId::new(3, 4))),
                output(second, 2, 1200, Some(crate::SurfaceId::new(5, 4))),
            ],
            vec![
                surface(3, 4, first),
                surface(4, 4, first),
                surface(5, 4, second),
            ],
        ),
        "ordered-action" => returned_scene(5, second, 5),
        "timeout-discard" => returned_scene(6, second, 6),
        "post-timeout-recovery" => returned_scene(7, second, 7),
        "stale-discard" => returned_scene(8, second, 8),
        "post-stale-recovery" => returned_scene(9, second, 9),
        "invalid-discard" => returned_scene(10, second, 10),
        "post-invalid-recovery" => returned_scene(11, second, 11),
        _ => return None,
    };
    Some(crate::PolicySceneSnapshot {
        generation,
        active_output,
        outputs,
        surfaces,
        session_operations: Vec::new(),
    })
}

fn returned_scene(
    generation: u64,
    active_output: crate::OutputId,
    surface_generation: u32,
) -> (
    u64,
    crate::OutputId,
    Vec<crate::PolicyOutputSnapshot>,
    Vec<crate::PolicySurfaceSnapshot>,
) {
    let first = crate::OutputId::from_raw(1);
    let second = crate::OutputId::from_raw(2);
    (
        generation,
        active_output,
        vec![
            output(
                first,
                1,
                0,
                Some(crate::SurfaceId::new(3, surface_generation)),
            ),
            output(
                second,
                2,
                1200,
                Some(crate::SurfaceId::new(5, surface_generation)),
            ),
        ],
        vec![
            surface(3, surface_generation, first),
            surface(4, surface_generation, first),
            surface(5, surface_generation, second),
        ],
    )
}

fn output(
    output: crate::OutputId,
    generation: u64,
    x: i32,
    focus: Option<crate::SurfaceId>,
) -> crate::PolicyOutputSnapshot {
    crate::PolicyOutputSnapshot {
        policy_key: None,
        output,
        generation,
        focus,
        bounds: crate::Rect {
            x,
            y: 0,
            width: 1200,
            height: 800,
        },
        work_area: crate::Rect {
            x,
            y: 0,
            width: 1200,
            height: 800,
        },
    }
}

fn surface(index: u32, generation: u32, output: crate::OutputId) -> crate::PolicySurfaceSnapshot {
    crate::PolicySurfaceSnapshot {
        surface: crate::SurfaceId::new(index, generation),
        generation: u64::from(generation),
        current_output: Some(output),
        kind: crate::PolicySurfaceKind::Toplevel,
        capabilities: crate::LayoutNodeCapabilities::STANDARD_TOPLEVEL,
        constraints: crate::SurfaceConstraints {
            min_size: Some(crate::Size {
                width: 100,
                height: 80,
            }),
            max_size: None,
        },
        exact_size: None,
        requested_state: crate::PolicyPresentationState::default(),
        current_state: crate::PolicyPresentationState::default(),
        transient_owner: None,
        geometry: crate::Rect {
            x: if output == crate::OutputId::from_raw(2) {
                1200
            } else {
                0
            },
            y: 0,
            width: 600,
            height: 800,
        },
    }
}

use sophia_protocol::{Rect, SurfaceId};

#[derive(Clone, Copy, Debug)]
struct ActiveFloatingPointerGesture {
    surface: SurfaceId,
    mode: sophia_protocol::WmPointerGestureMode,
    button: u32,
    start: sophia_protocol::WmPointerPosition,
    current: sophia_protocol::WmPointerPosition,
    initial_geometry: Rect,
}

#[derive(Default)]
pub(super) struct FloatingPointerGestureState {
    active: Option<ActiveFloatingPointerGesture>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct FloatingPointerGestureObservation {
    pub consumed: bool,
    pub completed: Option<sophia_protocol::WmPointerGestureCompleted>,
    pub interaction: Option<FloatingPointerPolicyInteraction>,
    pub outline: FloatingPointerOutlineUpdate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FloatingPointerPolicyInteraction {
    pub surface: SurfaceId,
    pub mode: sophia_protocol::WmPointerGestureMode,
    pub phase: sophia_protocol::PolicyInteractionPhase,
    pub start: sophia_protocol::WmPointerPosition,
    pub current: sophia_protocol::WmPointerPosition,
    pub geometry: Rect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FloatingPointerOutline {
    pub surface: SurfaceId,
    pub start: sophia_protocol::WmPointerPosition,
    pub geometry: Rect,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum FloatingPointerOutlineUpdate {
    #[default]
    Unchanged,
    Set(FloatingPointerOutline),
    Clear,
}

fn floating_pointer_outline(
    active: ActiveFloatingPointerGesture,
    position: sophia_protocol::WmPointerPosition,
) -> FloatingPointerOutline {
    let delta_x = position.x.saturating_sub(active.start.x);
    let delta_y = position.y.saturating_sub(active.start.y);
    let geometry = match active.mode {
        sophia_protocol::WmPointerGestureMode::Move => Rect {
            x: active.initial_geometry.x.saturating_add(delta_x),
            y: active.initial_geometry.y.saturating_add(delta_y),
            ..active.initial_geometry
        },
        sophia_protocol::WmPointerGestureMode::Resize => Rect {
            width: active.initial_geometry.width.saturating_add(delta_x).max(1),
            height: active.initial_geometry.height.saturating_add(delta_y).max(1),
            ..active.initial_geometry
        },
    };
    FloatingPointerOutline {
        surface: active.surface,
        start: active.start,
        geometry,
    }
}

fn floating_pointer_interaction(
    active: ActiveFloatingPointerGesture,
    phase: sophia_protocol::PolicyInteractionPhase,
) -> FloatingPointerPolicyInteraction {
    FloatingPointerPolicyInteraction {
        surface: active.surface,
        mode: active.mode,
        phase,
        start: active.start,
        current: active.current,
        geometry: floating_pointer_outline(active, active.current).geometry,
    }
}

impl FloatingPointerGestureState {
    pub(super) fn active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn cancel(&mut self) -> Option<FloatingPointerPolicyInteraction> {
        let active = self.active.take()?;
        Some(floating_pointer_interaction(
            active,
            sophia_protocol::PolicyInteractionPhase::Cancel,
        ))
    }
}

pub(super) fn clamp_floating_pointer_outline(
    mut outline: FloatingPointerOutline,
    output_bounds: &[(sophia_protocol::OutputId, Rect)],
) -> Option<FloatingPointerOutline> {
    let start_x = i64::from(outline.start.x);
    let start_y = i64::from(outline.start.y);
    let bounds = output_bounds.iter().find_map(|(_, bounds)| {
        let left = i64::from(bounds.x);
        let top = i64::from(bounds.y);
        let right = left + i64::from(bounds.width);
        let bottom = top + i64::from(bounds.height);
        (start_x >= left && start_x < right && start_y >= top && start_y < bottom)
            .then_some(*bounds)
    })?;
    outline.geometry.width = outline.geometry.width.min(bounds.width).max(1);
    outline.geometry.height = outline.geometry.height.min(bounds.height).max(1);
    outline.geometry.x = outline.geometry.x.clamp(
        bounds.x,
        bounds
            .x
            .saturating_add(bounds.width)
            .saturating_sub(outline.geometry.width),
    );
    outline.geometry.y = outline.geometry.y.clamp(
        bounds.y,
        bounds
            .y
            .saturating_add(bounds.height)
            .saturating_sub(outline.geometry.height),
    );
    Some(outline)
}

pub(super) fn observe_floating_pointer_gesture(
    state: &mut FloatingPointerGestureState,
    kind: sophia_protocol::InputEventKind,
    position: Option<sophia_protocol::WmPointerPosition>,
    target: Option<SurfaceId>,
    target_role: Option<sophia_protocol::SurfacePresentationRole>,
    target_geometry: Option<Rect>,
    super_held: bool,
) -> FloatingPointerGestureObservation {
    if let Some(mut active) = state.active {
        if let Some(position) = position {
            active.current = position;
        }
        let release_matches = matches!(
            kind,
            sophia_protocol::InputEventKind::PointerButton {
                button,
                pressed: false,
            } if button == active.button
        );
        let completed = release_matches.then(|| {
            state.active = None;
            sophia_protocol::WmPointerGestureCompleted {
                surface: active.surface,
                output: sophia_protocol::OutputId::INVALID,
                workspace: sophia_protocol::WorkspaceId::INVALID,
                mode: active.mode,
                start: active.start,
                end: active.current,
            }
        });
        let interaction = if release_matches {
            Some(floating_pointer_interaction(
                active,
                sophia_protocol::PolicyInteractionPhase::End,
            ))
        } else if matches!(kind, sophia_protocol::InputEventKind::PointerMotion) {
            state.active = Some(active);
            Some(floating_pointer_interaction(
                active,
                sophia_protocol::PolicyInteractionPhase::Update,
            ))
        } else {
            state.active = Some(active);
            None
        };
        return FloatingPointerGestureObservation {
            consumed: true,
            completed,
            interaction,
            outline: if release_matches {
                FloatingPointerOutlineUpdate::Clear
            } else if matches!(kind, sophia_protocol::InputEventKind::PointerMotion) {
                position.map_or(FloatingPointerOutlineUpdate::Unchanged, |position| {
                    FloatingPointerOutlineUpdate::Set(floating_pointer_outline(active, position))
                })
            } else {
                FloatingPointerOutlineUpdate::Unchanged
            },
        };
    }

    let Some((button, mode)) = (match kind {
        sophia_protocol::InputEventKind::PointerButton {
            button: 0x110,
            pressed: true,
        } => Some((0x110, sophia_protocol::WmPointerGestureMode::Move)),
        sophia_protocol::InputEventKind::PointerButton {
            button: 0x111,
            pressed: true,
        } => Some((0x111, sophia_protocol::WmPointerGestureMode::Resize)),
        _ => None,
    }) else {
        return FloatingPointerGestureObservation::default();
    };
    let Some(position) = position else {
        return FloatingPointerGestureObservation::default();
    };
    let Some(surface) = target.filter(|_| {
        super_held
            && target_role
                == Some(sophia_protocol::SurfacePresentationRole::PolicyManaged)
    }) else {
        return FloatingPointerGestureObservation::default();
    };
    let Some(initial_geometry) = target_geometry else {
        return FloatingPointerGestureObservation::default();
    };
    state.active = Some(ActiveFloatingPointerGesture {
        surface,
        mode,
        button,
        start: position,
        current: position,
        initial_geometry,
    });
    FloatingPointerGestureObservation {
        consumed: true,
        completed: None,
        interaction: Some(FloatingPointerPolicyInteraction {
            surface,
            mode,
            phase: sophia_protocol::PolicyInteractionPhase::Begin,
            start: position,
            current: position,
            geometry: initial_geometry,
        }),
        outline: FloatingPointerOutlineUpdate::Set(FloatingPointerOutline {
            surface,
            start: position,
            geometry: initial_geometry,
        }),
    }
}

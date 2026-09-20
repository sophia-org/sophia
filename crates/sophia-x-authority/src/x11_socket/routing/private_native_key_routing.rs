/// Until the private path carries an applied visual-transform/geometry
/// witness, only congruent Engine-root and X-logical observations are admitted.
/// A transformed or half-published geometry snapshot refuses before the effect;
/// root coordinates must not silently replace the source's transformed local.
fn key_pointer_path(
    selected: &XCoreEventSelectionState,
    focus_selected: &XCoreEventSelectionState,
    position: crate::XPointerObservation,
) -> Result<PrivateOrderedAncestry, crate::KeyboardPreparationRefusal> {
    use crate::KeyboardPreparationRefusal as R;
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let budget = PrivateTraversalBudget::new();
    let source = if position.surface_window == root {
        // Over the bare root: no surface, so no surface's tree. The path is
        // read from the focus client's own projection, descending from the
        // root to whatever of its mapped windows lies under the point, and
        // is the root alone when none does. The observation's local
        // coordinates are root coordinates, which is the only check the
        // root's geometry admits.
        if position.local_x != i32::from(position.root_x)
            || position.local_y != i32::from(position.root_y)
            || position.local_x < 0
            || position.local_y < 0
        {
            return Err(R::PointerNotApplied);
        }
        focus_selected
    } else {
        let source = if focus_selected
            .geometries
            .contains_key(&position.surface_window)
        {
            focus_selected
        } else if selected.geometries.contains_key(&position.surface_window) {
            selected
        } else {
            // THE POINTER IS SOMEWHERE THIS CLIENT CANNOT SEE, WHICH IS NOT A
            // REASON TO REFUSE THE KEY. Neither projection holds that window,
            // so it belongs to a third client -- or to nothing, if the
            // observation is stale. From here the two are the same fact and
            // the same answer.
            //
            // The core protocol already says what that answer is. A key press
            // is reported to the pointer's window only when the focus window
            // is one of its ancestors; otherwise it is reported to the focus
            // window itself. A pointer over somebody else's window is exactly
            // the second case, so the key is delivered, not lost.
            //
            // THE ROOT ALONE IS THE HONEST PATH. It says what this client can
            // actually establish -- the pointer is under the root, in a branch
            // outside its projection -- and `normal_key_target` already reads
            // a path its focus window is absent from as a disjoint branch and
            // begins delivery at focus. Nothing new decides this; the refusal
            // was simply reaching the decision first.
            //
            // Refusing here made an injected key fail on where the user last
            // left the pointer, which on a desktop with more than one window
            // is most of the time. The wire never saw it: the request is
            // answered and the refusal is behind it.
            return Ok(PrivateOrderedAncestry::new(root));
        };
        let geometry = source
            .geometries
            .get(&position.surface_window)
            .ok_or(R::Applied(PrivateAppliedRefusal::HierarchyMissing))?;
        let (origin_x, origin_y) = source
            .ordered_root_origin_budget(position.surface_window, &budget)
            .map_err(R::Applied)?;
        if i64::from(origin_x) + i64::from(position.local_x) != i64::from(position.root_x)
            || i64::from(origin_y) + i64::from(position.local_y) != i64::from(position.root_y)
            || position.local_x < 0
            || position.local_y < 0
            || position.local_x >= geometry.width
            || position.local_y >= geometry.height
        {
            return Err(R::PointerNotApplied);
        }
        source
    };
    source
        .applied_revision
        .ok_or(R::Applied(PrivateAppliedRefusal::Interrupted))?;
    let x = i16::try_from(position.local_x)
        .map_err(|_| R::Applied(PrivateAppliedRefusal::CoordinateOverflow))?;
    let y = i16::try_from(position.local_y)
        .map_err(|_| R::Applied(PrivateAppliedRefusal::CoordinateOverflow))?;
    let target = source
        .ordered_pointer_target_budget(position.surface_window, x, y, &budget)
        .map_err(R::Applied)?;
    let path = source
        .ordered_ancestry_budget(target, &budget)
        .map_err(R::Applied)?;
    if path
        .as_slice()
        .iter()
        .any(|window| *window != root && !source.mapped.contains(window))
    {
        return Err(R::PointerNotApplied);
    }
    Ok(path)
}

fn selected_key_at(view: &PrivateAppliedRoutingView<'_>, window: XResourceId) -> Option<bool> {
    if view
        .authority
        .xi_event_selected(view.namespace(), view.client.raw(), window, 3, 2)
    {
        Some(false)
    } else {
        view.selections.selects(window, 1).then_some(true)
    }
}

/// Normal delivery starts at the actual pointer descendant, stops at focus,
/// and tries XI2 before core on each window. Core DNP stops both streams.
/// Ungrabbed delivery then tries focus directly; owner-events grab delivery
/// instead falls back to the grab window in the caller.
fn normal_key_target(
    view: &PrivateAppliedRoutingView<'_>,
    pointer_path: &PrivateOrderedAncestry,
    retry_focus: bool,
) -> Result<(XResourceId, bool, PrivateOrderedAncestry), PrivateAppliedRefusal> {
    let focus = view.focus().ok_or(PrivateAppliedRefusal::FocusNotApplied)?;
    if focus.client != view.client {
        return Err(PrivateAppliedRefusal::ForeignOrigin);
    }
    let mut path = if let Some(depth) = pointer_path.depth(focus.window) {
        let mut path = *pointer_path;
        path.len = depth + 1;
        path
    } else {
        view.selections.ordered_ancestry(focus.window)?
    };
    // A known disjoint pointer branch means delivery starts at focus itself.
    if pointer_path.depth(focus.window).is_none() {
        path.len = 1;
    }
    for window in path.as_slice().iter().copied() {
        view.budget.charge(1)?;
        if let Some(core) = selected_key_at(view, window) {
            return Ok((window, core, path));
        }
        if window == focus.window
            || view
                .selections
                .windows
                .get(&window)
                .is_some_and(|selection| selection.do_not_propagate_mask & 1 != 0)
        {
            break;
        }
    }
    if retry_focus && path.as_slice()[0] != focus.window {
        view.budget.charge(1)?;
        if let Some(core) = selected_key_at(view, focus.window) {
            return Ok((focus.window, core, path));
        }
    }
    Err(PrivateAppliedRefusal::NotSelected)
}

fn resolve_key_plan(
    publication: &PrivateAppliedRoutingState,
    recipient: &PrivateAppliedClientRef<'_>,
    selections: &XCoreEventSelectionState,
    focus_selections: &XCoreEventSelectionState,
    prepared: &crate::PreparedKeyboardPress<'_>,
    position: crate::XPointerObservation,
) -> Result<KeyPlan, Refusal> {
    let view = publication
        .view(recipient.client, selections, prepared.authority())
        .map_err(Refusal::Resolution)?;
    let pointer_path = key_pointer_path(selections, focus_selections, position)
        .map_err(Refusal::KeyboardPreparation)?;
    let (window, core, ancestry) = if let Some(grab) = prepared.recipient() {
        let normal = if grab.owner_events {
            match normal_key_target(&view, &pointer_path, false) {
                Ok(plan) => Some(plan),
                Err(
                    PrivateAppliedRefusal::NotSelected
                    | PrivateAppliedRefusal::ForeignOrigin
                    | PrivateAppliedRefusal::FocusNotApplied,
                ) => None,
                Err(cause) => return Err(Refusal::Resolution(cause)),
            }
        } else {
            None
        };
        if let Some(plan) = normal {
            plan
        } else {
            if grab.event_mask & 1 == 0 && !grab.selects_xi_event(2) {
                return Err(Refusal::Resolution(PrivateAppliedRefusal::NotSelected));
            }
            let path = if pointer_path.depth(grab.window).is_some() {
                pointer_path
            } else {
                selections
                    .ordered_ancestry(grab.window)
                    .map_err(Refusal::Resolution)?
            };
            (grab.window, !grab.selects_xi_event(2), path)
        }
    } else {
        normal_key_target(&view, &pointer_path, true).map_err(Refusal::Resolution)?
    };
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    let pointer = XAuthorityPointerEvent {
        kind: XAuthorityPointerEventKind::Motion,
        surface: position.surface,
        root_x: position.root_x,
        root_y: position.root_y,
        event_x: position.root_x,
        event_y: position.root_y,
        state: 0,
        time_msec: 0,
    };
    let target = view
        .target(root, &ancestry, window, pointer)
        .map_err(Refusal::Resolution)?;
    Ok(KeyPlan {
        target,
        core,
        xkb_details: selections.xkb_state_details,
    })
}

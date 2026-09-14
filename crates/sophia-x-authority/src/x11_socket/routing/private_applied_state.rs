// The resolution interfaces below are an integration foundation. They have
// focused controls but no production consumer until the private runner and
// connection publication hooks are joined. Individual dead-code annotations
// describe that temporary boundary; they do not claim the hooks are wired.
/// Private policy: at most 64 parent links, including the root when present.
/// The ordinary path retains its existing traversal policy. Overflow here is
/// a refusal, never permission to deliver to a truncated ancestor chain.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
const PRIVATE_ORDERED_ANCESTRY: usize = 65;

/// Private work policy, not a claim about how many windows X may own. One
/// resolution shares this allowance across scans, geometry and ancestry reads.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
const PRIVATE_ORDERED_WINDOW_WORK: usize = 4096;

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateTraversalBudget(std::cell::Cell<usize>);

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl PrivateTraversalBudget {
    fn new() -> Self {
        Self(std::cell::Cell::new(PRIVATE_ORDERED_WINDOW_WORK))
    }
    fn charge(&self, count: usize) -> Result<(), PrivateAppliedRefusal> {
        self.0.set(
            self.0
                .get()
                .checked_sub(count)
                .ok_or(PrivateAppliedRefusal::TraversalBudget)?,
        );
        Ok(())
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
enum PrivateAppliedRefusal {
    Unpublished,
    Interrupted,
    IdentityExhausted,
    ForeignOrigin,
    UnboundSelections,
    FocusNotApplied,
    NotSelected,
    AmbiguousSelection,
    HierarchyOverflow,
    HierarchyCycle,
    HierarchyMissing,
    CoordinateOverflow,
    TraversalBudget,
    NativeRefused,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PrivateAppliedSelectionOrigin {
    authority: sophia_input_authority::AuthorityIdentity,
    namespace: NamespaceId,
    client: XServerFrontendClientId,
}

/// Only an applied runtime focus operation publishes this state. A queued
/// control has no publication method. The owner holds its mutex beneath common
/// and through the operation; readers retain that guard through application.
#[cfg(unix)]
#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateAppliedRoutingState {
    authority: sophia_input_authority::AuthorityIdentity,
    namespace: NamespaceId,
    revision: u64,
    published: bool,
    focus: Option<XServerFrontendSurfaceRoute>,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl PrivateAppliedRoutingState {
    fn new(authority: sophia_input_authority::AuthorityIdentity, namespace: NamespaceId) -> Self {
        Self {
            authority,
            namespace,
            revision: 0,
            published: false,
            focus: None,
        }
    }

    /// Invalidate before taking any native step. Losing the transaction leaves
    /// the view unavailable, including an unwind before or inside the effect.
    fn begin_focus_change(
        &mut self,
    ) -> Result<PrivateAppliedFocusChange<'_>, PrivateAppliedRefusal> {
        self.published = false;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(PrivateAppliedRefusal::IdentityExhausted)?;
        Ok(PrivateAppliedFocusChange { state: self })
    }

    fn view<'a>(
        &'a self,
        client: XServerFrontendClientId,
        selections: &'a XCoreEventSelectionState,
        authority: &'a crate::XInputAuthorityState,
    ) -> Result<PrivateAppliedRoutingView<'a>, PrivateAppliedRefusal> {
        if !self.published {
            return Err(PrivateAppliedRefusal::Unpublished);
        }
        let origin = selections
            .private_origin
            .ok_or(PrivateAppliedRefusal::UnboundSelections)?;
        if origin
            != (PrivateAppliedSelectionOrigin {
                authority: self.authority,
                namespace: self.namespace,
                client,
            })
        {
            return Err(PrivateAppliedRefusal::ForeignOrigin);
        }
        let selection_revision = selections
            .applied_revision
            .ok_or(PrivateAppliedRefusal::Interrupted)?;
        Ok(PrivateAppliedRoutingView {
            state: self,
            selections,
            authority,
            client,
            selection_revision,
            budget: PrivateTraversalBudget::new(),
        })
    }
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateAppliedFocusChange<'a> {
    state: &'a mut PrivateAppliedRoutingState,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl PrivateAppliedFocusChange<'_> {
    /// This is the effect producer, not an assertion that an effect succeeded.
    /// There is no separate commit method and no caller-supplied success bit.
    /// Runtime and routed-focus projection change before the publication.
    /// This implements Engine FocusSurface/ClearFocus (revert-to-parent).
    /// Core SetInputFocus has distinct None/root/revert arguments and still
    /// needs its own ordered producer integration, not coercion through this.
    fn apply(
        self,
        runtime: &mut XAuthorityRuntime,
        focused_projection: &AtomicU64,
        route: Option<XServerFrontendSurfaceRoute>,
    ) -> Result<(), PrivateAppliedRefusal> {
        if route.is_some_and(|route| route.namespace != self.state.namespace) {
            return Err(PrivateAppliedRefusal::ForeignOrigin);
        }
        let window = route.map_or(
            XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            |route| route.window,
        );
        runtime
            .set_input_focus(self.state.namespace, window, 1)
            .map_err(|_| PrivateAppliedRefusal::NativeRefused)?;
        focused_projection.store(window.local.raw(), Ordering::Release);
        self.state.focus = route;
        self.state.published = true;
        Ok(())
    }
}

/// This view borrows the applied publication and both selection authorities.
/// Its lifetime must stay within their held guards under common. An owned
/// result freezes a decision; it does not authorize a later native mutation.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateAppliedRoutingView<'a> {
    state: &'a PrivateAppliedRoutingState,
    selections: &'a XCoreEventSelectionState,
    authority: &'a crate::XInputAuthorityState,
    client: XServerFrontendClientId,
    selection_revision: u64,
    budget: PrivateTraversalBudget,
}

/// Derive grab authority from an actual held state or its exclusive pure
/// preparation. A newly implicit proposal can never supply selection masks.
#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
enum PrivatePointerSelection<'a, 'state> {
    Current,
    Prepared(&'a crate::PreparedPointerPress<'state>),
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl PrivateAppliedRoutingView<'_> {
    fn authority(&self) -> sophia_input_authority::AuthorityIdentity {
        self.state.authority
    }
    fn namespace(&self) -> NamespaceId {
        self.state.namespace
    }
    fn revision(&self) -> u64 {
        self.state.revision
    }
    fn selection_revision(&self) -> u64 {
        self.selection_revision
    }
    fn focus(&self) -> Option<XServerFrontendSurfaceRoute> {
        self.state.focus
    }

    fn keyboard(&self, pressed: bool) -> Result<PrivateResolvedKeyboard, PrivateAppliedRefusal> {
        let focus = self
            .state
            .focus
            .ok_or(PrivateAppliedRefusal::FocusNotApplied)?;
        if focus.client != self.client {
            return Err(PrivateAppliedRefusal::ForeignOrigin);
        }
        self.keyboard_for(focus.window, pressed)
    }

    /// The caller may use an already resolved authorized grab window. This
    /// method does not obtain or mutate a grab and never substitutes fallback
    /// focus or a most-recently-mapped window when selection is absent.
    fn keyboard_for(
        &self,
        window: XResourceId,
        pressed: bool,
    ) -> Result<PrivateResolvedKeyboard, PrivateAppliedRefusal> {
        self.budget.0.set(PRIVATE_ORDERED_WINDOW_WORK);
        let ancestry = self
            .selections
            .ordered_ancestry_budget(window, &self.budget)?;
        let event_type = if pressed { 2 } else { 3 };
        let mask = if pressed { 1 } else { 2 };
        self.budget.charge(ancestry.as_slice().len())?;
        let core = ancestry
            .as_slice()
            .iter()
            .copied()
            .find(|window| self.selections.selects(*window, mask));
        let xi = self.selected_xi(&ancestry, 3, event_type)?;
        let core_depth = core.and_then(|window| ancestry.depth(window));
        let xi_depth = xi.and_then(|window| ancestry.depth(window));
        let xi_wins = xi_depth.is_some_and(|depth| core_depth.is_none_or(|core| depth <= core));
        let delivered_window =
            if xi_wins { xi } else { core }.ok_or(PrivateAppliedRefusal::NotSelected)?;
        Ok(PrivateResolvedKeyboard {
            revision: self.revision(),
            selection_revision: self.selection_revision,
            delivered_window,
            core: !xi_wins,
            xi_event_type: xi_wins.then_some(event_type),
        })
    }

    /// Resolve every writer selection while these guards still answer for it.
    /// `previous` is the origin's previously committed ordered pointer target,
    /// not the writer's mutable remembered target. The caller advances it only
    /// when the guarded input effect commits. The exclusive preview supplies
    /// passive/active grab authority; a newly implicit proposal supplies none.
    fn pointer(
        &self,
        surface_window: XResourceId,
        pointer: XAuthorityPointerEvent,
        previous: Option<XResourceId>,
        selection: PrivatePointerSelection<'_, '_>,
    ) -> Result<PrivateResolvedPointer, PrivateAppliedRefusal> {
        self.budget.0.set(PRIVATE_ORDERED_WINDOW_WORK);
        let effective_grab = match selection {
            PrivatePointerSelection::Current => self.authority.pointer_grab(self.namespace()),
            PrivatePointerSelection::Prepared(prepared) => {
                if !std::ptr::eq(prepared.authority(), self.authority)
                    || prepared.namespace() != self.namespace()
                {
                    return Err(PrivateAppliedRefusal::ForeignOrigin);
                }
                (!prepared.is_new_implicit()).then(|| prepared.recipient())
            }
        };
        if effective_grab.is_some_and(|grab| grab.owner != self.client.raw()) {
            return Err(PrivateAppliedRefusal::ForeignOrigin);
        }
        let event_window = match effective_grab {
            Some(grab) if !grab.owner_events => grab.window,
            _ => self.selections.ordered_pointer_target_budget(
                surface_window,
                pointer.event_x,
                pointer.event_y,
                &self.budget,
            )?,
        };
        let ancestry = self
            .selections
            .ordered_ancestry_budget(event_window, &self.budget)?;
        let (mask, types) = match pointer.kind {
            XAuthorityPointerEventKind::Motion => (1 << 6, [(Some(6), 0), (None, 0)]),
            XAuthorityPointerEventKind::Button { pressed, .. } => (
                if pressed { 1 << 2 } else { 1 << 3 },
                [(Some(if pressed { 4 } else { 5 }), 0), (None, 0)],
            ),
            XAuthorityPointerEventKind::Axis {
                pressed,
                horizontal_position_v120,
                vertical_position_v120,
                ..
            } => (
                if pressed { 1 << 2 } else { 1 << 3 },
                [
                    (
                        (horizontal_position_v120.is_some() || vertical_position_v120.is_some())
                            .then_some(6),
                        0,
                    ),
                    (Some(if pressed { 4 } else { 5 }), XI_POINTER_EMULATED),
                ],
            ),
        };
        self.budget.charge(ancestry.as_slice().len())?;
        let selected = self
            .selections
            .ordered_pointer_selection(&ancestry, surface_window, mask);
        let selected = match effective_grab {
            Some(grab) if grab.event_mask & mask as u16 == 0 => None,
            Some(grab) if !grab.owner_events || selected.is_none() => Some(grab.window),
            _ => selected,
        };
        let mut core = selected
            .map(|window| self.target(surface_window, &ancestry, window, pointer))
            .transpose()?;
        let mut master = [None; 2];
        let mut source = [None; 2];
        for (index, (event_type, flags)) in types.into_iter().enumerate() {
            let Some(event_type) = event_type else {
                continue;
            };
            master[index] = if let Some(grab) =
                effective_grab.filter(|grab| grab.selects_xi_event(event_type))
            {
                Some(PrivateResolvedXi {
                    device: 2,
                    event_type,
                    flags,
                    target: self.target(surface_window, &ancestry, grab.window, pointer)?,
                })
            } else {
                self.xi(surface_window, &ancestry, pointer, 2, event_type, flags)?
            };
            source[index] = self.xi(
                surface_window,
                &ancestry,
                pointer,
                crate::X_INPUT_POINTER_SOURCE_ID,
                event_type,
                flags,
            )?;
        }
        let xi_depth = master
            .iter()
            .flatten()
            .map(|entry| entry.target.ancestry_depth)
            .min();
        if let Some(depth) = xi_depth {
            if core.is_none_or(|core| depth <= core.ancestry_depth) {
                core = None;
            } else {
                master = [None; 2];
            }
        }
        if core.is_none()
            && master.iter().all(Option::is_none)
            && source.iter().all(Option::is_none)
        {
            return Err(PrivateAppliedRefusal::NotSelected);
        }
        let delivered_window = PrivateResolvedPointer::primary_window(core, &master, &source)?;
        let mut crossings = [None; 6];
        if previous != Some(delivered_window) {
            for (side, (window, event_type)) in [(previous, 8), (Some(delivered_window), 7)]
                .into_iter()
                .enumerate()
            {
                let Some(window) = window else { continue };
                let chain = self
                    .selections
                    .ordered_ancestry_budget(window, &self.budget)?;
                if self.selections.crossing_selected(window, event_type == 7) {
                    crossings[side] = Some(PrivateResolvedCrossing {
                        device: None,
                        event_type,
                        target: self.target(surface_window, &chain, window, pointer)?,
                    });
                }
                for (offset, device) in [(2, 2), (4, crate::X_INPUT_POINTER_SOURCE_ID)] {
                    if let Some(target) = self.selected_xi(&chain, device, event_type)? {
                        crossings[offset + side] = Some(PrivateResolvedCrossing {
                            device: Some(device),
                            event_type,
                            target: self.target(surface_window, &chain, target, pointer)?,
                        });
                    }
                }
            }
        }
        Ok(PrivateResolvedPointer {
            revision: self.revision(),
            selection_revision: self.selection_revision,
            surface_window,
            event_window,
            delivered_window,
            ancestry,
            core,
            master,
            source,
            crossings,
        })
    }

    fn selected_xi(
        &self,
        ancestry: &PrivateOrderedAncestry,
        device: u16,
        event_type: u16,
    ) -> Result<Option<XResourceId>, PrivateAppliedRefusal> {
        for window in ancestry.as_slice().iter().copied() {
            self.budget.charge(1)?;
            if self.authority.xi_event_selected(
                self.namespace(),
                self.client.raw(),
                window,
                device,
                event_type,
            ) {
                return Ok(Some(window));
            }
        }
        Ok(None)
    }

    fn target(
        &self,
        surface: XResourceId,
        ancestry: &PrivateOrderedAncestry,
        window: XResourceId,
        pointer: XAuthorityPointerEvent,
    ) -> Result<PrivateResolvedTarget, PrivateAppliedRefusal> {
        self.budget.charge(ancestry.as_slice().len())?;
        let ancestry_depth = ancestry
            .depth(window)
            .ok_or(PrivateAppliedRefusal::HierarchyMissing)?;
        let child = ancestry_depth
            .checked_sub(1)
            .map_or(XResourceId::NONE, |index| ancestry.as_slice()[index]);
        let (event_x, event_y) = self.selections.ordered_coordinates_budget(
            surface,
            window,
            pointer.event_x,
            pointer.event_y,
            &self.budget,
        )?;
        Ok(PrivateResolvedTarget {
            window,
            child,
            event_x,
            event_y,
            ancestry_depth,
        })
    }

    fn xi(
        &self,
        surface: XResourceId,
        ancestry: &PrivateOrderedAncestry,
        pointer: XAuthorityPointerEvent,
        device: u16,
        event_type: u16,
        flags: u32,
    ) -> Result<Option<PrivateResolvedXi>, PrivateAppliedRefusal> {
        self.selected_xi(ancestry, device, event_type)?
            .map(|window| {
                Ok(PrivateResolvedXi {
                    device,
                    event_type,
                    flags,
                    target: self.target(surface, ancestry, window, pointer)?,
                })
            })
            .transpose()
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateResolvedKeyboard {
    revision: u64,
    selection_revision: u64,
    delivered_window: XResourceId,
    core: bool,
    xi_event_type: Option<u16>,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateResolvedTarget {
    window: XResourceId,
    child: XResourceId,
    event_x: i16,
    event_y: i16,
    ancestry_depth: usize,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateResolvedXi {
    device: u16,
    event_type: u16,
    flags: u32,
    target: PrivateResolvedTarget,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateResolvedCrossing {
    /// None is a core crossing; Some identifies the XI master or source.
    device: Option<u16>,
    event_type: u16,
    target: PrivateResolvedTarget,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateResolvedPointer {
    revision: u64,
    selection_revision: u64,
    surface_window: XResourceId,
    event_window: XResourceId,
    delivered_window: XResourceId,
    ancestry: PrivateOrderedAncestry,
    core: Option<PrivateResolvedTarget>,
    master: [Option<PrivateResolvedXi>; 2],
    source: [Option<PrivateResolvedXi>; 2],
    crossings: [Option<PrivateResolvedCrossing>; 6],
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl PrivateResolvedPointer {
    fn primary_recipient_window(&self) -> Result<XResourceId, PrivateAppliedRefusal> {
        Self::primary_window(self.core, &self.master, &self.source)
    }

    fn primary_window(
        core: Option<PrivateResolvedTarget>,
        master: &[Option<PrivateResolvedXi>; 2],
        source: &[Option<PrivateResolvedXi>; 2],
    ) -> Result<XResourceId, PrivateAppliedRefusal> {
        if let Some(core) = core {
            return Ok(core.window);
        }
        let primary = if master.iter().any(Option::is_some) {
            master
        } else {
            source
        };
        let mut window = None;
        for record in primary.iter().flatten() {
            if window.is_some_and(|window| window != record.target.window) {
                return Err(PrivateAppliedRefusal::AmbiguousSelection);
            }
            window = Some(record.target.window);
        }
        window.ok_or(PrivateAppliedRefusal::NotSelected)
    }
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))]
struct PrivateOrderedAncestry {
    entries: [XResourceId; PRIVATE_ORDERED_ANCESTRY],
    len: usize,
}

#[cfg(unix)]
#[cfg_attr(not(test), allow(dead_code))]
impl PrivateOrderedAncestry {
    fn new(window: XResourceId) -> Self {
        let mut entries = [XResourceId::NONE; PRIVATE_ORDERED_ANCESTRY];
        entries[0] = window;
        Self { entries, len: 1 }
    }
    fn as_slice(&self) -> &[XResourceId] {
        &self.entries[..self.len]
    }
    fn depth(&self, window: XResourceId) -> Option<usize> {
        self.as_slice().iter().position(|entry| *entry == window)
    }
    fn push(&mut self, window: XResourceId) -> Result<(), PrivateAppliedRefusal> {
        if self.as_slice().contains(&window) {
            return Err(PrivateAppliedRefusal::HierarchyCycle);
        }
        if self.len == self.entries.len() {
            return Err(PrivateAppliedRefusal::HierarchyOverflow);
        }
        self.entries[self.len] = window;
        self.len += 1;
        Ok(())
    }
}

#[cfg(unix)]
impl XCoreEventSelectionState {
    #[cfg_attr(not(test), allow(dead_code))]
    fn bind_private_origin(
        &mut self,
        origin: PrivateAppliedSelectionOrigin,
    ) -> Result<(), PrivateAppliedRefusal> {
        if self.private_origin.is_some_and(|old| old != origin) {
            return Err(PrivateAppliedRefusal::ForeignOrigin);
        }
        self.private_origin = Some(origin);
        Ok(())
    }

    fn begin_applied_mutation(&mut self) -> Option<u64> {
        let next = self
            .applied_revision
            .and_then(|revision| revision.checked_add(1));
        self.applied_revision = None;
        next
    }

    fn finish_applied_mutation(&mut self, revision: Option<u64>) {
        self.applied_revision = revision;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn ordered_ancestry(
        &self,
        window: XResourceId,
    ) -> Result<PrivateOrderedAncestry, PrivateAppliedRefusal> {
        self.ordered_ancestry_budget(window, &PrivateTraversalBudget::new())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn ordered_ancestry_budget(
        &self,
        window: XResourceId,
        budget: &PrivateTraversalBudget,
    ) -> Result<PrivateOrderedAncestry, PrivateAppliedRefusal> {
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let mut result = PrivateOrderedAncestry::new(window);
        let mut current = window;
        budget.charge(1)?;
        while current != root {
            budget.charge(1)?;
            let parent = self
                .parents
                .get(&current)
                .copied()
                .ok_or(PrivateAppliedRefusal::HierarchyMissing)?;
            result.push(parent)?;
            current = parent;
        }
        // A root reparent is invalid too; do not silently terminate a cycle.
        if self.parents.contains_key(&root) {
            return Err(PrivateAppliedRefusal::HierarchyCycle);
        }
        Ok(result)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn ordered_root_origin(
        &self,
        window: XResourceId,
    ) -> Result<(i32, i32), PrivateAppliedRefusal> {
        self.ordered_root_origin_budget(window, &PrivateTraversalBudget::new())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn ordered_root_origin_budget(
        &self,
        window: XResourceId,
        budget: &PrivateTraversalBudget,
    ) -> Result<(i32, i32), PrivateAppliedRefusal> {
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let mut x = 0_i32;
        let mut y = 0_i32;
        for window in self
            .ordered_ancestry_budget(window, budget)?
            .as_slice()
            .iter()
            .copied()
            .take_while(|window| *window != root)
        {
            budget.charge(1)?;
            let geometry = self
                .geometries
                .get(&window)
                .ok_or(PrivateAppliedRefusal::HierarchyMissing)?;
            x = x
                .checked_add(geometry.x)
                .ok_or(PrivateAppliedRefusal::CoordinateOverflow)?;
            y = y
                .checked_add(geometry.y)
                .ok_or(PrivateAppliedRefusal::CoordinateOverflow)?;
        }
        Ok((x, y))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn ordered_coordinates_budget(
        &self,
        surface: XResourceId,
        target: XResourceId,
        event_x: i16,
        event_y: i16,
        budget: &PrivateTraversalBudget,
    ) -> Result<(i16, i16), PrivateAppliedRefusal> {
        let (surface_x, surface_y) = self.ordered_root_origin_budget(surface, budget)?;
        let (target_x, target_y) = self.ordered_root_origin_budget(target, budget)?;
        let x = i64::from(event_x) + i64::from(surface_x) - i64::from(target_x);
        let y = i64::from(event_y) + i64::from(surface_y) - i64::from(target_y);
        Ok((
            x.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16,
            y.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16,
        ))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn ordered_pointer_target_budget(
        &self,
        surface: XResourceId,
        event_x: i16,
        event_y: i16,
        budget: &PrivateTraversalBudget,
    ) -> Result<XResourceId, PrivateAppliedRefusal> {
        let (surface_x, surface_y) = self.ordered_root_origin_budget(surface, budget)?;
        let mut visited = PrivateOrderedAncestry::new(surface);
        let mut target = surface;
        loop {
            let mut next = None;
            for child in self.stacking.iter().rev().copied() {
                budget.charge(1)?;
                if self.parents.get(&child) != Some(&target) || !self.mapped.contains(&child) {
                    continue;
                }
                let geometry = self
                    .geometries
                    .get(&child)
                    .ok_or(PrivateAppliedRefusal::HierarchyMissing)?;
                let (child_x, child_y) = self.ordered_root_origin_budget(child, budget)?;
                let x = i64::from(event_x) + i64::from(surface_x) - i64::from(child_x);
                let y = i64::from(event_y) + i64::from(surface_y) - i64::from(child_y);
                if x >= 0
                    && y >= 0
                    && x < i64::from(geometry.width)
                    && y < i64::from(geometry.height)
                {
                    next = Some(child);
                    break;
                }
            }
            let Some(child) = next else { return Ok(target) };
            visited.push(child)?;
            target = child;
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn ordered_pointer_selection(
        &self,
        ancestry: &PrivateOrderedAncestry,
        surface: XResourceId,
        mask: u32,
    ) -> Option<XResourceId> {
        for window in ancestry.as_slice().iter().copied() {
            let selection = self.windows.get(&window).copied().unwrap_or_default();
            if selection.mask & mask != 0 {
                return Some(window);
            }
            if window == surface || selection.do_not_propagate_mask & mask != 0 {
                break;
            }
        }
        None
    }
}

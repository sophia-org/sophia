#[cfg(unix)]
fn x11_core_event_selection_update(
    request: &crate::XWireRequest,
) -> Option<(XResourceId, Option<u32>, Option<u32>)> {
    match request {
        crate::XWireRequest::CreateWindow {
            packet:
                crate::XAuthorityRequestPacket {
                    kind: crate::XAuthorityRequestKind::CreateWindow { window, .. },
                    ..
                },
            event_mask,
            do_not_propagate_mask,
            ..
        }
        | crate::XWireRequest::ChangeWindowAttributes {
            window,
            event_mask,
            do_not_propagate_mask,
            ..
        } => Some((*window, *event_mask, *do_not_propagate_mask)),
        _ => None,
    }
}
#[derive(Clone, Copy, Debug, Default)]
struct XCoreWindowEventSelection {
    mask: u32,
    do_not_propagate_mask: u32,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
struct XCorePointerSnapshot {
    surface_window: XResourceId,
    pointer_window: XResourceId,
    root_x: i16,
    root_y: i16,
    event_x: i16,
    event_y: i16,
    mask: u16,
}

#[cfg(unix)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct XCorePointerQuery {
    child: XResourceId,
    root_x: i16,
    root_y: i16,
    win_x: i16,
    win_y: i16,
    mask: u16,
}

#[cfg(unix)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct XCoreMapTransition {
    viewable: bool,
    promoted_descendants: Vec<XResourceId>,
}

#[cfg(unix)]
#[derive(Debug)]
struct XCoreEventSelectionState {
    // None is an interrupted/exhausted mutation, never a clear selection.
    applied_revision: Option<u64>,
    // Bound only by private connection setup; ordinary clients do not use it.
    #[cfg_attr(not(test), allow(dead_code))]
    private_origin: Option<PrivateAppliedSelectionOrigin>,
    // Actual XKB selection, captured under the same guard as core/XI routing.
    // The ordinary writer's atomic is a projection of this source update.
    xkb_state_details: u16,
    windows: BTreeMap<XResourceId, XCoreWindowEventSelection>,
    parents: BTreeMap<XResourceId, XResourceId>,
    geometries: BTreeMap<XResourceId, Rect>,
    stacking: Vec<XResourceId>,
    mapped: BTreeSet<XResourceId>,
    fallback_mapped_window: XResourceId,
    pointer: Option<XCorePointerSnapshot>,
}

#[cfg(unix)]
impl Default for XCoreEventSelectionState {
    fn default() -> Self {
        Self {
            applied_revision: Some(1),
            private_origin: None,
            xkb_state_details: 0,
            windows: BTreeMap::new(),
            parents: BTreeMap::new(),
            geometries: BTreeMap::new(),
            stacking: Vec::new(),
            mapped: BTreeSet::new(),
            fallback_mapped_window: XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1),
            pointer: None,
        }
    }
}

#[cfg(unix)]
impl XCoreEventSelectionState {
    const KEY_MASKS: u32 = (1 << 0) | (1 << 1);
    const BUTTON_MASKS: u32 = (1 << 2) | (1 << 3);
    const POINTER_MOTION_MASK: u32 = 1 << 6;
    const ENTER_WINDOW_MASK: u32 = 1 << 4;
    const LEAVE_WINDOW_MASK: u32 = 1 << 5;
    const FOCUS_CHANGE_MASK: u32 = 1 << 21;

    fn select_xkb_state_notifications(
        &mut self,
        ordinary_projection: &AtomicU16,
        affect_which: u16,
        clear: u16,
        select_all: u16,
        state: Option<(u16, u16)>,
    ) {
        let revision = self.begin_applied_mutation();
        let mut details = self.xkb_state_details;
        if clear & 4 != 0 {
            details = 0;
        }
        if select_all & 4 != 0 {
            details = u16::MAX;
        }
        if affect_which & 4 != 0 && let Some((affect, selected)) = state {
            details = (details & !affect) | (selected & affect);
        }
        self.xkb_state_details = details;
        ordinary_projection.store(details, Ordering::Release);
        self.finish_applied_mutation(revision);
    }

    fn update(
        &mut self,
        window: XResourceId,
        event_mask: Option<u32>,
        do_not_propagate_mask: Option<u32>,
    ) {
        if event_mask.is_none() && do_not_propagate_mask.is_none() {
            return;
        }
        let revision = self.begin_applied_mutation();
        let selection = self.windows.entry(window).or_default();
        if let Some(mask) = event_mask {
            selection.mask = mask;
        }
        if let Some(mask) = do_not_propagate_mask {
            selection.do_not_propagate_mask = mask;
        }
        self.finish_applied_mutation(revision);
    }

    fn register(&mut self, window: XResourceId, parent: XResourceId, geometry: Rect) {
        let revision = self.begin_applied_mutation();
        self.parents.insert(window, parent);
        self.geometries.insert(window, geometry);
        self.stacking.retain(|candidate| *candidate != window);
        self.stacking.push(window);
        self.finish_applied_mutation(revision);
    }

    fn reparent(&mut self, window: XResourceId, parent: XResourceId, x: i16, y: i16) {
        let revision = self.begin_applied_mutation();
        self.parents.insert(window, parent);
        if let Some(geometry) = self.geometries.get_mut(&window) {
            geometry.x = i32::from(x);
            geometry.y = i32::from(y);
        }
        self.finish_applied_mutation(revision);
    }

    fn configure_geometry(
        &mut self,
        window: XResourceId,
        x: Option<i16>,
        y: Option<i16>,
        width: Option<u16>,
        height: Option<u16>,
    ) {
        let revision = self.begin_applied_mutation();
        let geometry = self.geometries.entry(window).or_insert(Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        });
        if let Some(x) = x {
            geometry.x = i32::from(x);
        }
        if let Some(y) = y {
            geometry.y = i32::from(y);
        }
        if let Some(width) = width {
            geometry.width = i32::from(width);
        }
        if let Some(height) = height {
            geometry.height = i32::from(height);
        }
        self.finish_applied_mutation(revision);
    }

    fn update_geometry(&mut self, window: XResourceId, geometry: Rect) {
        let revision = self.begin_applied_mutation();
        self.geometries.insert(window, geometry);
        self.finish_applied_mutation(revision);
    }

    fn restack(&mut self, window: XResourceId, sibling: Option<XResourceId>, mode: Option<u8>) {
        let revision = self.begin_applied_mutation();
        self.stacking.retain(|candidate| *candidate != window);
        let sibling_index = sibling.and_then(|sibling| {
            self.stacking
                .iter()
                .position(|candidate| *candidate == sibling)
        });
        let index = match (mode, sibling_index) {
            (Some(1 | 3), Some(index)) => index,
            (Some(1 | 3), None) => 0,
            (Some(0 | 2 | 4), Some(index)) => index.saturating_add(1),
            _ => self.stacking.len(),
        };
        self.stacking.insert(index.min(self.stacking.len()), window);
        self.finish_applied_mutation(revision);
    }

    fn observe_mapped(&mut self, window: XResourceId) -> XCoreMapTransition {
        let revision = self.begin_applied_mutation();
        if !self.mapped.insert(window) {
            self.finish_applied_mutation(revision);
            return XCoreMapTransition {
                viewable: self.is_viewable(window),
                promoted_descendants: Vec::new(),
            };
        }
        self.fallback_mapped_window = window;
        let viewable = self.is_viewable(window);
        let promoted_descendants = if viewable {
            self.viewable_descendants(window)
        } else {
            Vec::new()
        };
        self.finish_applied_mutation(revision);
        XCoreMapTransition {
            viewable,
            promoted_descendants,
        }
    }

    fn observe_unmapped(&mut self, window: XResourceId) {
        let revision = self.begin_applied_mutation();
        self.mapped.remove(&window);
        if self.fallback_mapped_window == window {
            self.fallback_mapped_window = self
                .stacking
                .iter()
                .rev()
                .copied()
                .find(|candidate| self.mapped.contains(candidate))
                .unwrap_or_else(|| XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1));
        }
        self.finish_applied_mutation(revision);
    }

    fn is_viewable(&self, window: XResourceId) -> bool {
        if !self.mapped.contains(&window) {
            return false;
        }
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let mut current = window;
        while let Some(parent) = self.parents.get(&current).copied() {
            if parent == root {
                return true;
            }
            if !self.mapped.contains(&parent) {
                return false;
            }
            current = parent;
        }
        false
    }

    fn viewable_descendants(&self, parent: XResourceId) -> Vec<XResourceId> {
        let mut descendants = Vec::new();
        self.collect_viewable_descendants(parent, &mut descendants);
        descendants
    }

    fn collect_viewable_descendants(
        &self,
        parent: XResourceId,
        descendants: &mut Vec<XResourceId>,
    ) {
        for child in self
            .parents
            .iter()
            .filter_map(|(child, candidate)| (*candidate == parent).then_some(*child))
        {
            if !self.mapped.contains(&child) {
                continue;
            }
            descendants.push(child);
            self.collect_viewable_descendants(child, descendants);
        }
    }

    fn geometry(&self, window: XResourceId) -> Option<Rect> {
        self.geometries.get(&window).copied()
    }

    fn parent(&self, window: XResourceId) -> Option<XResourceId> {
        self.parents.get(&window).copied()
    }

    fn selects(&self, window: XResourceId, mask: u32) -> bool {
        self.windows
            .get(&window)
            .is_some_and(|selection| selection.mask & mask != 0)
    }

    fn remove(&mut self, window: XResourceId) {
        let revision = self.begin_applied_mutation();
        self.windows.remove(&window);
        self.parents.remove(&window);
        self.geometries.remove(&window);
        self.stacking.retain(|candidate| *candidate != window);
        self.mapped.remove(&window);
        if self.fallback_mapped_window == window {
            self.fallback_mapped_window = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        }
        self.finish_applied_mutation(revision);
    }

    fn keyboard_target(&self, focused: XResourceId) -> XResourceId {
        self.selected_keyboard_target(focused)
            .unwrap_or_else(|| self.keyboard_fallback(focused))
    }

    /// Where a key is reported, by the protocol's rule rather than by the
    /// focus window alone.
    ///
    /// This used to walk up from the focus checking one mask, which got the
    /// common case right and two things wrong. It never delivered to the
    /// window the pointer was in when that window was inside the focus
    /// subtree, and it never honoured a do-not-propagate mask, which
    /// `selected_pointer_target` directly below has always honoured -- so a
    /// client setting it was obeyed for buttons and ignored for keys.
    ///
    /// The rule is [`crate::key_routing::x_key_delivery_target`], shared with
    /// the private delivery path so the two cannot drift again. The ceiling
    /// is this path's own: it continues to the focus's ancestors, as Xorg
    /// does, where the private path stops at the focus.
    fn selected_keyboard_target(&self, focused: XResourceId) -> Option<XResourceId> {
        let focus = self.keyboard_fallback(focused);
        let above_focus = self.ancestry_including(focus);
        // The pointer's own chain when it reaches the focus, truncated there
        // and then continued upward; the focus's chain alone when the pointer
        // is on a branch the focus does not contain.
        let delivery_path = match self
            .pointer_window()
            .map(|pointer| self.ancestry_including(pointer))
            .filter(|path| path.contains(&focus))
        {
            Some(path) => {
                let depth = path
                    .iter()
                    .position(|window| *window == focus)
                    .unwrap_or_default();
                let mut path = path[..depth].to_vec();
                path.extend(above_focus);
                path
            }
            None => above_focus,
        };
        let found = crate::key_routing::x_key_delivery_target::<()>(
            focus,
            &delivery_path,
            &mut |window| Ok(self.selects(window, Self::KEY_MASKS).then_some(true)),
            &|window| {
                self.windows
                    .get(&window)
                    .is_some_and(|selection| selection.do_not_propagate_mask & Self::KEY_MASKS != 0)
            },
            // No second try at the focus. This path's delivery path already
            // contains the focus, so a retry can only fire when the walk was
            // stopped early by a do-not-propagate mask -- and offering the
            // event to the focus anyway is exactly what that mask forbids.
            false,
        );
        found.ok().flatten().map(|(window, _)| window)
    }

    fn selected_pointer_target(
        &self,
        surface_window: XResourceId,
        motion: bool,
        event_x: i16,
        event_y: i16,
    ) -> Option<XResourceId> {
        let selected_mask = if motion {
            Self::POINTER_MOTION_MASK
        } else {
            Self::BUTTON_MASKS
        };
        let event_window = self.pointer_event_target(surface_window, event_x, event_y);
        for candidate in self.ancestry_including(event_window) {
            let selection = self.windows.get(&candidate).copied().unwrap_or_default();
            if selection.mask & selected_mask != 0 {
                return Some(candidate);
            }
            if candidate == surface_window || selection.do_not_propagate_mask & selected_mask != 0 {
                break;
            }
        }
        None
    }

    fn pointer_event_target(
        &self,
        surface_window: XResourceId,
        event_x: i16,
        event_y: i16,
    ) -> XResourceId {
        let mut target = surface_window;
        for _ in 0..64 {
            let Some(child) = self.stacking.iter().rev().copied().find(|candidate| {
                self.parents.get(candidate) == Some(&target)
                    && self.mapped.contains(candidate)
                    && self.contains_surface_point(
                        surface_window,
                        *candidate,
                        i32::from(event_x),
                        i32::from(event_y),
                    )
            }) else {
                break;
            };
            target = child;
        }
        target
    }

    /// The window the pointer is in, when one has been observed.
    ///
    /// The focus algebra needs it because a transition that crosses on or off
    /// the pointer's chain owes that chain its own events. With no observation
    /// yet the caller falls back to the root, which is where the pointer is
    /// when it is in no other window.
    fn pointer_window(&self) -> Option<XResourceId> {
        self.pointer.map(|pointer| pointer.pointer_window)
    }

    fn ancestry_including(&self, window: XResourceId) -> Vec<XResourceId> {
        std::iter::once(window)
            .chain(self.ancestors(window))
            .collect()
    }

    fn crossing_selected(&self, window: XResourceId, entered: bool) -> bool {
        let mask = if entered {
            Self::ENTER_WINDOW_MASK
        } else {
            Self::LEAVE_WINDOW_MASK
        };
        self.windows
            .get(&window)
            .is_some_and(|selection| selection.mask & mask != 0)
    }

    fn focus_selected(&self, window: XResourceId) -> bool {
        self.windows
            .get(&window)
            .is_some_and(|selection| selection.mask & Self::FOCUS_CHANGE_MASK != 0)
    }

    fn pointer_event_coordinates(
        &self,
        surface_window: XResourceId,
        delivered_window: XResourceId,
        event_x: i16,
        event_y: i16,
    ) -> (i16, i16) {
        let Some((surface_x, surface_y)) = self.root_origin(surface_window) else {
            return (event_x, event_y);
        };
        let Some((delivered_x, delivered_y)) = self.root_origin(delivered_window) else {
            return (event_x, event_y);
        };
        (
            clamp_engine_i16(i32::from(event_x) + surface_x - delivered_x),
            clamp_engine_i16(i32::from(event_y) + surface_y - delivered_y),
        )
    }

    fn contains_surface_point(
        &self,
        surface_window: XResourceId,
        candidate: XResourceId,
        event_x: i32,
        event_y: i32,
    ) -> bool {
        if candidate == surface_window {
            return true;
        }
        let Some(geometry) = self.geometries.get(&candidate) else {
            return true;
        };
        let Some((surface_x, surface_y)) = self.root_origin(surface_window) else {
            return true;
        };
        let Some((candidate_x, candidate_y)) = self.root_origin(candidate) else {
            return true;
        };
        let local_x = event_x + surface_x - candidate_x;
        let local_y = event_y + surface_y - candidate_y;
        local_x >= 0 && local_y >= 0 && local_x < geometry.width && local_y < geometry.height
    }

    fn root_origin(&self, window: XResourceId) -> Option<(i32, i32)> {
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let mut candidate = window;
        let mut x = 0_i32;
        let mut y = 0_i32;
        for _ in 0..64 {
            if candidate == root {
                return Some((x, y));
            }
            let geometry = self.geometries.get(&candidate)?;
            x = x.saturating_add(geometry.x);
            y = y.saturating_add(geometry.y);
            candidate = self.parents.get(&candidate).copied()?;
        }
        None
    }

    #[allow(clippy::too_many_arguments)]
    fn observe_pointer(
        &mut self,
        surface_window: XResourceId,
        pointer_window: XResourceId,
        root_x: i16,
        root_y: i16,
        event_x: i16,
        event_y: i16,
        mask: u16,
    ) {
        let revision = self.begin_applied_mutation();
        self.pointer = Some(XCorePointerSnapshot {
            surface_window,
            pointer_window,
            root_x,
            root_y,
            event_x,
            event_y,
            mask,
        });
        self.finish_applied_mutation(revision);
    }

    fn query_pointer(&self, window: XResourceId) -> Option<XCorePointerQuery> {
        let pointer = self.pointer?;
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let ancestry = self.ancestry_including(pointer.pointer_window);
        let child = if window == root {
            ancestry
                .iter()
                .position(|candidate| *candidate == root)
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| ancestry.get(index).copied())
                .unwrap_or(pointer.surface_window)
        } else {
            ancestry
                .iter()
                .position(|candidate| *candidate == window)
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| ancestry.get(index).copied())
                .unwrap_or(XResourceId::NONE)
        };
        let (win_x, win_y) = if window == root {
            (pointer.root_x, pointer.root_y)
        } else {
            self.pointer_event_coordinates(
                pointer.surface_window,
                window,
                pointer.event_x,
                pointer.event_y,
            )
        };
        Some(XCorePointerQuery {
            child,
            root_x: pointer.root_x,
            root_y: pointer.root_y,
            win_x,
            win_y,
            mask: pointer.mask,
        })
    }

    fn keyboard_fallback(&self, focused: XResourceId) -> XResourceId {
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        if focused == root {
            self.stacking
                .iter()
                .rev()
                .copied()
                .find(|window| self.mapped.contains(window))
                .unwrap_or(self.fallback_mapped_window)
        } else {
            focused
        }
    }

    fn ancestors(&self, window: XResourceId) -> Vec<XResourceId> {
        let mut ancestors = Vec::new();
        let mut candidate = window;
        for _ in 0..64 {
            let Some(parent) = self.parents.get(&candidate).copied() else {
                break;
            };
            ancestors.push(parent);
            candidate = parent;
        }
        ancestors
    }
}

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
/// What the keyboard rule decided for one event.
///
/// Three outcomes rather than an `Option`, because "nobody has selected this
/// yet" and "this must not be delivered" are different instructions and used
/// to be the same `None`. The writer waits out a startup race for the first
/// and must not wait at all for the second.
/// Why a key is delivered nowhere. Both are the client's own decision; they
/// are told apart only so the trace says which one happened.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XKeyDiscard {
    /// The focus is `None`.
    FocusNone,
    /// A do-not-propagate mask ended the walk with nobody selecting.
    DoNotPropagate,
}

/// One EnterNotify or LeaveNotify of a pointer move: the window it is
/// reported on, its detail (Ancestor, Virtual, Inferior, Nonlinear,
/// NonlinearVirtual) and the child of that window containing the pointer's
/// position on the other side of the move, None when there is none.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct XPointerCrossingStep {
    pub(crate) window: XResourceId,
    pub(crate) entered: bool,
    pub(crate) detail: u8,
    pub(crate) child: XResourceId,
}

/// The crossing events of a move from `from` to `to`, given each window's
/// ancestry (the window first, then its parents up to the root). The
/// protocol's three shapes: `to` an inferior of `from` (leave `from` as
/// Inferior, enter the windows between as Virtual, enter `to` as
/// Ancestor); `to` an ancestor of `from` (leave `from` as Ancestor, leave
/// the windows between as Virtual, enter `to` as Inferior); and otherwise
/// through the nearest common ancestor, which itself hears nothing (leave
/// `from` as Nonlinear, leave and enter the windows between as
/// NonlinearVirtual, enter `to` as Nonlinear). Every leave precedes every
/// enter.
pub(crate) fn x11_pointer_crossings(
    ancestry_from: &[XResourceId],
    ancestry_to: &[XResourceId],
) -> Vec<XPointerCrossingStep> {
    const ANCESTOR: u8 = 0;
    const VIRTUAL: u8 = 1;
    const INFERIOR: u8 = 2;
    const NONLINEAR: u8 = 3;
    const NONLINEAR_VIRTUAL: u8 = 4;
    let (Some(&from), Some(&to)) = (ancestry_from.first(), ancestry_to.first()) else {
        return Vec::new();
    };
    if from == to {
        return Vec::new();
    }
    // The child of ancestry[depth] toward its window: the element below it.
    let below = |ancestry: &[XResourceId], depth: usize| {
        depth.checked_sub(1).map_or(XResourceId::NONE, |index| ancestry[index])
    };
    let mut steps = Vec::new();
    if let Some(depth) = ancestry_to.iter().position(|window| *window == from) {
        // `from` is an ancestor of `to`.
        steps.push(XPointerCrossingStep { window: from, entered: false, detail: INFERIOR, child: below(ancestry_to, depth) });
        for index in (1..depth).rev() {
            steps.push(XPointerCrossingStep { window: ancestry_to[index], entered: true, detail: VIRTUAL, child: below(ancestry_to, index) });
        }
        steps.push(XPointerCrossingStep { window: to, entered: true, detail: ANCESTOR, child: XResourceId::NONE });
        return steps;
    }
    if let Some(depth) = ancestry_from.iter().position(|window| *window == to) {
        // `to` is an ancestor of `from`.
        steps.push(XPointerCrossingStep { window: from, entered: false, detail: ANCESTOR, child: XResourceId::NONE });
        for index in 1..depth {
            steps.push(XPointerCrossingStep { window: ancestry_from[index], entered: false, detail: VIRTUAL, child: below(ancestry_from, index) });
        }
        steps.push(XPointerCrossingStep { window: to, entered: true, detail: INFERIOR, child: below(ancestry_from, depth) });
        return steps;
    }
    let common_from = ancestry_from
        .iter()
        .position(|window| ancestry_to.contains(window))
        .unwrap_or(ancestry_from.len());
    let common_to = ancestry_from
        .get(common_from)
        .and_then(|common| ancestry_to.iter().position(|window| window == common))
        .unwrap_or(ancestry_to.len());
    steps.push(XPointerCrossingStep { window: from, entered: false, detail: NONLINEAR, child: XResourceId::NONE });
    for index in 1..common_from {
        steps.push(XPointerCrossingStep { window: ancestry_from[index], entered: false, detail: NONLINEAR_VIRTUAL, child: below(ancestry_from, index) });
    }
    for index in (1..common_to).rev() {
        steps.push(XPointerCrossingStep { window: ancestry_to[index], entered: true, detail: NONLINEAR_VIRTUAL, child: below(ancestry_to, index) });
    }
    steps.push(XPointerCrossingStep { window: to, entered: true, detail: NONLINEAR, child: XResourceId::NONE });
    steps
}

/// Which half of the pointer selection a core event answers to: the motion
/// masks (chosen by the held buttons), ButtonPress or ButtonRelease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XPointerSelection {
    Motion,
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XKeyDelivery {
    /// Report the key with respect to this window.
    Window(XResourceId),
    /// Deliver nothing, and do not wait: a focus of `None` discards keyboard
    /// events until a focus is set again, and no amount of waiting changes
    /// what a client has decided.
    Discard(XKeyDiscard),
    /// No window has selected the event yet, which may still be a client
    /// that has not finished starting up.
    Unselected,
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
    const KEY_PRESS_MASK: u32 = 1 << 0;
    const KEY_RELEASE_MASK: u32 = 1 << 1;
    const BUTTON_PRESS_MASK: u32 = 1 << 2;
    const BUTTON_RELEASE_MASK: u32 = 1 << 3;
    const POINTER_MOTION_MASK: u32 = 1 << 6;
    /// ButtonMotion: motion while any button is down.
    const BUTTON_MOTION_MASK: u32 = 1 << 13;
    /// Button1Mask..Button5Mask in an event's state field, which occupy the
    /// same bits as Button1Motion..Button5Motion in an event mask.
    const HELD_BUTTON_STATE: u32 = 0x1F00;

    /// The masks a MotionNotify answers to. PointerMotion always; with buttons
    /// down, ButtonMotion and each held button's own ButtonNMotion, which is
    /// how a text widget follows a drag without asking for every motion --
    /// xterm's `<Btn1Motion>: select-extend()` selects Button1Motion alone.
    /// Delivering motion only to PointerMotion selectors left such a widget
    /// blind until the release, so xterm highlighted a selection only once
    /// the button came up.
    pub(in crate::x11_socket) fn motion_selection_mask(state: u16) -> u32 {
        let held = u32::from(state) & Self::HELD_BUTTON_STATE;
        Self::POINTER_MOTION_MASK | held | if held != 0 { Self::BUTTON_MOTION_MASK } else { 0 }
    }
    const ENTER_WINDOW_MASK: u32 = 1 << 4;
    const LEAVE_WINDOW_MASK: u32 = 1 << 5;
    const KEYMAP_STATE_MASK: u32 = 1 << 14;
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

    fn keyboard_target(&self, focused: XResourceId, pressed: bool) -> XResourceId {
        self.selected_keyboard_target(focused, pressed)
            .unwrap_or_else(|| self.keyboard_fallback(focused))
    }

    /// The rule's answer as an `Option`, for callers that have no way to act
    /// on a discard. Prefer [`Self::keyboard_delivery`], which distinguishes
    /// "must not be delivered" from "nobody has selected it yet".
    fn selected_keyboard_target(&self, focused: XResourceId, pressed: bool) -> Option<XResourceId> {
        match self.keyboard_delivery(focused, pressed) {
            XKeyDelivery::Window(window) => Some(window),
            XKeyDelivery::Discard(_) | XKeyDelivery::Unselected => None,
        }
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
    ///
    /// `pressed` names the half of the keyboard selection that applies: a
    /// press is owed to KeyPressMask and a release to KeyReleaseMask, and a
    /// client holding one of the two is owed only that half. One combined
    /// mask here wrote presses to clients that had selected releases alone
    /// (XTS Xlib11 KeyPress 3).
    pub(crate) fn keyboard_delivery(&self, focused: XResourceId, pressed: bool) -> XKeyDelivery {
        self.keyboard_delivery_selecting(
            focused,
            if pressed { Self::KEY_PRESS_MASK } else { Self::KEY_RELEASE_MASK },
        )
    }

    /// Whether any keyboard selection at all decides delivery from `focused`.
    /// The readiness wait for a client still installing its masks asks this,
    /// not the direction's own rule: a client that selected releases alone
    /// has decided, and holding its presses for the deadline would only delay
    /// every event queued behind them.
    pub(crate) fn keyboard_selection_decided(&self, focused: XResourceId) -> bool {
        !matches!(
            self.keyboard_delivery_selecting(focused, Self::KEY_PRESS_MASK | Self::KEY_RELEASE_MASK),
            XKeyDelivery::Unselected
        )
    }

    fn keyboard_delivery_selecting(&self, focused: XResourceId, key_mask: u32) -> XKeyDelivery {
        let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
        let focus = match u32::try_from(focused.local.raw()) {
            // A focus of None discards keyboard events until a focus is set
            // again. It is a decision, not an absence, so it must not be
            // confused with nobody having selected the event yet.
            Ok(crate::X_FOCUS_NONE) => return XKeyDelivery::Discard(XKeyDiscard::FocusNone),
            // PointerRoot is the root of the screen the pointer is on,
            // resolved at each event rather than stored. Taking the root as
            // the focus is the whole implementation: every window is in the
            // root's subtree, so the subtree rule below then reports the key
            // on the pointer's own window, which is what PointerRoot means.
            //
            // keyboard_fallback is deliberately not consulted here. Its
            // topmost-mapped-window substitution is for a focus that really
            // is the root, and applying it to PointerRoot would pin the
            // answer to one window and stop it following the pointer.
            Ok(crate::X_FOCUS_POINTER_ROOT) => root,
            _ => self.keyboard_fallback(focused),
        };
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
            &mut |window| Ok(self.selects(window, key_mask).then_some(true)),
            &|window| {
                self.windows
                    .get(&window)
                    .is_some_and(|selection| selection.do_not_propagate_mask & key_mask != 0)
            },
            // No second try at the focus. This path's delivery path already
            // contains the focus, so a retry can only fire when the walk was
            // stopped early by a do-not-propagate mask -- and offering the
            // event to the focus anyway is exactly what that mask forbids.
            false,
        );
        match found {
            Ok(crate::key_routing::XKeyTarget::Found { window, .. }) => {
                XKeyDelivery::Window(window)
            }
            Ok(crate::key_routing::XKeyTarget::Blocked) => {
                XKeyDelivery::Discard(XKeyDiscard::DoNotPropagate)
            }
            Ok(crate::key_routing::XKeyTarget::Unselected) | Err(()) => {
                XKeyDelivery::Unselected
            }
        }
    }

    /// The window that hears this pointer event, or None when nothing on the
    /// path from the window under the pointer up to the surface selected it.
    /// `state` is the event's core state field, whose held-button bits decide
    /// which motion masks apply; buttons ignore it.
    fn selected_pointer_target(
        &self,
        surface_window: XResourceId,
        selection: XPointerSelection,
        state: u16,
        event_x: i16,
        event_y: i16,
    ) -> Option<XResourceId> {
        // A button is selected by direction, as a key is: one combined mask
        // here delivered presses to windows that had selected releases alone
        // (XTS Xlib11 ButtonPress 4 and 6).
        let selected_mask = match selection {
            XPointerSelection::Motion => Self::motion_selection_mask(state),
            XPointerSelection::Press => Self::BUTTON_PRESS_MASK,
            XPointerSelection::Release => Self::BUTTON_RELEASE_MASK,
        };
        // The walk goes on past the surface window to the root: a client's
        // selection on the root, or on a parent it reparented its toplevel
        // under, is as good as one on the toplevel (XTS Xlib11 ButtonPress
        // 7). What stops it is a do-not-propagate mask, or a selector.
        let event_window = self.pointer_event_target(surface_window, event_x, event_y);
        for candidate in self.ancestry_including(event_window) {
            let selection = self.windows.get(&candidate).copied().unwrap_or_default();
            if selection.mask & selected_mask != 0 {
                return Some(candidate);
            }
            if selection.do_not_propagate_mask & selected_mask != 0 {
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

    /// Where the pointer was last observed, in root coordinates.
    pub(crate) fn pointer_position(&self) -> Option<(i16, i16)> {
        self.pointer.map(|pointer| (pointer.root_x, pointer.root_y))
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

    /// Whether this connection selected KeymapState on the window: the
    /// KeymapNotify after an EnterNotify or FocusIn on it is owed to it.
    pub(crate) fn keymap_state_selected(&self, window: XResourceId) -> bool {
        self.windows
            .get(&window)
            .is_some_and(|selection| selection.mask & Self::KEYMAP_STATE_MASK != 0)
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

    /// Whether this table still knows the window: registered with a parent
    /// or selected on, and not removed since.
    pub(crate) fn knows_window(&self, window: XResourceId) -> bool {
        self.parents.contains_key(&window) || self.windows.contains_key(&window)
    }

    /// Whether a surface-relative point lies inside the surface window's
    /// own extent; a surface whose geometry is unknown is taken to contain
    /// it. Outside it the pointer is in the root (or another client's
    /// window, which this table cannot see), and the crossing events say
    /// so.
    pub(crate) fn surface_contains(&self, surface_window: XResourceId, event_x: i16, event_y: i16) -> bool {
        self.geometries.get(&surface_window).is_none_or(|geometry| {
            event_x >= 0
                && event_y >= 0
                && i32::from(event_x) < geometry.width
                && i32::from(event_y) < geometry.height
        })
    }

    /// The EnterNotify and LeaveNotify events a pointer move from `from` to
    /// `to` generates, in the protocol's order: the leaves from the window
    /// left up to the common ancestor, then the enters down from it, each
    /// with its detail and the child on the way to the pointer.
    pub(crate) fn pointer_crossings(&self, from: XResourceId, to: XResourceId) -> Vec<XPointerCrossingStep> {
        x11_pointer_crossings(&self.ancestry_including(from), &self.ancestry_including(to))
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

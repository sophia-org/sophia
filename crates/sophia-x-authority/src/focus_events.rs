//! FocusIn and FocusOut generation, as the X11 protocol states it.
//!
//! The protocol defines focus events as a procedure over the window chain
//! rather than as a pair of notifications to the windows losing and gaining
//! focus. Every window between the old focus and the new one is told what
//! happened, in a defined order, with a detail that says how it was involved.
//! Getting this wrong is invisible to a client that only watches the two
//! windows it asked about and immediately visible to a toolkit that tracks
//! the chain, which is why it is written here as one pure function over an
//! ancestry oracle rather than folded into the dispatch path.
//!
//! This server has one screen and one root, so the protocol's cross-screen
//! cases cannot arise and are not written. Grabs are not implemented, so the
//! `Grab`, `Ungrab` and `WhileGrabbed` modes are expressible here but never
//! produced; the caller names the mode.

use crate::XResourceId;

/// The keyboard is not grabbed.
pub const X_FOCUS_MODE_NORMAL: u8 = 0;
/// A keyboard grab is activating.
pub const X_FOCUS_MODE_GRAB: u8 = 1;
/// A keyboard grab is deactivating.
pub const X_FOCUS_MODE_UNGRAB: u8 = 2;
/// A focus request arriving while the keyboard is grabbed.
pub const X_FOCUS_MODE_WHILE_GRABBED: u8 = 3;

/// The window is an ancestor of the other end of the transition.
pub const X_FOCUS_DETAIL_ANCESTOR: u8 = 0;
/// The window lies on the path between the two ends and is not either of them.
pub const X_FOCUS_DETAIL_VIRTUAL: u8 = 1;
/// The window is an inferior of the other end of the transition.
pub const X_FOCUS_DETAIL_INFERIOR: u8 = 2;
/// Neither end is an ancestor of the other; this window is one of the ends.
pub const X_FOCUS_DETAIL_NONLINEAR: u8 = 3;
/// Neither end is an ancestor of the other; this window is on the path.
pub const X_FOCUS_DETAIL_NONLINEAR_VIRTUAL: u8 = 4;
/// The window is only involved because the pointer is in it or under it.
pub const X_FOCUS_DETAIL_POINTER: u8 = 5;
/// Reported on the root when the focus becomes or stops being `PointerRoot`.
pub const X_FOCUS_DETAIL_POINTER_ROOT: u8 = 6;
/// Reported on the root when the focus becomes or stops being `None`.
pub const X_FOCUS_DETAIL_NONE: u8 = 7;

/// What the keyboard focus is set to, which is not always a window.
///
/// `None` discards keyboard events until a focus is set again. `PointerRoot`
/// resolves at each keyboard event to the root of the screen the pointer is
/// on. Neither is a resource, and the protocol treats transitions to and from
/// them quite differently from transitions between two windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XFocusTarget {
    None,
    PointerRoot,
    Window(XResourceId),
}

impl XFocusTarget {
    /// Reads the focus field of a request or of stored focus state.
    #[must_use]
    pub fn from_resource(focus: XResourceId) -> Self {
        match u32::try_from(focus.local.raw()) {
            Ok(crate::X_FOCUS_NONE) => Self::None,
            Ok(crate::X_FOCUS_POINTER_ROOT) => Self::PointerRoot,
            _ => Self::Window(focus),
        }
    }

    /// The value this focus is stored and reported as.
    #[must_use]
    pub fn to_resource(self) -> XResourceId {
        match self {
            Self::None => XResourceId::new(u64::from(crate::X_FOCUS_NONE), 1),
            Self::PointerRoot => XResourceId::new(u64::from(crate::X_FOCUS_POINTER_ROOT), 1),
            Self::Window(window) => window,
        }
    }

    /// The detail a root window carries when the focus becomes or leaves this.
    fn root_detail(self) -> Option<u8> {
        match self {
            Self::None => Some(X_FOCUS_DETAIL_NONE),
            Self::PointerRoot => Some(X_FOCUS_DETAIL_POINTER_ROOT),
            Self::Window(_) => None,
        }
    }
}

/// One FocusIn or FocusOut owed to one window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XFocusTransitionEvent {
    pub window: XResourceId,
    /// True for FocusIn, false for FocusOut.
    pub focused: bool,
    pub detail: u8,
}

impl XFocusTransitionEvent {
    fn out(window: XResourceId, detail: u8) -> Self {
        Self {
            window,
            focused: false,
            detail,
        }
    }
    fn into_(window: XResourceId, detail: u8) -> Self {
        Self {
            window,
            focused: true,
            detail,
        }
    }
}

/// The window chains a focus transition is computed over.
///
/// `ancestry` returns the chain from the root down to the window inclusive,
/// root first. A window the caller knows nothing about yields just itself,
/// which makes it a child of nothing and so nonlinear with everything; that
/// is the safe reading, since the alternative is to invent a relationship.
pub struct XFocusChains<'a> {
    pub root: XResourceId,
    /// The window the pointer is in. The root when it is in no other window.
    pub pointer: XResourceId,
    pub ancestry: &'a dyn Fn(XResourceId) -> Vec<XResourceId>,
}

impl XFocusChains<'_> {
    fn chain(&self, window: XResourceId) -> Vec<XResourceId> {
        let chain = (self.ancestry)(window);
        if chain.last() == Some(&window) {
            chain
        } else {
            vec![window]
        }
    }

    /// Whether `inferior` is strictly below `ancestor`.
    fn is_inferior(&self, inferior: XResourceId, ancestor: XResourceId) -> bool {
        inferior != ancestor && self.chain(inferior).contains(&ancestor)
    }
}

/// Every FocusIn and FocusOut a change from `old` to `new` owes, in order.
///
/// The caller supplies the mode and delivers only to windows that selected
/// focus events; this decides who is owed what, which is the part the
/// protocol specifies and the part that cannot be guessed from the two ends.
#[must_use]
pub fn x_focus_transition_events(
    old: XFocusTarget,
    new: XFocusTarget,
    chains: &XFocusChains<'_>,
) -> Vec<XFocusTransitionEvent> {
    if old == new {
        return Vec::new();
    }
    match (old, new) {
        (XFocusTarget::Window(a), XFocusTarget::Window(b)) => between_windows(a, b, chains),
        (XFocusTarget::Window(a), _) => leaving_windows(a, new, chains),
        (_, XFocusTarget::Window(b)) => entering_windows(old, b, chains),
        // PointerRoot to None or back: only the root is involved, plus the
        // pointer chain on whichever side is PointerRoot.
        _ => {
            let mut events = Vec::new();
            pointer_chain_out(old, chains, &mut events);
            if let Some(detail) = old.root_detail() {
                events.push(XFocusTransitionEvent::out(chains.root, detail));
            }
            if let Some(detail) = new.root_detail() {
                events.push(XFocusTransitionEvent::into_(chains.root, detail));
            }
            pointer_chain_in(new, chains, &mut events);
            events
        }
    }
}

/// Focus moving between two real windows: the three shapes the protocol names.
fn between_windows(
    a: XResourceId,
    b: XResourceId,
    chains: &XFocusChains<'_>,
) -> Vec<XFocusTransitionEvent> {
    let mut events = Vec::new();
    let pointer = chains.pointer;
    if chains.is_inferior(a, b) {
        // A is below B: the focus rose out of A up to B.
        events.push(XFocusTransitionEvent::out(a, X_FOCUS_DETAIL_ANCESTOR));
        for window in ascending_between(a, b, chains) {
            events.push(XFocusTransitionEvent::out(window, X_FOCUS_DETAIL_VIRTUAL));
        }
        events.push(XFocusTransitionEvent::into_(b, X_FOCUS_DETAIL_INFERIOR));
        if chains.is_inferior(pointer, b)
            && pointer != a
            && !chains.is_inferior(pointer, a)
            && !chains.is_inferior(a, pointer)
        {
            for window in descending_below(b, pointer, chains) {
                events.push(XFocusTransitionEvent::into_(window, X_FOCUS_DETAIL_POINTER));
            }
        }
    } else if chains.is_inferior(b, a) {
        // B is below A: the focus descended from A into B.
        if chains.is_inferior(pointer, a)
            && !chains.is_inferior(pointer, b)
            && !chains.is_inferior(b, pointer)
            && pointer != b
        {
            for window in ascending_upto(pointer, a, chains) {
                events.push(XFocusTransitionEvent::out(window, X_FOCUS_DETAIL_POINTER));
            }
        }
        events.push(XFocusTransitionEvent::out(a, X_FOCUS_DETAIL_INFERIOR));
        for window in descending_between(a, b, chains) {
            events.push(XFocusTransitionEvent::into_(window, X_FOCUS_DETAIL_VIRTUAL));
        }
        events.push(XFocusTransitionEvent::into_(b, X_FOCUS_DETAIL_ANCESTOR));
    } else {
        // Neither contains the other, so the chains meet at a common ancestor
        // and both ends are nonlinear with respect to it.
        if chains.is_inferior(pointer, a) {
            for window in ascending_upto(pointer, a, chains) {
                events.push(XFocusTransitionEvent::out(window, X_FOCUS_DETAIL_POINTER));
            }
        }
        events.push(XFocusTransitionEvent::out(a, X_FOCUS_DETAIL_NONLINEAR));
        let common = least_common_ancestor(a, b, chains);
        for window in ascending_to_boundary(a, common, chains) {
            events.push(XFocusTransitionEvent::out(
                window,
                X_FOCUS_DETAIL_NONLINEAR_VIRTUAL,
            ));
        }
        for window in descending_from_boundary(common, b, chains) {
            events.push(XFocusTransitionEvent::into_(
                window,
                X_FOCUS_DETAIL_NONLINEAR_VIRTUAL,
            ));
        }
        events.push(XFocusTransitionEvent::into_(b, X_FOCUS_DETAIL_NONLINEAR));
        if chains.is_inferior(pointer, b) {
            for window in descending_below(b, pointer, chains) {
                events.push(XFocusTransitionEvent::into_(window, X_FOCUS_DETAIL_POINTER));
            }
        }
    }
    events
}

/// Focus leaving a window for `None` or `PointerRoot`.
fn leaving_windows(
    a: XResourceId,
    new: XFocusTarget,
    chains: &XFocusChains<'_>,
) -> Vec<XFocusTransitionEvent> {
    let mut events = Vec::new();
    if chains.is_inferior(chains.pointer, a) {
        for window in ascending_upto(chains.pointer, a, chains) {
            events.push(XFocusTransitionEvent::out(window, X_FOCUS_DETAIL_POINTER));
        }
    }
    events.push(XFocusTransitionEvent::out(a, X_FOCUS_DETAIL_NONLINEAR));
    if a != chains.root {
        for window in ascending_through_root(a, chains) {
            events.push(XFocusTransitionEvent::out(
                window,
                X_FOCUS_DETAIL_NONLINEAR_VIRTUAL,
            ));
        }
    }
    if let Some(detail) = new.root_detail() {
        events.push(XFocusTransitionEvent::into_(chains.root, detail));
    }
    pointer_chain_in(new, chains, &mut events);
    events
}

/// Focus arriving at a window from `None` or `PointerRoot`.
fn entering_windows(
    old: XFocusTarget,
    b: XResourceId,
    chains: &XFocusChains<'_>,
) -> Vec<XFocusTransitionEvent> {
    let mut events = Vec::new();
    pointer_chain_out(old, chains, &mut events);
    if let Some(detail) = old.root_detail() {
        events.push(XFocusTransitionEvent::out(chains.root, detail));
    }
    if b != chains.root {
        for window in descending_from_root(b, chains) {
            events.push(XFocusTransitionEvent::into_(
                window,
                X_FOCUS_DETAIL_NONLINEAR_VIRTUAL,
            ));
        }
    }
    events.push(XFocusTransitionEvent::into_(b, X_FOCUS_DETAIL_NONLINEAR));
    if chains.is_inferior(chains.pointer, b) {
        for window in descending_below(b, chains.pointer, chains) {
            events.push(XFocusTransitionEvent::into_(window, X_FOCUS_DETAIL_POINTER));
        }
    }
    events
}

/// Leaving `PointerRoot` tells the pointer's whole chain, up to and including
/// its root, because that chain was the focus until now.
fn pointer_chain_out(
    old: XFocusTarget,
    chains: &XFocusChains<'_>,
    events: &mut Vec<XFocusTransitionEvent>,
) {
    if old != XFocusTarget::PointerRoot {
        return;
    }
    for window in chains.chain(chains.pointer).into_iter().rev() {
        events.push(XFocusTransitionEvent::out(window, X_FOCUS_DETAIL_POINTER));
    }
}

/// Arriving at `PointerRoot` tells the pointer's whole chain, root first.
fn pointer_chain_in(
    new: XFocusTarget,
    chains: &XFocusChains<'_>,
    events: &mut Vec<XFocusTransitionEvent>,
) {
    if new != XFocusTarget::PointerRoot {
        return;
    }
    for window in chains.chain(chains.pointer) {
        events.push(XFocusTransitionEvent::into_(window, X_FOCUS_DETAIL_POINTER));
    }
}

/// Windows strictly between `low` and `high`, walking up from `low`.
fn ascending_between(
    low: XResourceId,
    high: XResourceId,
    chains: &XFocusChains<'_>,
) -> Vec<XResourceId> {
    let chain = chains.chain(low);
    chain
        .into_iter()
        .rev()
        .skip(1)
        .take_while(|window| *window != high)
        .collect()
}

/// Windows strictly between `high` and `low`, walking down from `high`.
fn descending_between(
    high: XResourceId,
    low: XResourceId,
    chains: &XFocusChains<'_>,
) -> Vec<XResourceId> {
    let mut between = ascending_between(low, high, chains);
    between.reverse();
    between
}

/// From `from` up to but not including `stop`.
fn ascending_upto(
    from: XResourceId,
    stop: XResourceId,
    chains: &XFocusChains<'_>,
) -> Vec<XResourceId> {
    chains
        .chain(from)
        .into_iter()
        .rev()
        .take_while(|window| *window != stop)
        .collect()
}

/// Every window below `top` on the path to `bottom`, ending at `bottom`.
fn descending_below(
    top: XResourceId,
    bottom: XResourceId,
    chains: &XFocusChains<'_>,
) -> Vec<XResourceId> {
    let chain = chains.chain(bottom);
    let Some(index) = chain.iter().position(|window| *window == top) else {
        return vec![bottom];
    };
    chain[index + 1..].to_vec()
}

/// From just above `from` up to but not including `boundary`.
fn ascending_to_boundary(
    from: XResourceId,
    boundary: Option<XResourceId>,
    chains: &XFocusChains<'_>,
) -> Vec<XResourceId> {
    let chain = chains.chain(from);
    chain
        .into_iter()
        .rev()
        .skip(1)
        .take_while(|window| Some(*window) != boundary)
        .collect()
}

/// From just below `boundary` down to but not including `to`.
fn descending_from_boundary(
    boundary: Option<XResourceId>,
    to: XResourceId,
    chains: &XFocusChains<'_>,
) -> Vec<XResourceId> {
    let mut between = ascending_to_boundary(to, boundary, chains);
    between.reverse();
    between
}

/// Every window above `from`, ending at its root.
fn ascending_through_root(from: XResourceId, chains: &XFocusChains<'_>) -> Vec<XResourceId> {
    chains.chain(from).into_iter().rev().skip(1).collect()
}

/// From `to`'s root down to but not including `to`.
fn descending_from_root(to: XResourceId, chains: &XFocusChains<'_>) -> Vec<XResourceId> {
    let mut chain = chains.chain(to);
    chain.pop();
    chain
}

/// The deepest window that is an ancestor of both, if they share one.
fn least_common_ancestor(
    a: XResourceId,
    b: XResourceId,
    chains: &XFocusChains<'_>,
) -> Option<XResourceId> {
    let left = chains.chain(a);
    let right = chains.chain(b);
    left.iter()
        .zip(right.iter())
        .take_while(|(one, other)| one == other)
        .map(|(one, _)| *one)
        .last()
}

/// Where the focus goes when its window stops being viewable.
///
/// The protocol gives three answers and they are not symmetric. `Parent`
/// reverts to the parent, or to the closest viewable ancestor when the parent
/// is not viewable either, and the revert_to itself becomes `None` so a second
/// unviewability does not walk the tree again. `PointerRoot` and `None` revert
/// to exactly that value and keep their revert_to, since there is nothing left
/// to walk. The last-focus-change time is deliberately not touched: reverting
/// is not the client naming a moment, and a request that was honest when it
/// was sent should still land afterwards.
#[must_use]
pub fn x_focus_reversion(
    revert_to: u8,
    focus: XResourceId,
    chains: &XFocusChains<'_>,
    is_viewable: &dyn Fn(XResourceId) -> bool,
) -> (XFocusTarget, u8) {
    match revert_to {
        crate::X_REVERT_TO_PARENT => {
            let chain = chains.chain(focus);
            let ancestor = chain
                .iter()
                .rev()
                .skip(1)
                .find(|window| is_viewable(**window))
                .copied();
            match ancestor {
                Some(window) => (XFocusTarget::Window(window), crate::X_REVERT_TO_NONE),
                // Nothing above it is viewable either, which can only happen
                // when the root itself is not reported viewable. Releasing the
                // focus entirely is the honest answer; inventing a window here
                // would focus something no less unviewable.
                None => (XFocusTarget::None, crate::X_REVERT_TO_NONE),
            }
        }
        crate::X_REVERT_TO_POINTER_ROOT => {
            (XFocusTarget::PointerRoot, crate::X_REVERT_TO_POINTER_ROOT)
        }
        _ => (XFocusTarget::None, crate::X_REVERT_TO_NONE),
    }
}

mod tests;

//! Which window a keyboard event is reported with respect to.
//!
//! The protocol states it as: the source window is the window the pointer is
//! in; the event window is found by walking up from the source to the first
//! window on which a client selected the event, unless an intervening window
//! forbids it through its do-not-propagate mask; and for keyboard events that
//! is modified by the focus window.
//!
//! Written here as one function over a path and two oracles, rather than
//! inside either delivery path, because the server has two of those and they
//! disagreed. A rule that lives in one of them is a rule the other can drift
//! from without anyone noticing, which is how the two came to implement
//! different protocols.

use crate::XResourceId;

/// The window a key is delivered to, and whether by core rather than XI2.
///
/// `delivery_path` runs deepest first: it is the pointer's own chain
/// truncated at the focus when the pointer is inside the focus subtree, and
/// the focus window when it is not. Deepest first is delivery order, which is
/// the opposite of the root-first convention
/// [`crate::x_focus_transition_events`] uses; the two are ordered the way
/// their own protocol text reads.
///
/// **Where the walk stops is the path's business, not this function's.** The
/// two callers differ there and the difference is real: the private path ends
/// its path at the focus, so propagation never climbs above it, while the
/// ordinary path continues to the focus's ancestors as Xorg does. Expressing
/// that through the path rather than a flag keeps one rule with one meaning,
/// and makes each caller's ceiling visible where it is chosen.
///
/// `selects` answers whether a window wants the event, `Some(false)` meaning
/// XI2 selected it and `Some(true)` core. It is fallible so a caller that
/// meters its own traversal can stop the walk with its own refusal rather
/// than having exhaustion read as disinterest.
///
/// [`XKeyTarget::Unselected`] means nobody on the path wanted it. That is not
/// an error: a key nobody selected is a key nobody is owed.
/// [`XKeyTarget::Blocked`] means a do-not-propagate mask ended the walk with
/// nobody found, which is a different instruction to the caller -- the client
/// asked for the event not to be offered further, so a caller that would
/// otherwise wait and then fall back must not.
pub(crate) fn x_key_delivery_target<E>(
    focus: XResourceId,
    delivery_path: &[XResourceId],
    selects: &mut dyn FnMut(XResourceId) -> Result<Option<bool>, E>,
    do_not_propagate: &dyn Fn(XResourceId) -> bool,
    retry_focus: bool,
) -> Result<XKeyTarget, E> {
    let mut blocked = false;
    for window in delivery_path.iter().copied() {
        if let Some(core) = selects(window)? {
            return Ok(XKeyTarget::Found { window, core });
        }
        if do_not_propagate(window) {
            blocked = true;
            break;
        }
    }
    // Nothing under the focus wanted it, so the focus itself is offered the
    // event it would have had if the pointer had been elsewhere. Skipped when
    // the walk began at the focus, because it was already asked.
    //
    // This runs after a block as well, and deliberately: the private path
    // documents core do-not-propagate as stopping both streams and then
    // trying the focus directly, and its controls pin that. The mask ends the
    // walk up the tree; it does not withdraw the focus's own claim.
    if retry_focus
        && delivery_path.first() != Some(&focus)
        && let Some(core) = selects(focus)?
    {
        return Ok(XKeyTarget::Found {
            window: focus,
            core,
        });
    }
    if blocked {
        Ok(XKeyTarget::Blocked)
    } else {
        Ok(XKeyTarget::Unselected)
    }
}

/// What the walk found, or why it found nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XKeyTarget {
    /// Report the key on this window, by core rather than XI2 when `core`.
    Found { window: XResourceId, core: bool },
    /// A do-not-propagate mask ended the walk and nobody wanted the event.
    Blocked,
    /// The path ran out and nobody wanted the event.
    Unselected,
}

mod tests;

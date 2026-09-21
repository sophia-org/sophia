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
/// `delivery_path` runs deepest first and ends at the focus window: it is the
/// pointer's own chain truncated at the focus when the pointer is inside the
/// focus subtree, and just the focus window when it is not. Deepest first is
/// delivery order, which is the opposite of the root-first convention
/// [`crate::x_focus_transition_events`] uses; the two are ordered the way
/// their own protocol text reads.
///
/// `selects` answers whether a window wants the event, `Some(false)` meaning
/// XI2 selected it and `Some(true)` core. It is fallible so a caller that
/// meters its own traversal can stop the walk with its own refusal rather
/// than having exhaustion read as disinterest.
///
/// `Ok(None)` means nobody on the path wanted it. That is not an error: a key
/// nobody selected is a key nobody is owed.
pub(crate) fn x_key_delivery_target<E>(
    focus: XResourceId,
    delivery_path: &[XResourceId],
    selects: &mut dyn FnMut(XResourceId) -> Result<Option<bool>, E>,
    do_not_propagate: &dyn Fn(XResourceId) -> bool,
    retry_focus: bool,
) -> Result<Option<(XResourceId, bool)>, E> {
    for window in delivery_path.iter().copied() {
        if let Some(core) = selects(window)? {
            return Ok(Some((window, core)));
        }
        // The focus is the ceiling: propagation inside the subtree stops
        // there rather than continuing to its ancestors.
        if window == focus || do_not_propagate(window) {
            break;
        }
    }
    // Nothing under the focus wanted it, so the focus itself is offered the
    // event it would have had if the pointer had been elsewhere. Skipped when
    // the walk began at the focus, because it was already asked.
    if retry_focus
        && delivery_path.first() != Some(&focus)
        && let Some(core) = selects(focus)?
    {
        return Ok(Some((focus, core)));
    }
    Ok(None)
}

mod tests;

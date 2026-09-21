#![cfg(test)]

use super::*;
use crate::{X_REVERT_TO_NONE, X_REVERT_TO_PARENT, X_REVERT_TO_POINTER_ROOT};

/// A small window tree, root first:
///
/// ```text
/// root
///  +- base            (0x10)
///  |   +- left        (0x11)
///  |   |   +- leftleaf(0x12)
///  |   +- right       (0x13)
///  +- other           (0x20)
/// ```
fn window(raw: u64) -> XResourceId {
    XResourceId::new(raw, 1)
}
fn root() -> XResourceId {
    window(0x20_0000)
}
fn tree(target: XResourceId) -> Vec<XResourceId> {
    let mut chain = vec![root()];
    match target.local.raw() {
        0x10 => chain.push(window(0x10)),
        0x11 => chain.extend([window(0x10), window(0x11)]),
        0x12 => chain.extend([window(0x10), window(0x11), window(0x12)]),
        0x13 => chain.extend([window(0x10), window(0x13)]),
        0x20 => chain.push(window(0x20)),
        0x20_0000 => {}
        _ => return vec![target],
    }
    chain
}

fn chains(pointer: XResourceId) -> XFocusChains<'static> {
    XFocusChains {
        root: root(),
        pointer,
        ancestry: &tree,
    }
}

/// Renders the events as `(raw window, focused, detail)` so a table reads.
fn shape(events: &[XFocusTransitionEvent]) -> Vec<(u64, bool, u8)> {
    events
        .iter()
        .map(|event| (event.window.local.raw(), event.focused, event.detail))
        .collect()
}

#[test]
fn a_focus_that_does_not_move_owes_nothing() {
    for target in [
        XFocusTarget::None,
        XFocusTarget::PointerRoot,
        XFocusTarget::Window(window(0x11)),
    ] {
        assert!(x_focus_transition_events(target, target, &chains(root())).is_empty());
    }
}

#[test]
fn focus_rising_out_of_a_child_tells_the_whole_path_between() {
    let events = x_focus_transition_events(
        XFocusTarget::Window(window(0x12)),
        XFocusTarget::Window(window(0x10)),
        &chains(root()),
    );
    assert_eq!(
        vec![
            (0x12, false, X_FOCUS_DETAIL_ANCESTOR),
            // left lies between the two ends and is told so.
            (0x11, false, X_FOCUS_DETAIL_VIRTUAL),
            (0x10, true, X_FOCUS_DETAIL_INFERIOR),
        ],
        shape(&events)
    );
}

#[test]
fn focus_descending_into_a_child_walks_the_same_path_the_other_way() {
    let events = x_focus_transition_events(
        XFocusTarget::Window(window(0x10)),
        XFocusTarget::Window(window(0x12)),
        &chains(root()),
    );
    assert_eq!(
        vec![
            (0x10, false, X_FOCUS_DETAIL_INFERIOR),
            (0x11, true, X_FOCUS_DETAIL_VIRTUAL),
            (0x12, true, X_FOCUS_DETAIL_ANCESTOR),
        ],
        shape(&events)
    );
}

#[test]
fn a_move_between_siblings_is_nonlinear_through_their_common_ancestor() {
    // This is the shape the conformance suite's purpose 10 exercises, and the
    // one the old hardcoded detail of 3 happened to get right, so it is the
    // regression to watch.
    let events = x_focus_transition_events(
        XFocusTarget::Window(window(0x11)),
        XFocusTarget::Window(window(0x13)),
        &chains(root()),
    );
    assert_eq!(
        vec![
            (0x11, false, X_FOCUS_DETAIL_NONLINEAR),
            (0x13, true, X_FOCUS_DETAIL_NONLINEAR),
        ],
        shape(&events)
    );
}

#[test]
fn a_deeper_nonlinear_move_names_every_window_on_both_legs() {
    let events = x_focus_transition_events(
        XFocusTarget::Window(window(0x12)),
        XFocusTarget::Window(window(0x20)),
        &chains(root()),
    );
    assert_eq!(
        vec![
            (0x12, false, X_FOCUS_DETAIL_NONLINEAR),
            (0x11, false, X_FOCUS_DETAIL_NONLINEAR_VIRTUAL),
            (0x10, false, X_FOCUS_DETAIL_NONLINEAR_VIRTUAL),
            (0x20, true, X_FOCUS_DETAIL_NONLINEAR),
        ],
        shape(&events)
    );
}

#[test]
fn the_pointer_chain_is_told_when_the_focus_crosses_off_it() {
    // The pointer sits in leftleaf, under base and not under other. The focus
    // used to cover the pointer and no longer does, so everything from the
    // pointer up to but not including base hears about it.
    let events = x_focus_transition_events(
        XFocusTarget::Window(window(0x10)),
        XFocusTarget::Window(window(0x20)),
        &chains(window(0x12)),
    );
    assert_eq!(
        vec![
            (0x12, false, X_FOCUS_DETAIL_POINTER),
            (0x11, false, X_FOCUS_DETAIL_POINTER),
            (0x10, false, X_FOCUS_DETAIL_NONLINEAR),
            (0x20, true, X_FOCUS_DETAIL_NONLINEAR),
        ],
        shape(&events)
    );
}

#[test]
fn focus_arriving_over_the_pointer_tells_the_chain_below_it() {
    let events = x_focus_transition_events(
        XFocusTarget::Window(window(0x20)),
        XFocusTarget::Window(window(0x10)),
        &chains(window(0x12)),
    );
    assert_eq!(
        vec![
            (0x20, false, X_FOCUS_DETAIL_NONLINEAR),
            (0x10, true, X_FOCUS_DETAIL_NONLINEAR),
            (0x11, true, X_FOCUS_DETAIL_POINTER),
            (0x12, true, X_FOCUS_DETAIL_POINTER),
        ],
        shape(&events)
    );
}

#[test]
fn leaving_a_window_for_none_walks_up_to_the_root_and_then_says_none() {
    let events = x_focus_transition_events(
        XFocusTarget::Window(window(0x11)),
        XFocusTarget::None,
        &chains(root()),
    );
    assert_eq!(
        vec![
            (0x11, false, X_FOCUS_DETAIL_NONLINEAR),
            (0x10, false, X_FOCUS_DETAIL_NONLINEAR_VIRTUAL),
            (0x20_0000, false, X_FOCUS_DETAIL_NONLINEAR_VIRTUAL),
            (0x20_0000, true, X_FOCUS_DETAIL_NONE),
        ],
        shape(&events)
    );
}

#[test]
fn arriving_at_pointer_root_tells_the_pointers_chain_root_first() {
    let events = x_focus_transition_events(
        XFocusTarget::None,
        XFocusTarget::PointerRoot,
        &chains(window(0x11)),
    );
    assert_eq!(
        vec![
            (0x20_0000, false, X_FOCUS_DETAIL_NONE),
            (0x20_0000, true, X_FOCUS_DETAIL_POINTER_ROOT),
            // The pointer's chain becomes the focus, so it is told, root first.
            (0x20_0000, true, X_FOCUS_DETAIL_POINTER),
            (0x10, true, X_FOCUS_DETAIL_POINTER),
            (0x11, true, X_FOCUS_DETAIL_POINTER),
        ],
        shape(&events)
    );
}

#[test]
fn leaving_pointer_root_tells_the_pointers_chain_deepest_first() {
    let events = x_focus_transition_events(
        XFocusTarget::PointerRoot,
        XFocusTarget::None,
        &chains(window(0x11)),
    );
    assert_eq!(
        vec![
            (0x11, false, X_FOCUS_DETAIL_POINTER),
            (0x10, false, X_FOCUS_DETAIL_POINTER),
            (0x20_0000, false, X_FOCUS_DETAIL_POINTER),
            (0x20_0000, false, X_FOCUS_DETAIL_POINTER_ROOT),
            (0x20_0000, true, X_FOCUS_DETAIL_NONE),
        ],
        shape(&events)
    );
}

#[test]
fn entering_a_window_from_pointer_root_descends_from_the_root_to_it() {
    let events = x_focus_transition_events(
        XFocusTarget::PointerRoot,
        XFocusTarget::Window(window(0x13)),
        &chains(root()),
    );
    assert_eq!(
        vec![
            (0x20_0000, false, X_FOCUS_DETAIL_POINTER),
            (0x20_0000, false, X_FOCUS_DETAIL_POINTER_ROOT),
            (0x20_0000, true, X_FOCUS_DETAIL_NONLINEAR_VIRTUAL),
            (0x10, true, X_FOCUS_DETAIL_NONLINEAR_VIRTUAL),
            (0x13, true, X_FOCUS_DETAIL_NONLINEAR),
        ],
        shape(&events)
    );
}

#[test]
fn reverting_to_parent_finds_the_closest_viewable_ancestor_and_forgets_itself() {
    let all_viewable = |_: XResourceId| true;
    assert_eq!(
        (XFocusTarget::Window(window(0x11)), X_REVERT_TO_NONE),
        x_focus_reversion(
            X_REVERT_TO_PARENT,
            window(0x12),
            &chains(root()),
            &all_viewable
        ),
        "the parent is viewable, so the focus stops there and revert_to becomes None"
    );

    // The parent went away with the child, which is what an unmapped subtree
    // looks like. The search keeps climbing rather than focusing something
    // just as unviewable.
    let only_base_and_root =
        |candidate: XResourceId| matches!(candidate.local.raw(), 0x10 | 0x20_0000);
    assert_eq!(
        (XFocusTarget::Window(window(0x10)), X_REVERT_TO_NONE),
        x_focus_reversion(
            X_REVERT_TO_PARENT,
            window(0x12),
            &chains(root()),
            &only_base_and_root
        )
    );
}

#[test]
fn reverting_to_pointer_root_or_none_keeps_its_own_answer() {
    let all_viewable = |_: XResourceId| true;
    assert_eq!(
        (XFocusTarget::PointerRoot, X_REVERT_TO_POINTER_ROOT),
        x_focus_reversion(
            X_REVERT_TO_POINTER_ROOT,
            window(0x12),
            &chains(root()),
            &all_viewable
        ),
        "PointerRoot has nothing to walk, so it keeps its revert_to too"
    );
    assert_eq!(
        (XFocusTarget::None, X_REVERT_TO_NONE),
        x_focus_reversion(
            X_REVERT_TO_NONE,
            window(0x12),
            &chains(root()),
            &all_viewable
        )
    );
}

#[test]
fn a_window_the_server_knows_nothing_about_is_nonlinear_with_everything() {
    // Inventing an ancestry for an unknown window would invent a relationship
    // and with it the wrong details. Standing alone is the honest reading.
    let stranger = window(0x9999);
    let events = x_focus_transition_events(
        XFocusTarget::Window(window(0x11)),
        XFocusTarget::Window(stranger),
        &chains(root()),
    );
    assert_eq!(
        vec![
            (0x11, false, X_FOCUS_DETAIL_NONLINEAR),
            (0x10, false, X_FOCUS_DETAIL_NONLINEAR_VIRTUAL),
            (0x20_0000, false, X_FOCUS_DETAIL_NONLINEAR_VIRTUAL),
            (0x9999, true, X_FOCUS_DETAIL_NONLINEAR),
        ],
        shape(&events)
    );
}

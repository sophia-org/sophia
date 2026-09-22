#![cfg(test)]

use super::*;

/// The tree the conformance suite builds for its focus-delivery assertion:
///
/// ```text
/// root
///  +- parent
///      +- child1
///      +- child2        <- the focus
///          +- grandchild
/// ```
fn window(raw: u64) -> XResourceId {
    XResourceId::new(raw, 1)
}
fn root() -> XResourceId {
    window(0x20)
}
fn parent() -> XResourceId {
    window(0x10)
}
fn child1() -> XResourceId {
    window(0x11)
}
fn child2() -> XResourceId {
    window(0x12)
}
fn grandchild() -> XResourceId {
    window(0x13)
}

/// Deepest first, ending at the focus when the pointer is under it, and just
/// the focus when it is not. This is the truncation a caller performs before
/// calling, written out here so each case says which shape it is.
fn path_from(pointer: XResourceId) -> Vec<XResourceId> {
    match pointer.local.raw() {
        0x13 => vec![grandchild(), child2()],
        0x12 => vec![child2()],
        // root, parent and child1 are all outside the focus subtree.
        _ => vec![child2()],
    }
}

fn everything_selects(_: XResourceId) -> Result<Option<bool>, ()> {
    Ok(Some(true))
}
fn nothing_propagates(_: XResourceId) -> bool {
    false
}

#[test]
fn a_key_lands_on_the_pointers_window_inside_the_focus_subtree_and_on_the_focus_outside_it() {
    // The suite warps the pointer into each of five windows in turn and
    // requires the event window to be the focus for the three outside its
    // subtree, and the pointer's own window for the two inside.
    for (pointer, expected) in [
        (root(), child2()),
        (parent(), child2()),
        (child1(), child2()),
        (child2(), child2()),
        (grandchild(), grandchild()),
    ] {
        let path = path_from(pointer);
        assert_eq!(
            Ok(XKeyTarget::Found {
                window: expected,
                core: true
            }),
            x_key_delivery_target(
                child2(),
                &path,
                &mut everything_selects,
                &nothing_propagates,
                true
            ),
            "pointer in {:#x}",
            pointer.local.raw()
        );
    }
}

#[test]
fn delivery_stops_at_the_focus_rather_than_climbing_past_it() {
    // Only a window above the focus wants the event, and the path ends at the
    // focus, so nobody gets it. The ceiling is the path's last element rather
    // than a rule inside the walk, which is how the two callers can differ.
    let selects = |window: XResourceId| Ok::<_, ()>(Some(true).filter(|_| window == parent()));
    assert_eq!(
        Ok(XKeyTarget::Unselected),
        x_key_delivery_target(
            child2(),
            &[grandchild(), child2()],
            &mut { selects },
            &nothing_propagates,
            true
        )
    );
}

#[test]
fn a_do_not_propagate_mask_below_the_focus_stops_the_walk() {
    // The grandchild forbids propagation, so the focus above it is never
    // offered the event even though it would have taken it.
    let selects = |window: XResourceId| Ok::<_, ()>(Some(true).filter(|_| window == child2()));
    assert_eq!(
        Ok(XKeyTarget::Blocked),
        x_key_delivery_target(
            child2(),
            &[grandchild(), child2()],
            &mut { selects },
            &|window| window == grandchild(),
            false
        ),
        "with no retry the blocked walk is the whole answer, and it is \
         reported as Blocked rather than Unselected: the caller must be able \
         to tell a mask it has to honour from nobody having selected yet"
    );
}

#[test]
fn the_focus_is_retried_once_when_nothing_on_the_path_wanted_the_event() {
    // The retry exists for the case where the pointer's branch declined and
    // the focus itself would have taken it. It must not fire when the walk
    // already began at the focus, or the focus is asked twice.
    let mut asked = Vec::new();
    let mut selects = |window: XResourceId| {
        asked.push(window);
        Ok::<_, ()>(None)
    };
    let _ = x_key_delivery_target(
        child2(),
        &[child2()],
        &mut selects,
        &nothing_propagates,
        true,
    );
    assert_eq!(vec![child2()], asked, "the focus is asked exactly once");

    let mut asked = Vec::new();
    let mut selects = |window: XResourceId| {
        asked.push(window);
        Ok::<_, ()>(None)
    };
    let _ = x_key_delivery_target(
        child2(),
        &[grandchild(), child2()],
        &mut selects,
        &nothing_propagates,
        true,
    );
    // The focus is asked twice here, because the retry is guarded on where
    // the walk began rather than on where it ended, and it began at the
    // grandchild. The second ask returns the same answer, so it costs a
    // repeated lookup and a unit of whatever the caller is metering, and
    // nothing else. Recorded rather than corrected: this function is an
    // extraction, and the behaviour is the one both paths must keep until a
    // change to it is measured on its own.
    assert_eq!(vec![grandchild(), child2(), child2()], asked);
}

#[test]
fn xi2_selection_is_reported_as_such() {
    let selects = |_: XResourceId| Ok::<_, ()>(Some(false));
    assert_eq!(
        Ok(XKeyTarget::Found {
            window: grandchild(),
            core: false
        }),
        x_key_delivery_target(
            child2(),
            &[grandchild(), child2()],
            &mut { selects },
            &nothing_propagates,
            true
        )
    );
}

#[test]
fn a_refusal_from_the_selection_oracle_stops_the_walk_rather_than_reading_as_disinterest() {
    // A caller that meters its own traversal must be able to end the walk
    // with its own refusal. Folding exhaustion into "nobody selected" would
    // turn running out of budget into a key nobody was owed.
    let selects = |_: XResourceId| Err("budget");
    assert_eq!(
        Err("budget"),
        x_key_delivery_target(
            child2(),
            &[grandchild(), child2()],
            &mut { selects },
            &nothing_propagates,
            true
        )
    );
}

#[test]
fn a_blocked_walk_and_an_empty_one_are_different_answers() {
    // The whole point of the third outcome. Both find nobody; only one of
    // them is a client instruction the caller must honour, and while they
    // were the same value the ordinary delivery path could not honour it.
    let nobody = |_: XResourceId| Ok::<_, ()>(None);
    assert_eq!(
        Ok(XKeyTarget::Unselected),
        x_key_delivery_target(
            child2(),
            &[grandchild(), child2()],
            &mut { nobody },
            &nothing_propagates,
            false
        ),
        "nobody selected it, and nobody forbade anything"
    );
    assert_eq!(
        Ok(XKeyTarget::Blocked),
        x_key_delivery_target(
            child2(),
            &[grandchild(), child2()],
            &mut { nobody },
            &|window| window == grandchild(),
            false
        ),
        "the same empty walk, ended by a mask"
    );
}

#[test]
fn a_block_still_lets_the_focus_be_retried_where_a_caller_asks_for_it() {
    // The private path documents core do-not-propagate as stopping both
    // streams and then trying the focus directly, and its controls pin that.
    // The mask ends the walk up the tree; it does not withdraw the focus's
    // own claim, so Blocked must not short-circuit the retry.
    let selects = |window: XResourceId| Ok::<_, ()>(Some(true).filter(|_| window == child2()));
    assert_eq!(
        Ok(XKeyTarget::Found {
            window: child2(),
            core: true
        }),
        x_key_delivery_target(
            child2(),
            &[grandchild()],
            &mut { selects },
            &|window| window == grandchild(),
            true
        )
    );
}

#![cfg(test)]
use super::*;

/// A stand-in handoff: the images it carries.
#[derive(Debug, Eq, PartialEq)]
struct Handoff(Vec<u32>);

#[test]
fn without_a_live_owner_the_held_handoff_is_kept() {
    // Zero-output Waiting: the loss retired the owner and its images live only
    // in this handoff, which the runtime still names. A terminal switch must
    // not drop it, or every later resume refuses for missing images.
    let mut held = Some(Handoff(vec![7, 9]));
    let custody = suspend_renderer_handoff_for_terminal_switch(
        &mut held,
        None::<fn() -> Result<Handoff, &'static str>>,
    );
    assert_eq!(custody, Ok(RendererHandoffCustody::Retained));
    assert_eq!(held, Some(Handoff(vec![7, 9])));

    let mut none: Option<Handoff> = None;
    let custody = suspend_renderer_handoff_for_terminal_switch(
        &mut none,
        None::<fn() -> Result<Handoff, &'static str>>,
    );
    assert_eq!(custody, Ok(RendererHandoffCustody::Retained));
    assert_eq!(none, None);
}

#[test]
fn a_live_owner_is_captured_as_before() {
    let mut held = None;
    let custody = suspend_renderer_handoff_for_terminal_switch(
        &mut held,
        Some(|| Ok::<_, &str>(Handoff(vec![3]))),
    );
    assert_eq!(custody, Ok(RendererHandoffCustody::Captured));
    assert_eq!(held, Some(Handoff(vec![3])));
}

#[test]
fn a_failed_capture_leaves_the_held_handoff_untouched() {
    let mut held = Some(Handoff(vec![5]));
    let custody = suspend_renderer_handoff_for_terminal_switch(
        &mut held,
        Some(|| Err::<Handoff, _>("export failed")),
    );
    assert_eq!(custody, Err("export failed"));
    assert_eq!(held, Some(Handoff(vec![5])));
}

#[test]
fn the_terminal_switch_path_settles_custody_through_this_helper() {
    let seat = include_str!("../../src/live_session/owner_loop/lifecycle/seat.rs");
    let requested = seat
        .split_once("Ok(report) => {")
        .expect("requested-switch settlement")
        .1
        .split_once("close_native_owner!(\"seat_release\"")
        .expect("requested-switch close")
        .0;
    assert!(requested.contains("suspend_renderer_handoff_for_terminal_switch("));
    assert!(requested.contains("custody.reduced_name()"));
    // The handoff is never overwritten wholesale on this path.
    assert!(!requested.contains("*suspended_renderer_images ="));
}

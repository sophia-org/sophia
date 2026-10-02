#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

//! The keyboard scope of an output is aggregated over its heads from each
//! head's own retired frame, without requiring the heads to agree on one
//! publication: shielding follows what every head actually shows.
use sophia_backend_live::presented_keyboard_scope;
use sophia_engine::{
    CompositorDisplayCommand, CompositorDisplayList, CompositorPresentationStamp, HeadlessOutput,
    OutputFrameDamageSnapshot, PresentedKeyboardScope, output_frame_damage_snapshot,
};
use sophia_protocol::Rect;

fn frame(stamp: Option<(u64, PresentedKeyboardScope)>) -> OutputFrameDamageSnapshot {
    let output = HeadlessOutput::deterministic();
    let commands = stamp
        .map(|(generation, keyboard)| {
            CompositorDisplayCommand::PresentationStamp(CompositorPresentationStamp {
                owner_epoch: 1,
                publication_generation: generation,
                output: output.id,
                output_generation: 1,
                coverage: Rect {
                    x: 0,
                    y: 0,
                    width: 100,
                    height: 100,
                },
                keyboard,
            })
        })
        .into_iter()
        .collect();
    output_frame_damage_snapshot(
        output,
        CompositorDisplayList {
            output: output.id,
            commands,
        },
        &[],
        None,
    )
    .unwrap()
}

/// A mirror whose heads retired two different Held generations still
/// presents Held: disagreement on the publication is not an unknown mode.
#[test]
fn two_held_heads_of_different_generations_present_held() {
    let (a, b) = (
        frame(Some((1, PresentedKeyboardScope::Held))),
        frame(Some((2, PresentedKeyboardScope::Held))),
    );
    assert_eq!(
        presented_keyboard_scope(&[Some(&a), Some(&b)]),
        PresentedKeyboardScope::Held
    );
}

/// A head with no retired frame is unknown, which counts as Modal.
#[test]
fn an_unknown_head_counts_as_modal() {
    let held = frame(Some((1, PresentedKeyboardScope::Held)));
    assert_eq!(
        presented_keyboard_scope(&[Some(&held), None]),
        PresentedKeyboardScope::Modal
    );
}

/// Modal outranks Held, and a head whose frame carries no stamp shows no
/// publication: a half-withdrawn Held strip is still Held.
#[test]
fn modal_outranks_held_and_a_withdrawn_head_adds_nothing() {
    let held = frame(Some((1, PresentedKeyboardScope::Held)));
    let modal = frame(Some((2, PresentedKeyboardScope::Modal)));
    let withdrawn = frame(None);
    assert_eq!(
        presented_keyboard_scope(&[Some(&held), Some(&modal)]),
        PresentedKeyboardScope::Modal
    );
    assert_eq!(
        presented_keyboard_scope(&[Some(&held), Some(&withdrawn)]),
        PresentedKeyboardScope::Held
    );
    assert_eq!(
        presented_keyboard_scope(&[Some(&withdrawn)]),
        PresentedKeyboardScope::None
    );
    assert_eq!(presented_keyboard_scope(&[]), PresentedKeyboardScope::None);
}

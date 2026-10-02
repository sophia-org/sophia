//! Which shortcut plans a registry accepts (t277 D2): every shape must be
//! matchable without ambiguity, reserved chords stay reserved in every step,
//! and leaders sit alone on sequence prefixes with actions of their own.
use sophia_engine::*;
use sophia_protocol::{
    WmActionId, WmBindingRegistration, WmCapabilities, WmChromePolicy, WmModifierMask,
};

const SUPER: u32 = WmModifierMask::SUPER;
const CONTROL_ALT: u32 = WmModifierMask::CONTROL | WmModifierMask::ALT;
const W: u32 = 17;
const K: u32 = 37;
const J: u32 = 36;
const Q: u32 = 16;
const F5: u32 = 63;
const F12: u32 = 88;
const BACKSPACE: u32 = 14;
const ESCAPE: u32 = 1;
const MODIFIER_KEYCODES: [u32; 8] = [42, 54, 29, 97, 56, 100, 125, 126];

const fn action(raw: u64) -> WmActionId {
    WmActionId::from_raw(raw)
}

const fn step(keycode: u32, modifiers: u32) -> WmKeyStep {
    WmKeyStep { keycode, modifiers }
}

fn sequence(steps: &[WmKeyStep], raw: u64) -> WmSequenceBinding {
    WmSequenceBinding {
        steps: steps.to_vec(),
        action: action(raw),
    }
}

fn leader(steps: &[WmKeyStep], raw: u64) -> WmSequenceLeader {
    WmSequenceLeader {
        steps: steps.to_vec(),
        action: action(raw),
    }
}

fn immediate(keycode: u32, modifiers: u32, raw: u64) -> WmBindingRegistration {
    WmBindingRegistration {
        action: action(raw),
        keycode,
        modifiers: WmModifierMask { bits: modifiers },
    }
}

fn hold(keycode: u32, modifiers: u32, hold_ms: u32, raw: u64) -> WmHoldBinding {
    WmHoldBinding {
        step: step(keycode, modifiers),
        hold_ms,
        action: action(raw),
    }
}

fn build(plan: &WmShortcutPlan) -> Result<WmShortcutRegistry, WmShortcutRegistryError> {
    WmShortcutRegistry::from_plan(
        plan,
        WmCapabilities::all_supported(),
        1,
        WmChromePolicy::default(),
    )
}

/// Super+W K and Super+W J K with a leader on Super+W, a tap and hold on
/// Super+Q, and a Super tap: every shape at once.
fn every_shape() -> WmShortcutPlan {
    WmShortcutPlan {
        immediate: vec![immediate(Q, SUPER, 1)],
        holds: vec![hold(Q, SUPER, 500, 2)],
        taps: vec![WmModifierTapBinding {
            modifier: SUPER,
            action: action(3),
        }],
        sequences: vec![
            sequence(&[step(W, SUPER), step(K, 0)], 4),
            sequence(&[step(W, SUPER), step(J, 0), step(K, 0)], 5),
        ],
        leaders: vec![leader(&[step(W, SUPER)], 6)],
        timing: WmShortcutTiming::default(),
    }
}

#[test]
fn every_shape_builds_and_counts() {
    let registry = build(&every_shape()).unwrap();
    assert_eq!(registry.binding_count(), 6);
    assert_eq!(build(&every_shape()).unwrap(), registry);
    let mut slower = every_shape();
    slower.timing.sequence_ms = 2000;
    assert_ne!(build(&slower).unwrap(), registry);
}

#[test]
fn ambiguous_or_misplaced_shapes_are_refused_by_name() {
    type Case = (&'static str, fn(&mut WmShortcutPlan), &'static str);
    let cases: Vec<Case> = vec![
        (
            "duplicate immediate",
            |plan| plan.immediate.push(immediate(Q, SUPER, 9)),
            "duplicate WM chord",
        ),
        (
            "duplicate hold",
            |plan| plan.holds.push(hold(Q, SUPER, 600, 9)),
            "duplicate WM chord",
        ),
        (
            "hold too short",
            |plan| plan.holds[0].hold_ms = 99,
            "invalid WM hold",
        ),
        (
            "hold too long",
            |plan| plan.holds[0].hold_ms = 5001,
            "invalid WM hold",
        ),
        (
            "tap of two classes",
            |plan| plan.taps[0].modifier = SUPER | WmModifierMask::ALT,
            "invalid WM modifier tap",
        ),
        (
            "tap of no class",
            |plan| plan.taps[0].modifier = 0,
            "invalid WM modifier tap",
        ),
        (
            "duplicate tap",
            |plan| {
                plan.taps.push(WmModifierTapBinding {
                    modifier: SUPER,
                    action: action(9),
                })
            },
            "duplicate WM chord",
        ),
        (
            "one-step sequence",
            |plan| plan.sequences.push(sequence(&[step(J, SUPER)], 9)),
            "invalid WM sequence",
        ),
        (
            "five-step sequence",
            |plan| {
                plan.sequences.push(sequence(
                    &[
                        step(J, SUPER),
                        step(K, 0),
                        step(K, 0),
                        step(K, 0),
                        step(K, 0),
                    ],
                    9,
                ))
            },
            "invalid WM sequence",
        ),
        (
            "escape continues",
            |plan| {
                plan.sequences
                    .push(sequence(&[step(J, SUPER), step(ESCAPE, 0)], 9))
            },
            "invalid WM sequence",
        ),
        (
            "sequence extends an immediate chord",
            |plan| {
                plan.sequences
                    .push(sequence(&[step(Q, SUPER), step(K, 0)], 9))
            },
            "WM sequence extends a binding",
        ),
        (
            "sequence extends another sequence",
            |plan| {
                plan.sequences
                    .push(sequence(&[step(W, SUPER), step(K, 0), step(J, 0)], 9))
            },
            "WM sequence extends a binding",
        ),
        (
            "sequence is a prefix of another",
            |plan| {
                plan.sequences
                    .push(sequence(&[step(W, SUPER), step(J, 0)], 9))
            },
            "WM sequence extends a binding",
        ),
        (
            "duplicate sequence",
            |plan| {
                plan.sequences
                    .push(sequence(&[step(W, SUPER), step(K, 0)], 9))
            },
            "duplicate WM chord",
        ),
        (
            "leader on a whole sequence",
            |plan| plan.leaders.push(leader(&[step(W, SUPER), step(K, 0)], 9)),
            "WM leader names no sequence prefix",
        ),
        (
            "leader on nothing",
            |plan| plan.leaders.push(leader(&[step(J, SUPER)], 9)),
            "WM leader names no sequence prefix",
        ),
        (
            "duplicate leader",
            |plan| plan.leaders.push(leader(&[step(W, SUPER)], 9)),
            "duplicate WM leader",
        ),
        (
            "nested leaders",
            |plan| plan.leaders.push(leader(&[step(W, SUPER), step(J, 0)], 9)),
            "nested WM leaders",
        ),
        (
            "leader action bound",
            |plan| plan.leaders[0].action = action(1),
            "WM leader action is reused",
        ),
        (
            "leader action a leaf",
            |plan| plan.leaders[0].action = action(4),
            "WM leader action is reused",
        ),
        (
            "tap window too short",
            |plan| plan.timing.tap_ms = 49,
            "invalid WM shortcut timing",
        ),
        (
            "sequence timeout too long",
            |plan| plan.timing.sequence_ms = 10001,
            "invalid WM shortcut timing",
        ),
    ];
    for (name, change, reason) in cases {
        let mut plan = every_shape();
        change(&mut plan);
        assert_eq!(build(&plan).map(|_| ()), Err(reason), "{name}");
    }
}

#[test]
fn reserved_chords_are_refused_in_every_step() {
    let mut cases: Vec<WmShortcutPlan> = Vec::new();
    for (keycode, extra) in [(F5, 0), (F12, SUPER | WmModifierMask::SHIFT)] {
        let modifiers = CONTROL_ALT | extra;
        cases.push(WmShortcutPlan {
            immediate: vec![immediate(keycode, modifiers, 1)],
            ..WmShortcutPlan::default()
        });
        cases.push(WmShortcutPlan {
            holds: vec![hold(keycode, modifiers, 500, 1)],
            ..WmShortcutPlan::default()
        });
        cases.push(WmShortcutPlan {
            sequences: vec![sequence(&[step(W, SUPER), step(keycode, modifiers)], 1)],
            ..WmShortcutPlan::default()
        });
        cases.push(WmShortcutPlan {
            sequences: vec![sequence(&[step(keycode, modifiers), step(K, 0)], 1)],
            ..WmShortcutPlan::default()
        });
    }
    for plan in &cases {
        assert_eq!(
            build(plan).map(|_| ()),
            Err("reserved virtual-terminal chord"),
            "{plan:?}"
        );
    }
    let emergency = WmShortcutPlan {
        sequences: vec![sequence(&[step(W, SUPER), step(BACKSPACE, CONTROL_ALT)], 1)],
        ..WmShortcutPlan::default()
    };
    assert_eq!(
        build(&emergency).map(|_| ()),
        Err("reserved emergency chord")
    );
    // Neither modifier alone reserves a function key.
    let free = WmShortcutPlan {
        immediate: vec![
            immediate(F5, WmModifierMask::CONTROL | SUPER, 1),
            immediate(F5, WmModifierMask::ALT, 2),
        ],
        ..WmShortcutPlan::default()
    };
    build(&free).unwrap();
}

#[test]
fn the_entry_bound_counts_every_shape() {
    let mut plan = every_shape();
    // Five entries besides the immediate chords; fill the rest of the 256
    // with immediate chords.
    let mut keycode = 30;
    while plan.immediate.len() + 5 < 256 {
        if MODIFIER_KEYCODES.contains(&keycode) {
            keycode += 1;
            continue;
        }
        for modifiers in [0, WmModifierMask::SHIFT] {
            if plan.immediate.len() + 5 < 256 {
                plan.immediate.push(immediate(
                    keycode,
                    modifiers,
                    100 + u64::from(keycode) * 2 + u64::from(modifiers),
                ));
            }
        }
        keycode += 1;
    }
    assert_eq!(build(&plan).unwrap().binding_count(), 256);
    plan.immediate.push(immediate(200, 0, 9999));
    assert_eq!(build(&plan).map(|_| ()), Err("too many WM bindings"));
}

/// D2 review R3: a modifier key only changes the mask, so no ordinary step
/// may name one; a lone modifier is bound as a modifier tap.
#[test]
fn modifier_keys_are_refused_as_key_steps() {
    for keycode in MODIFIER_KEYCODES {
        let shapes = [
            WmShortcutPlan {
                immediate: vec![immediate(keycode, 0, 1)],
                ..WmShortcutPlan::default()
            },
            WmShortcutPlan {
                holds: vec![hold(keycode, 0, 500, 1)],
                ..WmShortcutPlan::default()
            },
            WmShortcutPlan {
                sequences: vec![sequence(&[step(keycode, 0), step(K, 0)], 1)],
                ..WmShortcutPlan::default()
            },
            WmShortcutPlan {
                sequences: vec![sequence(&[step(W, SUPER), step(keycode, 0)], 1)],
                ..WmShortcutPlan::default()
            },
            WmShortcutPlan {
                sequences: vec![sequence(&[step(W, SUPER), step(K, 0)], 1)],
                leaders: vec![leader(&[step(keycode, SUPER)], 2)],
                ..WmShortcutPlan::default()
            },
        ];
        for plan in &shapes {
            assert_eq!(
                build(plan).map(|_| ()),
                Err("modifier key is not a WM key step"),
                "{plan:?}"
            );
        }
    }
    // Control: every modifier class is still a valid tap.
    let taps = WmShortcutPlan {
        taps: [
            WmModifierMask::SHIFT,
            WmModifierMask::CONTROL,
            WmModifierMask::ALT,
            SUPER,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, modifier)| WmModifierTapBinding {
            modifier,
            action: action(index as u64 + 1),
        })
        .collect(),
        ..WmShortcutPlan::default()
    };
    build(&taps).unwrap();
}

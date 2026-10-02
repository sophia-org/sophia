//! Deferred matching in the engine (t277 D2): modifier taps, tap versus hold,
//! sequences and leaders, the shared advance of due work, the ordered output
//! batch, the opening press's latch, and two-phase proposals.
use sophia_engine::*;
use sophia_protocol::{
    DeviceId, PolicyActionLifecycleInterest, PolicyChordEnd, SeatId, WmActionId,
    WmBindingRegistration, WmCapabilities, WmChromePolicy, WmModifierMask,
};

const SEAT: SeatId = SeatId::from_raw(1);
const OTHER_SEAT: SeatId = SeatId::from_raw(2);
const KEYBOARD: DeviceId = DeviceId::from_raw(1);
const SECOND_KEYBOARD: DeviceId = DeviceId::from_raw(2);
const SUPER: u32 = WmModifierMask::SUPER;
const LEFT_SUPER: u32 = 125;
const LEFT_ALT: u32 = 56;
const LEFT_SHIFT: u32 = 42;
const ESCAPE: u32 = 1;
const Q: u32 = 16;
const W: u32 = 17;
const E: u32 = 18;
const J: u32 = 36;
const K: u32 = 37;
const L: u32 = 38;
const X: u32 = 45;
const B: u32 = 48;
const N: u32 = 49;
const Z: u32 = 44;
const Y: u32 = 21;
const M: u32 = 50;

const LAUNCHER: WmActionId = WmActionId::from_raw(1); // Super tap
const CLOSE: WmActionId = WmActionId::from_raw(2); // Super+Q tap
const FORCE: WmActionId = WmActionId::from_raw(3); // Super+Q hold 500
const POWER: WmActionId = WmActionId::from_raw(4); // Super+X hold only 500
const KILL: WmActionId = WmActionId::from_raw(5); // Super+W K
const DEEP: WmActionId = WmActionId::from_raw(6); // Super+W J L
const SUPER_K: WmActionId = WmActionId::from_raw(7); // Super+W Super+K
const HINT: WmActionId = WmActionId::from_raw(8); // leader Super+W
const BARE_TAP: WmActionId = WmActionId::from_raw(9); // B tap
const BARE_HOLD: WmActionId = WmActionId::from_raw(10); // B hold 500
const EDIT: WmActionId = WmActionId::from_raw(11); // Super+E J K
const EDIT_HINT: WmActionId = WmActionId::from_raw(12); // leader Super+E J
const PLAIN: WmActionId = WmActionId::from_raw(13); // N, and Z's tap
const SHIFTED: WmActionId = WmActionId::from_raw(14); // Super+E Shift+L K
const Z_HOLD: WmActionId = WmActionId::from_raw(15); // Z hold 500

fn step(keycode: u32, modifiers: u32) -> WmKeyStep {
    WmKeyStep { keycode, modifiers }
}

fn plan() -> WmShortcutPlan {
    let bind = |action, keycode, modifiers| WmBindingRegistration {
        action,
        keycode,
        modifiers: WmModifierMask { bits: modifiers },
    };
    let hold = |action, keycode, modifiers| WmHoldBinding {
        step: step(keycode, modifiers),
        hold_ms: 500,
        action,
    };
    let sequence = |action, steps: &[WmKeyStep]| WmSequenceBinding {
        steps: steps.to_vec(),
        action,
    };
    WmShortcutPlan {
        immediate: vec![
            bind(CLOSE, Q, SUPER),
            bind(BARE_TAP, B, 0),
            bind(PLAIN, N, 0),
            bind(PLAIN, Z, 0),
            bind(PLAIN, M, 0),
            bind(PLAIN, W, WmModifierMask::ALT),
        ],
        holds: vec![
            hold(FORCE, Q, SUPER),
            hold(POWER, X, SUPER),
            hold(BARE_HOLD, B, 0),
            hold(Z_HOLD, Z, 0),
            hold(PLAIN, Y, WmModifierMask::ALT),
        ],
        taps: vec![WmModifierTapBinding {
            modifier: SUPER,
            action: LAUNCHER,
        }],
        sequences: vec![
            sequence(KILL, &[step(W, SUPER), step(K, 0)]),
            sequence(SUPER_K, &[step(W, SUPER), step(K, SUPER)]),
            sequence(DEEP, &[step(W, SUPER), step(J, 0), step(L, 0)]),
            sequence(EDIT, &[step(E, SUPER), step(J, 0), step(K, 0)]),
            sequence(
                SHIFTED,
                &[step(E, SUPER), step(L, WmModifierMask::SHIFT), step(K, 0)],
            ),
        ],
        leaders: vec![
            WmSequenceLeader {
                steps: vec![step(W, SUPER)],
                action: HINT,
            },
            WmSequenceLeader {
                steps: vec![step(E, SUPER), step(J, 0)],
                action: EDIT_HINT,
            },
        ],
        timing: WmShortcutTiming::default(),
    }
}

/// A router with the outputs its key events returned kept in order, so the
/// tests can take everything produced since the last take.
struct Keys {
    router: WmShortcutRouter,
    kept: Vec<WmShortcutOutput>,
}

impl Keys {
    fn new(router: WmShortcutRouter) -> Self {
        Self {
            router,
            kept: Vec::new(),
        }
    }

    fn keep(&mut self, outputs: Vec<WmShortcutOutput>) {
        self.kept.extend(outputs);
    }

    fn take(&mut self) -> Vec<WmShortcutOutput> {
        let mut outputs = core::mem::take(&mut self.kept);
        outputs.extend(self.router.take_outputs());
        outputs
    }
}

impl core::ops::Deref for Keys {
    type Target = WmShortcutRouter;

    fn deref(&self) -> &WmShortcutRouter {
        &self.router
    }
}

impl core::ops::DerefMut for Keys {
    fn deref_mut(&mut self) -> &mut WmShortcutRouter {
        &mut self.router
    }
}

fn router_with(interests: &[(WmActionId, u32)]) -> Keys {
    let registry = WmShortcutRegistry::from_plan(
        &plan(),
        WmCapabilities::all_supported(),
        1,
        WmChromePolicy::default(),
    )
    .unwrap();
    let mut router = Keys::new(WmShortcutRouter::new(registry));
    let interests = interests
        .iter()
        .map(|(action, held_ms)| PolicyActionLifecycleInterest {
            action: *action,
            held_ms: *held_ms,
        })
        .collect::<Vec<_>>();
    router.set_action_lifecycles(&interests);
    router
}

fn router() -> Keys {
    router_with(&[])
}

/// Route a key event on the first keyboard and accept what it proposes.
fn key(router: &mut Keys, keycode: u32, pressed: bool, time: u64) -> bool {
    key_on(router, SEAT, KEYBOARD, keycode, pressed, time)
}

fn key_on(
    router: &mut Keys,
    seat: SeatId,
    device: DeviceId,
    keycode: u32,
    pressed: bool,
    time: u64,
) -> bool {
    let event = router.key_event(seat, device, keycode, pressed, time);
    let consumed = event.consumed();
    let returned = event.accept();
    router.keep(returned);
    consumed
}

fn proposal(router: &mut Keys, keycode: u32, pressed: bool, time: u64) -> Option<WmPressProposal> {
    let event = router.key_event(SEAT, KEYBOARD, keycode, pressed, time);
    let proposal = event.proposal().cloned();
    let returned = event.accept();
    router.keep(returned);
    proposal
}

fn decline(router: &mut Keys, keycode: u32, pressed: bool, time: u64) {
    let returned = router
        .key_event(SEAT, KEYBOARD, keycode, pressed, time)
        .decline();
    router.keep(returned);
}

fn tap(router: &mut Keys, keycode: u32, time: u64) {
    key(router, keycode, true, time);
    key(router, keycode, false, time);
}

/// The actions fired since the last take, in order.
fn fired(router: &mut Keys) -> Vec<WmActionId> {
    router
        .take()
        .into_iter()
        .filter_map(|output| match output {
            WmShortcutOutput::Activation(activation) => Some(activation.action),
            WmShortcutOutput::Chord(_) => None,
        })
        .collect()
}

/// Outputs reduced to what order tests compare: an action or a chord event
/// kind, with tokens replaced by the action whose chord it is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Out {
    Fired(WmActionId),
    Opened(WmActionId),
    Held,
    Ended(PolicyChordEnd),
}

fn outputs(router: &mut Keys) -> Vec<Out> {
    router
        .take()
        .into_iter()
        .map(|output| match output {
            WmShortcutOutput::Activation(activation) => match activation.chord {
                Some(chord) if chord.opens => Out::Opened(activation.action),
                _ => Out::Fired(activation.action),
            },
            WmShortcutOutput::Chord(WmChordEvent::Held { .. }) => Out::Held,
            WmShortcutOutput::Chord(WmChordEvent::Ended { end, .. }) => Out::Ended(end),
        })
        .collect()
}

mod modifier_taps {
    use super::*;

    #[test]
    fn a_lone_modifier_released_within_its_window_fires() {
        let mut router = router();
        let arm = proposal(&mut router, LEFT_SUPER, true, 0).unwrap();
        assert_eq!(arm.kind, WmPressKind::ArmModifierTap);
        assert_eq!(arm.possible_actions, [LAUNCHER]);
        let event = router.key_event(SEAT, KEYBOARD, LEFT_SUPER, false, 399);
        // Modifiers always reach clients, tap or not.
        assert!(!event.consumed());
        assert_eq!(event.proposal().unwrap().kind, WmPressKind::FireModifierTap);
        let returned = event.accept();
        router.keep(returned);
        assert_eq!(fired(&mut router), [LAUNCHER]);
        assert!(router.shortcut_idle());
        assert_eq!(router.next_deadline(), None);
    }

    #[test]
    fn a_tap_held_past_its_window_fires_nothing() {
        let mut router = router();
        key(&mut router, LEFT_SUPER, true, 0);
        assert!(proposal(&mut router, LEFT_SUPER, false, 400).is_none());
        assert!(fired(&mut router).is_empty());
    }

    #[test]
    fn a_key_a_second_modifier_or_the_pointer_disarms_it() {
        for interrupt in 0..3 {
            let mut router = router();
            key(&mut router, LEFT_SUPER, true, 0);
            match interrupt {
                0 => tap(&mut router, L, 10),
                1 => tap(&mut router, LEFT_SHIFT, 10),
                _ => router.pointer_activity(SEAT, 10),
            }
            assert!(
                proposal(&mut router, LEFT_SUPER, false, 20).is_none(),
                "{interrupt}"
            );
            assert!(fired(&mut router).is_empty(), "{interrupt}");
        }
    }

    #[test]
    fn nothing_arms_with_another_key_down_or_a_decision_pending() {
        let mut router = router();
        key(&mut router, L, true, 0);
        assert!(proposal(&mut router, LEFT_SUPER, true, 10).is_none());
        let mut router = super::router();
        key(&mut router, B, true, 0);
        key(&mut router, B, false, 10);
        fired(&mut router);
        key(&mut router, LEFT_SUPER, true, 20);
        key(&mut router, W, true, 30);
        key(&mut router, W, false, 40);
        // Super is down and a sequence is pending: Shift arms nothing.
        assert!(proposal(&mut router, LEFT_SHIFT, true, 50).is_none());
    }

    #[test]
    fn a_declined_arm_or_fire_fires_nothing() {
        let mut router = router();
        decline(&mut router, LEFT_SUPER, true, 0);
        assert!(proposal(&mut router, LEFT_SUPER, false, 10).is_none());
        key(&mut router, LEFT_SUPER, true, 20);
        decline(&mut router, LEFT_SUPER, false, 30);
        assert!(fired(&mut router).is_empty());
    }

    #[test]
    fn its_chord_ends_released_right_after_its_action() {
        let mut router = router_with(&[(LAUNCHER, 0)]);
        tap(&mut router, LEFT_SUPER, 0);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(LAUNCHER), Out::Ended(PolicyChordEnd::Released)]
        );
    }
}

mod tap_or_hold {
    use super::*;

    fn super_q(router: &mut Keys, time: u64) -> WmPressProposal {
        key(router, LEFT_SUPER, true, time);
        proposal(router, Q, true, time).unwrap()
    }

    #[test]
    fn a_release_before_the_deadline_is_the_tap() {
        let mut router = router();
        let deciding = super_q(&mut router, 0);
        assert_eq!(deciding.kind, WmPressKind::Deciding);
        assert_eq!(deciding.possible_actions, [CLOSE, FORCE]);
        assert_eq!(router.next_deadline(), Some(500));
        assert!(key(&mut router, Q, false, 499));
        assert_eq!(fired(&mut router), [CLOSE]);
        assert_eq!(router.next_deadline(), None);
    }

    #[test]
    fn the_deadline_with_the_key_down_is_the_hold() {
        let mut router = router();
        super_q(&mut router, 0);
        router.poll_shortcuts(500);
        assert_eq!(fired(&mut router), [FORCE]);
        assert!(key(&mut router, Q, false, 900));
        assert!(fired(&mut router).is_empty());
    }

    #[test]
    fn a_release_after_an_unserviced_deadline_is_still_the_hold() {
        let mut router = router();
        super_q(&mut router, 0);
        assert!(key(&mut router, Q, false, 700));
        assert_eq!(fired(&mut router), [FORCE]);
    }

    /// Q1 and R1: an interruption before the deadline fires neither, and a
    /// deadline already due is settled before the interrupting input, so
    /// servicing the timer first changes nothing.
    #[test]
    fn interruption_before_the_deadline_fires_nothing_and_after_it_settles_first() {
        let interrupts: [fn(&mut Keys, u64); 2] = [
            |router, time| tap(router, L, time),
            |router, time| router.pointer_activity(SEAT, time),
        ];
        for interrupt in interrupts {
            let mut router = router();
            super_q(&mut router, 0);
            interrupt(&mut router, 499);
            assert!(key(&mut router, Q, false, 600));
            router.poll_shortcuts(600);
            assert!(fired(&mut router).is_empty());

            let mut event_first = super::router();
            super_q(&mut event_first, 0);
            interrupt(&mut event_first, 600);
            let mut poll_first = super::router();
            super_q(&mut poll_first, 0);
            poll_first.poll_shortcuts(600);
            interrupt(&mut poll_first, 600);
            let event_first = outputs(&mut event_first);
            assert_eq!(event_first, [Out::Fired(FORCE)]);
            assert_eq!(outputs(&mut poll_first), event_first);
        }
    }

    #[test]
    fn a_hold_only_short_press_is_consumed_and_fires_nothing() {
        let mut router = router();
        key(&mut router, LEFT_SUPER, true, 0);
        assert_eq!(
            proposal(&mut router, X, true, 0).unwrap().possible_actions,
            [POWER]
        );
        assert!(key(&mut router, X, false, 100));
        assert!(fired(&mut router).is_empty());
    }

    #[test]
    fn a_repeat_leaves_the_decision_alone() {
        let mut router = router();
        super_q(&mut router, 0);
        let repeat = router.key_event(SEAT, KEYBOARD, Q, true, 250);
        assert!(repeat.consumed());
        assert!(repeat.proposal().is_none());
        let returned = repeat.accept();
        router.keep(returned);
        key(&mut router, Q, false, 300);
        assert_eq!(fired(&mut router), [CLOSE]);
    }

    #[test]
    fn a_declined_decision_never_fires() {
        let mut router = router();
        key(&mut router, LEFT_SUPER, true, 0);
        decline(&mut router, Q, true, 0);
        assert_eq!(router.next_deadline(), None);
        router.poll_shortcuts(600);
        assert!(key(&mut router, Q, false, 700));
        assert!(fired(&mut router).is_empty());
    }

    /// R2: the opening press decides the latch, checked against what is
    /// down when the delayed activation is emitted.
    #[test]
    fn a_modifier_latched_hold_whose_modifier_is_gone_ends_at_once() {
        let mut router = router_with(&[(FORCE, 0)]);
        super_q(&mut router, 0);
        key(&mut router, LEFT_SUPER, false, 100);
        router.poll_shortcuts(500);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(FORCE), Out::Ended(PolicyChordEnd::Released)]
        );
    }

    #[test]
    fn a_bare_tap_stays_trigger_latched_after_a_modifier_goes_down() {
        let mut router = router_with(&[(BARE_TAP, 0)]);
        key(&mut router, B, true, 0);
        key(&mut router, LEFT_ALT, true, 50);
        key(&mut router, B, false, 100);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(BARE_TAP), Out::Ended(PolicyChordEnd::Released)]
        );
    }

    #[test]
    fn a_hold_first_serviced_by_its_release_opens_then_ends() {
        let mut router = router_with(&[(BARE_HOLD, 0)]);
        key(&mut router, B, true, 0);
        key(&mut router, B, false, 600);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(BARE_HOLD), Out::Ended(PolicyChordEnd::Released)]
        );
    }

    #[test]
    fn a_held_hold_ends_by_its_latch_in_either_release_order() {
        for modifier_first in [false, true] {
            let mut router = router_with(&[(FORCE, 0)]);
            super_q(&mut router, 0);
            router.poll_shortcuts(500);
            assert_eq!(outputs(&mut router), [Out::Opened(FORCE)]);
            let (first, last) = if modifier_first {
                (LEFT_SUPER, Q)
            } else {
                (Q, LEFT_SUPER)
            };
            key(&mut router, first, false, 600);
            if modifier_first {
                assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Released)]);
            } else {
                assert!(outputs(&mut router).is_empty());
            }
            key(&mut router, last, false, 700);
            if !modifier_first {
                assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Released)]);
            }
        }
    }

    /// A tap fired at its release joins an open chord of its action without
    /// adding its key, which is already up, as a phantom trigger.
    #[test]
    fn a_delayed_join_adds_no_released_trigger() {
        let mut router = router_with(&[(PLAIN, 0)]);
        key(&mut router, N, true, 0);
        key(&mut router, Z, true, 10);
        key(&mut router, Z, false, 100);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(PLAIN), Out::Fired(PLAIN)]
        );
        key(&mut router, N, false, 200);
        assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Released)]);
    }

    /// D2 review R1: the opener decides how a chord is held. A join pressed
    /// with modifiers still holds a trigger-latched chord by its key.
    #[test]
    fn a_modified_join_holds_a_trigger_latched_chord() {
        let mut router = router_with(&[(PLAIN, 0)]);
        key(&mut router, N, true, 0);
        key(&mut router, LEFT_ALT, true, 10);
        key(&mut router, W, true, 20);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(PLAIN), Out::Fired(PLAIN)]
        );
        key(&mut router, N, false, 30);
        assert!(outputs(&mut router).is_empty(), "W still holds the chord");
        key(&mut router, W, false, 40);
        assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Released)]);
    }

    /// The same for a delayed join: a modified hold fired while its key is
    /// down joins a bare opener and holds it by that key.
    #[test]
    fn a_delayed_modified_hold_joins_by_its_key() {
        let mut router = router_with(&[(PLAIN, 0)]);
        key(&mut router, N, true, 0);
        key(&mut router, LEFT_ALT, true, 10);
        key(&mut router, Y, true, 20);
        router.poll_shortcuts(520);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(PLAIN), Out::Fired(PLAIN)]
        );
        key(&mut router, N, false, 600);
        assert!(outputs(&mut router).is_empty(), "Y still holds the chord");
        key(&mut router, Y, false, 700);
        assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Released)]);
    }

    /// Control: an unmodified join holds the chord until its last trigger.
    #[test]
    fn an_unmodified_join_holds_until_the_last_trigger() {
        let mut router = router_with(&[(PLAIN, 0)]);
        key(&mut router, N, true, 0);
        key(&mut router, M, true, 10);
        key(&mut router, N, false, 20);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(PLAIN), Out::Fired(PLAIN)]
        );
        key(&mut router, M, false, 30);
        assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Released)]);
    }

    #[test]
    fn held_counts_from_the_emission_not_the_deadline() {
        let mut router = router_with(&[(FORCE, 150)]);
        super_q(&mut router, 0);
        router.poll_shortcuts(700);
        assert_eq!(outputs(&mut router), [Out::Opened(FORCE)]);
        assert_eq!(router.next_deadline(), Some(850));
        router.poll_shortcuts(850);
        assert_eq!(outputs(&mut router), [Out::Held]);
    }

    #[test]
    fn a_hold_refused_for_credit_fires_nothing_and_keeps_its_release() {
        let mut router = router_with(&[(FORCE, 0), (PLAIN, 0)]);
        for seat in 1..=8 {
            key_on(
                &mut router,
                SeatId::from_raw(seat + 10),
                KEYBOARD,
                N,
                true,
                0,
            );
        }
        assert_eq!(router.chord_credits_free(), 0);
        fired(&mut router);
        super_q(&mut router, 0);
        router.poll_shortcuts(500);
        assert!(fired(&mut router).is_empty());
        assert!(key(&mut router, Q, false, 600));
    }
}

mod sequences {
    use super::*;

    fn start(router: &mut Keys, time: u64) -> WmPressProposal {
        key(router, LEFT_SUPER, true, time);
        proposal(router, W, true, time).unwrap()
    }

    #[test]
    fn a_sequence_fires_its_leaf_and_its_leader_completes() {
        let mut router = router_with(&[(HINT, 0)]);
        let first = start(&mut router, 0);
        assert_eq!(first.kind, WmPressKind::SequenceStart);
        assert_eq!(first.possible_actions, [KILL, DEEP, SUPER_K, HINT]);
        assert!(first.follows_chord);
        assert_eq!(outputs(&mut router), [Out::Opened(HINT)]);
        key(&mut router, LEFT_SUPER, false, 50);
        let leaf = proposal(&mut router, K, true, 100).unwrap();
        assert_eq!(leaf.kind, WmPressKind::SequenceContinue);
        assert_eq!(leaf.possible_actions, [KILL]);
        assert_eq!(
            outputs(&mut router),
            [Out::Fired(KILL), Out::Ended(PolicyChordEnd::Completed)]
        );
        assert_eq!(router.next_deadline(), None);
    }

    #[test]
    fn a_descent_narrows_the_possible_actions() {
        let mut router = router();
        start(&mut router, 0);
        tap(&mut router, LEFT_SUPER, 10);
        let descent = proposal(&mut router, J, true, 100).unwrap();
        assert_eq!(descent.possible_actions, [DEEP]);
        assert_eq!(router.next_deadline(), Some(1100));
        key(&mut router, J, false, 150);
        key(&mut router, L, true, 200);
        assert_eq!(fired(&mut router), [DEEP]);
    }

    #[test]
    fn escape_or_an_unmatched_key_abandons_it_and_is_swallowed() {
        for key_code in [ESCAPE, X] {
            let mut router = router_with(&[(HINT, 0)]);
            start(&mut router, 0);
            outputs(&mut router);
            let event = router.key_event(SEAT, KEYBOARD, key_code, true, 100);
            assert!(event.consumed());
            assert!(event.proposal().unwrap().possible_actions.is_empty());
            let returned = event.accept();
            router.keep(returned);
            assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Aborted)]);
            assert!(key(&mut router, key_code, false, 150));
        }
    }

    #[test]
    fn it_times_out_by_the_timer_or_by_a_late_key() {
        let mut router = router_with(&[(HINT, 0)]);
        start(&mut router, 0);
        outputs(&mut router);
        router.poll_shortcuts(1000);
        assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::TimedOut)]);

        let mut router = router_with(&[(HINT, 0), (PLAIN, 0)]);
        start(&mut router, 0);
        outputs(&mut router);
        key(&mut router, LEFT_SUPER, false, 10);
        // The late key is fresh input, after the leader timed out.
        assert!(key(&mut router, N, true, 1500));
        assert_eq!(
            outputs(&mut router),
            [Out::Ended(PolicyChordEnd::TimedOut), Out::Opened(PLAIN)]
        );
    }

    #[test]
    fn releases_mid_sequence_end_nothing_and_held_still_comes() {
        let mut router = router_with(&[(HINT, 200)]);
        start(&mut router, 0);
        assert!(key(&mut router, W, false, 10));
        key(&mut router, LEFT_SUPER, false, 20);
        assert_eq!(outputs(&mut router), [Out::Opened(HINT)]);
        router.poll_shortcuts(200);
        assert_eq!(outputs(&mut router), [Out::Held]);
    }

    /// Q2: the exact mask first, then the mask without the classes of the
    /// steps matched so far.
    #[test]
    fn an_exact_continuation_wins_over_the_fallback() {
        let cases: [(&[u32], Option<WmActionId>); 3] = [
            (&[LEFT_SUPER], Some(SUPER_K)),
            (&[], Some(KILL)),
            (&[LEFT_SUPER, LEFT_SHIFT], None),
        ];
        for (down, expected) in cases {
            let mut router = router();
            start(&mut router, 0);
            key(&mut router, W, false, 10);
            key(&mut router, LEFT_SUPER, false, 20);
            for modifier in down {
                key(&mut router, *modifier, true, 30);
            }
            key(&mut router, K, true, 40);
            assert_eq!(fired(&mut router), expected.into_iter().collect::<Vec<_>>());
        }
    }

    #[test]
    fn the_fallback_counts_classes_on_any_keyboard_and_after_a_repress() {
        // Plan without Super+W Super+K, so a held Super only falls back.
        let mut plan = plan();
        plan.sequences.retain(|sequence| sequence.action != SUPER_K);
        let registry = WmShortcutRegistry::from_plan(
            &plan,
            WmCapabilities::all_supported(),
            1,
            WmChromePolicy::default(),
        )
        .unwrap();
        for second_keyboard in [false, true] {
            let mut router = Keys::new(WmShortcutRouter::new(registry.clone()));
            start(&mut router, 0);
            key(&mut router, W, false, 10);
            key(&mut router, LEFT_SUPER, false, 20);
            let device = if second_keyboard {
                SECOND_KEYBOARD
            } else {
                KEYBOARD
            };
            key_on(&mut router, SEAT, device, LEFT_SUPER, true, 30);
            key(&mut router, K, true, 40);
            assert_eq!(fired(&mut router), [KILL], "{second_keyboard}");
        }
    }

    /// The fallback sets aside every class matched so far: Shift held from
    /// the second step still lets an unmodified third step match.
    #[test]
    fn matched_classes_accumulate_across_steps() {
        let mut router = router();
        key(&mut router, LEFT_SUPER, true, 0);
        key(&mut router, E, true, 0);
        key(&mut router, LEFT_SUPER, false, 10);
        key(&mut router, LEFT_SHIFT, true, 20);
        key(&mut router, L, true, 30);
        key(&mut router, K, true, 40);
        assert_eq!(fired(&mut router), [SHIFTED]);
    }

    #[test]
    fn two_seats_keep_separate_sequences() {
        let mut router = router();
        key(&mut router, LEFT_SUPER, true, 0);
        key(&mut router, W, true, 0);
        key_on(&mut router, OTHER_SEAT, KEYBOARD, LEFT_SUPER, true, 10);
        key_on(&mut router, OTHER_SEAT, KEYBOARD, W, true, 10);
        key_on(&mut router, OTHER_SEAT, KEYBOARD, X, true, 20);
        key(&mut router, LEFT_SUPER, false, 30);
        key(&mut router, K, true, 40);
        assert_eq!(fired(&mut router), [KILL]);
    }

    #[test]
    fn a_leader_on_a_deeper_prefix_fires_on_descent() {
        let mut router = router_with(&[(EDIT_HINT, 0)]);
        key(&mut router, LEFT_SUPER, true, 0);
        let first = proposal(&mut router, E, true, 0).unwrap();
        assert_eq!(first.possible_actions, [EDIT, EDIT_HINT, SHIFTED]);
        assert!(outputs(&mut router).is_empty());
        key(&mut router, LEFT_SUPER, false, 10);
        key(&mut router, J, true, 20);
        assert_eq!(outputs(&mut router), [Out::Opened(EDIT_HINT)]);
        key(&mut router, K, true, 30);
        assert_eq!(
            outputs(&mut router),
            [Out::Fired(EDIT), Out::Ended(PolicyChordEnd::Completed)]
        );
    }

    /// A leader fires only as a followed chord. Undeclared, without credit or
    /// refused, it leaves nothing behind and the sequence continues.
    #[test]
    fn an_ineligible_or_refused_leader_lets_the_sequence_continue() {
        for case in 0..3 {
            let mut router = match case {
                0 => router(),
                _ => router_with(&[(HINT, 0), (PLAIN, 0)]),
            };
            if case == 1 {
                for seat in 1..=8 {
                    key_on(
                        &mut router,
                        SeatId::from_raw(seat + 10),
                        KEYBOARD,
                        N,
                        true,
                        0,
                    );
                }
            }
            outputs(&mut router);
            start(&mut router, 0);
            if case == 2 {
                let token = match router.take().as_slice() {
                    [WmShortcutOutput::Activation(activation)] => activation.chord.unwrap().token,
                    other => panic!("expected the leader, got {other:?}"),
                };
                assert!(router.chord_opener_refused(token));
            }
            assert!(outputs(&mut router).is_empty(), "{case}");
            key(&mut router, LEFT_SUPER, false, 10);
            key(&mut router, K, true, 20);
            assert_eq!(outputs(&mut router), [Out::Fired(KILL)], "{case}");
        }
    }

    #[test]
    fn an_open_leader_survives_its_declaration_removal() {
        let mut router = router_with(&[(HINT, 0)]);
        start(&mut router, 0);
        outputs(&mut router);
        router.set_action_lifecycles(&[]);
        key(&mut router, LEFT_SUPER, false, 10);
        let leaf = proposal(&mut router, K, true, 20).unwrap();
        assert!(leaf.follows_chord, "the open leader keeps its eligibility");
        assert_eq!(
            outputs(&mut router),
            [Out::Fired(KILL), Out::Ended(PolicyChordEnd::Completed)]
        );
    }

    /// A refused keyboard's modifier makes the seat uncertain, whether it
    /// was refused for that press or refused earlier: what was pending is
    /// dropped without firing, and a sequence's leader ends aborted.
    #[test]
    fn uncertainty_drops_what_is_pending_without_firing() {
        for (deciding, refused_earlier) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut router = router_with(&[(HINT, 0), (FORCE, 0)]);
            // Fifteen other keyboards hold a key, so with the first keyboard
            // every slot is taken and a further keyboard is refused.
            for device in 10..25 {
                key_on(&mut router, SEAT, DeviceId::from_raw(device), L, true, 0);
            }
            let refused = DeviceId::from_raw(40);
            key(&mut router, LEFT_SUPER, true, 0);
            if refused_earlier {
                key_on(&mut router, SEAT, refused, L, true, 0);
                assert!(!router.seat_uncertain(SEAT));
            }
            key(&mut router, if deciding { Q } else { W }, true, 0);
            outputs(&mut router);
            key_on(&mut router, SEAT, refused, LEFT_SHIFT, true, 10);
            assert!(router.seat_uncertain(SEAT));
            let expected = if deciding {
                Vec::new()
            } else {
                vec![Out::Ended(PolicyChordEnd::Aborted)]
            };
            let case = (deciding, refused_earlier);
            assert_eq!(outputs(&mut router), expected, "{case:?}");
            assert_eq!(router.next_deadline(), None, "{case:?}");
            router.poll_shortcuts(2000);
            assert!(outputs(&mut router).is_empty(), "{case:?}");
        }
    }

    #[test]
    fn the_pointer_abandons_a_sequence() {
        let mut router = router_with(&[(HINT, 0)]);
        start(&mut router, 0);
        outputs(&mut router);
        router.pointer_activity(SEAT, 10);
        assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Aborted)]);
    }
}

mod proposals {
    use super::*;

    #[test]
    fn an_immediate_opener_without_credit_is_consumed_before_any_capture() {
        let mut router = router_with(&[(PLAIN, 0)]);
        for seat in 1..=8 {
            key_on(
                &mut router,
                SeatId::from_raw(seat + 10),
                KEYBOARD,
                N,
                true,
                0,
            );
        }
        fired(&mut router);
        let event = router.key_event(SEAT, KEYBOARD, N, true, 10);
        assert!(event.consumed());
        assert!(event.proposal().is_none());
        let returned = event.accept();
        router.keep(returned);
        assert!(fired(&mut router).is_empty());
        // A join needs no credit and is still proposed: seat 11 still holds
        // N on its first keyboard.
        let join = router.key_event(SeatId::from_raw(11), SECOND_KEYBOARD, N, true, 30);
        assert!(
            join.proposal()
                .is_some_and(|proposal| proposal.follows_chord)
        );
        let returned = join.accept();
        router.keep(returned);
        assert_eq!(fired(&mut router), [PLAIN]);
    }

    #[test]
    fn a_declined_prefix_extracts_no_leader_and_a_declined_continuation_aborts() {
        let mut router = router_with(&[(HINT, 0)]);
        key(&mut router, LEFT_SUPER, true, 0);
        decline(&mut router, W, true, 0);
        assert!(outputs(&mut router).is_empty());
        assert_eq!(router.next_deadline(), None);
        key(&mut router, W, false, 10);
        key(&mut router, W, true, 20);
        assert_eq!(outputs(&mut router), [Out::Opened(HINT)]);
        decline(&mut router, K, true, 30);
        assert_eq!(outputs(&mut router), [Out::Ended(PolicyChordEnd::Aborted)]);
    }

    #[test]
    fn dropping_an_event_declines_it() {
        let mut router = router();
        key(&mut router, LEFT_SUPER, true, 0);
        drop(router.key_event(SEAT, KEYBOARD, Q, true, 0));
        router.poll_shortcuts(600);
        assert!(fired(&mut router).is_empty());
        let consumed = router.key_event(SEAT, KEYBOARD, Q, false, 700).consumed();
        assert!(consumed, "the declined press keeps its consumed release");
    }

    #[test]
    fn follows_chord_includes_an_open_chord_whose_declaration_was_removed() {
        let mut router = router_with(&[(SUPER_K, 0)]);
        key(&mut router, LEFT_SUPER, true, 0);
        key(&mut router, W, true, 0);
        key(&mut router, K, true, 10);
        assert_eq!(fired(&mut router), [SUPER_K]);
        router.set_action_lifecycles(&[]);
        key(&mut router, K, false, 20);
        key(&mut router, W, false, 20);
        // Super still holds SUPER_K's modifier-latched chord.
        let prefix = proposal(&mut router, W, true, 30).unwrap();
        assert!(prefix.follows_chord);
        let mut undeclared = super::router();
        key(&mut undeclared, LEFT_SUPER, true, 0);
        assert!(!proposal(&mut undeclared, W, true, 0).unwrap().follows_chord);
    }
}

mod boundaries {
    use super::*;

    /// Trusted cancellation never advances: work already due is dropped,
    /// not fired, and a leader ends cancelled.
    #[test]
    fn cancellation_drops_due_work_without_firing_it() {
        let cancels: [fn(&mut Keys); 4] = [
            |router| router.cancel_seat_chords(SEAT),
            |router| router.cancel_all_chords(),
            |router| {
                router.clear_seat(SEAT);
            },
            |router| router.remove_device(KEYBOARD),
        ];
        for cancel in cancels {
            let mut router = router_with(&[(FORCE, 0)]);
            key(&mut router, LEFT_SUPER, true, 0);
            key(&mut router, Q, true, 0);
            cancel(&mut router);
            router.poll_shortcuts(600);
            assert!(outputs(&mut router).is_empty());

            let mut router = router_with(&[(HINT, 0)]);
            key(&mut router, LEFT_SUPER, true, 0);
            key(&mut router, W, true, 0);
            outputs(&mut router);
            cancel(&mut router);
            router.poll_shortcuts(2000);
            assert_eq!(
                outputs(&mut router),
                [Out::Ended(PolicyChordEnd::Cancelled)]
            );
        }
    }

    #[test]
    fn an_epoch_reset_drops_pending_work_and_keeps_releases_paired() {
        let mut router = router_with(&[(HINT, 0)]);
        key(&mut router, LEFT_SUPER, true, 0);
        key(&mut router, Q, true, 0);
        router.reset_chords();
        router.poll_shortcuts(600);
        assert!(outputs(&mut router).is_empty());
        assert!(key(&mut router, Q, false, 700));
        router.set_action_lifecycles(&[PolicyActionLifecycleInterest {
            action: HINT,
            held_ms: 0,
        }]);
        key(&mut router, W, true, 800);
        outputs(&mut router);
        router.reset_chords();
        router.poll_shortcuts(5000);
        assert!(outputs(&mut router).is_empty());
        assert!(key(&mut router, W, false, 5000));
    }

    /// D2 review R2: the order shapes are declared in has no meaning. The
    /// same shapes in another order are the same registry, so a pending
    /// prefix and its open leader survive the replacement.
    #[test]
    fn reordered_declarations_keep_pending_work() {
        let build = |plan: &WmShortcutPlan| {
            WmShortcutRegistry::from_plan(
                plan,
                WmCapabilities::all_supported(),
                2,
                WmChromePolicy::default(),
            )
            .unwrap()
        };
        let mut reordered = plan();
        reordered.sequences.reverse();
        reordered.leaders.reverse();
        reordered.immediate.reverse();
        reordered.holds.reverse();
        assert_eq!(build(&reordered), build(&plan()));
        // Siblings under one root swapped while the roots keep their order.
        let mut siblings = plan();
        siblings.sequences.swap(0, 2);
        assert_eq!(build(&siblings), build(&plan()));

        for replacement in [reordered, siblings] {
            let mut router = router_with(&[(HINT, 0)]);
            key(&mut router, LEFT_SUPER, true, 0);
            key(&mut router, W, true, 0);
            assert_eq!(outputs(&mut router), [Out::Opened(HINT)]);
            router.replace_registry(build(&replacement));
            key(&mut router, LEFT_SUPER, false, 10);
            assert!(proposal(&mut router, K, true, 20).is_some());
            assert_eq!(
                outputs(&mut router),
                [Out::Fired(KILL), Out::Ended(PolicyChordEnd::Completed)]
            );
        }
    }

    #[test]
    fn the_same_plan_keeps_pending_work_and_another_cancels_it() {
        let registry = || {
            WmShortcutRegistry::from_plan(
                &plan(),
                WmCapabilities::all_supported(),
                2,
                WmChromePolicy::default(),
            )
            .unwrap()
        };
        let mut router = router();
        key(&mut router, LEFT_SUPER, true, 0);
        key(&mut router, Q, true, 0);
        router.replace_registry(registry());
        router.poll_shortcuts(500);
        assert_eq!(fired(&mut router), [FORCE]);

        let mut router = super::router();
        key(&mut router, LEFT_SUPER, true, 0);
        key(&mut router, Q, true, 0);
        let mut other = plan();
        other.timing.tap_ms = 300;
        router.replace_registry(
            WmShortcutRegistry::from_plan(
                &other,
                WmCapabilities::all_supported(),
                2,
                WmChromePolicy::default(),
            )
            .unwrap(),
        );
        router.poll_shortcuts(500);
        assert!(fired(&mut router).is_empty());
    }

    #[test]
    fn a_hold_due_before_a_held_is_emitted_first() {
        let mut router = router_with(&[(PLAIN, 600)]);
        key(&mut router, N, true, 0);
        key_on(&mut router, OTHER_SEAT, KEYBOARD, LEFT_SUPER, true, 0);
        key_on(&mut router, OTHER_SEAT, KEYBOARD, Q, true, 0);
        outputs(&mut router);
        router.poll_shortcuts(1000);
        assert_eq!(outputs(&mut router), [Out::Fired(FORCE), Out::Held]);
    }

    /// Late service settles due work by (deadline, creation), so a hold due
    /// before a Held is emitted first, and equal deadlines keep creation
    /// order.
    #[test]
    fn due_work_settles_by_deadline_then_creation() {
        let mut router = router_with(&[(PLAIN, 300), (HINT, 200)]);
        key(&mut router, N, true, 0);
        key_on(&mut router, OTHER_SEAT, KEYBOARD, LEFT_SUPER, true, 0);
        key_on(&mut router, OTHER_SEAT, KEYBOARD, Q, true, 0);
        key_on(
            &mut router,
            SeatId::from_raw(3),
            KEYBOARD,
            LEFT_SUPER,
            true,
            100,
        );
        key_on(&mut router, SeatId::from_raw(3), KEYBOARD, W, true, 100);
        assert_eq!(
            outputs(&mut router),
            [Out::Opened(PLAIN), Out::Opened(HINT)]
        );
        assert_eq!(router.next_deadline(), Some(300));
        // Due: PLAIN Held at 300 (created first), HINT Held at 300, FORCE
        // hold at 500, the sequence timeout at 1100.
        router.poll_shortcuts(2000);
        assert_eq!(
            outputs(&mut router),
            [
                Out::Held,
                Out::Held,
                Out::Fired(FORCE),
                Out::Ended(PolicyChordEnd::TimedOut),
            ]
        );
    }
}

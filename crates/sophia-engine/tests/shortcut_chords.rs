//! The engine half of the chord lifecycle (t277): when a declared action's
//! chord is held and how it ends, the eight credits, and the down ledger that
//! pairs every consumed press with its release. Session's correlation with
//! admitted Actions and queues is tested with Session.
use sophia_engine::*;
use sophia_protocol::{
    DeviceId, PolicyActionLifecycleInterest, PolicyChordEnd, SeatId, WmActionId,
    WmBindingRegistration, WmCapabilities, WmChromePolicy, WmModifierMask,
};

const SEAT: SeatId = SeatId::from_raw(1);
const KEYBOARD: DeviceId = DeviceId::from_raw(1);
const SECOND_KEYBOARD: DeviceId = DeviceId::from_raw(2);
const LEFT_ALT: u32 = 56;
const LEFT_SHIFT: u32 = 42;
const TAB: u32 = 15;
const F9: u32 = 67;
const KP_9: u32 = 73;
const KP_8: u32 = 72;
const NEXT: WmActionId = WmActionId::from_raw(186);
const PREVIOUS: WmActionId = WmActionId::from_raw(187);
const LAUNCH: WmActionId = WmActionId::from_raw(190);
const UNDECLARED: WmActionId = WmActionId::from_raw(191);

fn binding(action: WmActionId, keycode: u32, modifiers: u32) -> WmBindingRegistration {
    WmBindingRegistration {
        action,
        keycode,
        modifiers: WmModifierMask { bits: modifiers },
    }
}

/// These tests read as one call per key event, with chord events drained
/// separately. `Keys` adapts the two-phase API to that: each event is
/// accepted, its activation is the decision, and its chord events wait for
/// `drain_chord_events`.
struct Keys {
    router: WmShortcutRouter,
    events: Vec<WmChordEvent>,
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

impl Keys {
    fn absorb(&mut self, outputs: Vec<WmShortcutOutput>) -> Option<WmShortcutActivation> {
        let mut activation = None;
        for output in outputs {
            match output {
                WmShortcutOutput::Activation(fired) => {
                    assert!(
                        activation.replace(fired).is_none(),
                        "one activation per event"
                    );
                }
                WmShortcutOutput::Chord(event) => self.events.push(event),
            }
        }
        activation
    }

    fn decision(&mut self, consumed: bool, outputs: Vec<WmShortcutOutput>) -> WmShortcutDecision {
        let activation = self.absorb(outputs);
        WmShortcutDecision {
            action: activation.map(|activation| activation.action),
            consumed,
            chord: activation.and_then(|activation| activation.chord),
        }
    }

    fn route_key(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        now: u64,
    ) -> WmShortcutDecision {
        let event = self.router.key_event(seat, device, keycode, pressed, now);
        let consumed = event.consumed();
        let outputs = event.accept();
        self.decision(consumed, outputs)
    }

    fn observe_key(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
    ) -> WmShortcutDecision {
        let event = self
            .router
            .observe_key_event(seat, device, keycode, pressed, 0);
        let consumed = event.consumed();
        let outputs = event.accept();
        self.decision(consumed, outputs)
    }

    fn poll_chords(&mut self, now: u64) {
        self.router.poll_shortcuts(now);
        let outputs = self.router.take_outputs();
        assert!(self.absorb(outputs).is_none());
    }

    /// The buffer stands for events not yet drained, so a new epoch drops
    /// them as it drops the router's own.
    fn reset_chords(&mut self) {
        self.router.reset_chords();
        self.events.clear();
    }

    /// An undrained event of a refused opener goes with it, as in the router.
    fn chord_opener_refused(&mut self, token: WmChordToken) -> bool {
        self.events.retain(|event| match event {
            WmChordEvent::Held { token: owner } | WmChordEvent::Ended { token: owner, .. } => {
                *owner != token
            }
        });
        self.router.chord_opener_refused(token)
    }

    fn drain_chord_events(&mut self) -> Vec<WmChordEvent> {
        let outputs = self.router.take_outputs();
        assert!(self.absorb(outputs).is_none());
        core::mem::take(&mut self.events)
    }
}

/// Alt+Tab, Alt+Shift+Tab, F9 and keypad 9 for one unmodified action, and an
/// undeclared Alt+F9. Next and previous declare Held at 150 ms.
fn router() -> Keys {
    let registry = WmShortcutRegistry::new(
        &[
            binding(NEXT, TAB, WmModifierMask::ALT),
            binding(PREVIOUS, TAB, WmModifierMask::ALT | WmModifierMask::SHIFT),
            binding(LAUNCH, F9, 0),
            binding(LAUNCH, KP_9, 0),
            binding(UNDECLARED, F9, WmModifierMask::ALT),
            binding(LAUNCH, KP_8, WmModifierMask::ALT),
        ],
        WmCapabilities::all_supported(),
        1,
        WmChromePolicy::default(),
    )
    .unwrap();
    let mut router = Keys {
        router: WmShortcutRouter::new(registry),
        events: Vec::new(),
    };
    router.set_action_lifecycles(&[
        interest(NEXT, 150),
        interest(PREVIOUS, 150),
        interest(LAUNCH, 0),
    ]);
    router
}

fn interest(action: WmActionId, held_ms: u32) -> PolicyActionLifecycleInterest {
    PolicyActionLifecycleInterest { action, held_ms }
}

fn press(router: &mut Keys, keycode: u32, time: u64) -> WmShortcutDecision {
    router.route_key(SEAT, KEYBOARD, keycode, true, time)
}

fn release(router: &mut Keys, keycode: u32, time: u64) -> WmShortcutDecision {
    router.route_key(SEAT, KEYBOARD, keycode, false, time)
}

fn opened(decision: WmShortcutDecision) -> WmChordToken {
    let chord = decision.chord.expect("a declared action opens a chord");
    assert!(chord.opens);
    chord.token
}

fn ended(token: WmChordToken, end: PolicyChordEnd) -> WmChordEvent {
    WmChordEvent::Ended { token, end }
}

/// Alt held over Alt+Tab, then Shift+Tab: two chords, both released at the
/// last modifier's release whichever goes up first, in open order.
#[test]
fn chords_sharing_alt_end_together_at_the_last_modifier_in_either_order() {
    for alt_first in [false, true] {
        let mut router = router();
        press(&mut router, LEFT_ALT, 0);
        let next = opened(press(&mut router, TAB, 10));
        release(&mut router, TAB, 20);
        press(&mut router, LEFT_SHIFT, 30);
        let previous = opened(press(&mut router, TAB, 40));
        assert!(release(&mut router, TAB, 50).consumed);
        let (first, last) = if alt_first {
            (LEFT_ALT, LEFT_SHIFT)
        } else {
            (LEFT_SHIFT, LEFT_ALT)
        };
        release(&mut router, first, 60);
        assert!(
            router.drain_chord_events().is_empty(),
            "alt_first={alt_first}"
        );
        release(&mut router, last, 70);
        assert_eq!(
            router.drain_chord_events(),
            [
                ended(next, PolicyChordEnd::Released),
                ended(previous, PolicyChordEnd::Released)
            ],
            "alt_first={alt_first}"
        );
        assert!(router.shortcut_idle());
    }
}

#[test]
fn repeated_presses_of_one_action_join_its_open_chord() {
    let mut router = router();
    press(&mut router, LEFT_ALT, 0);
    let next = opened(press(&mut router, TAB, 0));
    for time in [10, 20, 30] {
        release(&mut router, TAB, time);
        let join = press(&mut router, TAB, time + 5);
        assert_eq!(join.action, Some(NEXT));
        assert_eq!(
            join.chord,
            Some(WmChordActivation {
                token: next,
                opens: false
            })
        );
    }
    release(&mut router, TAB, 40);
    release(&mut router, LEFT_ALT, 50);
    assert_eq!(
        router.drain_chord_events(),
        [ended(next, PolicyChordEnd::Released)]
    );
    assert_eq!(router.chord_credits_free(), WM_CHORD_CREDITS - 1);
}

#[test]
fn an_unmodified_tap_ends_released_on_its_trigger() {
    let mut router = router();
    let decision = press(&mut router, F9, 0);
    assert_eq!(decision.action, Some(LAUNCH));
    let launch = opened(decision);
    assert!(router.drain_chord_events().is_empty());
    assert!(release(&mut router, F9, 5).consumed);
    assert_eq!(
        router.drain_chord_events(),
        [ended(launch, PolicyChordEnd::Released)]
    );
}

/// The chord is held by every admitted trigger, on any keyboard of the seat.
#[test]
fn joining_triggers_on_other_keys_and_keyboards_extend_an_unmodified_chord() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    assert!(!press(&mut router, KP_9, 1).chord.unwrap().opens);
    assert!(
        !router
            .route_key(SEAT, SECOND_KEYBOARD, F9, true, 2)
            .chord
            .unwrap()
            .opens
    );
    release(&mut router, F9, 3);
    release(&mut router, KP_9, 4);
    assert!(router.drain_chord_events().is_empty());
    router.route_key(SEAT, SECOND_KEYBOARD, F9, false, 5);
    assert_eq!(
        router.drain_chord_events(),
        [ended(launch, PolicyChordEnd::Released)]
    );
}

#[test]
fn a_refused_join_trigger_stops_holding_the_chord() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    press(&mut router, KP_9, 1);
    // F9 still holds the chord, so the refusal ends nothing.
    assert_eq!(router.chord_join_refused(launch, KEYBOARD, KP_9), None);
    release(&mut router, F9, 2);
    assert_eq!(
        router.drain_chord_events(),
        [ended(launch, PolicyChordEnd::Released)]
    );
}

/// A modifier on one keyboard keeps the chord while the same modifier on
/// another goes up.
#[test]
fn a_modifier_held_on_one_keyboard_survives_its_release_on_another() {
    let mut router = router();
    press(&mut router, LEFT_ALT, 0);
    router.route_key(SEAT, SECOND_KEYBOARD, LEFT_ALT, true, 1);
    let next = opened(press(&mut router, TAB, 2));
    router.route_key(SEAT, SECOND_KEYBOARD, LEFT_ALT, false, 3);
    release(&mut router, TAB, 4);
    assert!(router.drain_chord_events().is_empty());
    release(&mut router, LEFT_ALT, 5);
    assert_eq!(
        router.drain_chord_events(),
        [ended(next, PolicyChordEnd::Released)]
    );
}

#[test]
fn held_is_sent_once_at_its_threshold_and_never_after_ended() {
    let mut router = router();
    press(&mut router, LEFT_ALT, 1000);
    let next = opened(press(&mut router, TAB, 1000));
    assert_eq!(router.next_deadline(), Some(1150));
    router.poll_chords(1149);
    assert!(router.drain_chord_events().is_empty());
    router.poll_chords(1150);
    router.poll_chords(1400);
    assert_eq!(
        router.drain_chord_events(),
        [WmChordEvent::Held { token: next }]
    );
    assert_eq!(router.next_deadline(), None);

    // A quick tap ends before its threshold; a late poll sends no Held.
    let mut router = self::router();
    press(&mut router, LEFT_ALT, 1000);
    let next = opened(press(&mut router, TAB, 1000));
    release(&mut router, TAB, 1010);
    release(&mut router, LEFT_ALT, 1020);
    router.poll_chords(5000);
    assert_eq!(
        router.drain_chord_events(),
        [ended(next, PolicyChordEnd::Released)]
    );
    assert_eq!(router.next_deadline(), None);

    // held_ms 0 asks for Ended only.
    let mut router = self::router();
    opened(press(&mut router, F9, 0));
    assert_eq!(router.next_deadline(), None);
    router.poll_chords(u64::MAX);
    assert!(router.drain_chord_events().is_empty());
}

#[test]
fn undeclared_actions_fire_as_before_with_no_chord() {
    let mut router = router();
    press(&mut router, LEFT_ALT, 0);
    let decision = press(&mut router, F9, 0);
    assert_eq!(decision.action, Some(UNDECLARED));
    assert!(decision.consumed && decision.chord.is_none());
    assert!(release(&mut router, F9, 1).consumed);
    assert_eq!(router.chord_credits_free(), WM_CHORD_CREDITS);
}

#[test]
fn a_repeated_or_duplicate_press_fires_nothing_and_joins_nothing() {
    let mut router = router();
    opened(press(&mut router, F9, 0));
    let repeat = press(&mut router, F9, 30);
    assert_eq!(repeat.action, None);
    assert!(repeat.consumed && repeat.chord.is_none());
    // An unbound key's repeat passes to the client as its press did.
    assert!(!press(&mut router, 30, 0).consumed);
    assert!(!press(&mut router, 30, 30).consumed);
}

/// Eight obligations, open or with Ended undelivered, take every credit. Only
/// a successful handoff returns one, and then exactly one opener is admitted.
#[test]
fn eight_credits_bound_open_chords_and_undelivered_ends() {
    let mut router = router();
    let mut tokens = Vec::new();
    for _ in 0..WM_CHORD_CREDITS {
        tokens.push(opened(press(&mut router, F9, 0)));
        release(&mut router, F9, 1);
    }
    assert_eq!(router.chord_credits_free(), 0);
    assert_eq!(router.drain_chord_events().len(), WM_CHORD_CREDITS);

    // A further opener is consumed as a shortcut but fires nothing.
    let refused = press(&mut router, F9, 2);
    assert_eq!(refused.action, None);
    assert!(refused.consumed && refused.chord.is_none());
    assert!(release(&mut router, F9, 3).consumed);
    assert!(router.drain_chord_events().is_empty());

    // Delivering an Ended returns exactly one credit, once.
    assert!(router.chord_delivered(tokens[0]));
    assert!(!router.chord_delivered(tokens[0]));
    assert_eq!(router.chord_credits_free(), 1);
    opened(press(&mut router, F9, 4));
    release(&mut router, F9, 5);
    assert_eq!(press(&mut router, F9, 6).action, None);
}

#[test]
fn an_open_chord_cannot_be_delivered_and_ending_early_frees_nothing() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    assert!(!router.chord_delivered(launch));
    router.cancel_seat_chords(SEAT);
    assert_eq!(
        router.drain_chord_events(),
        [ended(launch, PolicyChordEnd::Cancelled)]
    );
    assert_eq!(router.chord_credits_free(), WM_CHORD_CREDITS - 1);
    assert!(router.chord_delivered(launch));
    assert_eq!(router.chord_credits_free(), WM_CHORD_CREDITS);
}

#[test]
fn a_refused_opener_drops_its_chord_and_credit_silently() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    assert!(router.chord_opener_refused(launch));
    assert_eq!(router.chord_credits_free(), WM_CHORD_CREDITS);
    assert!(release(&mut router, F9, 1).consumed);
    assert!(router.drain_chord_events().is_empty());
}

/// Cancellation ends chords as cancelled, never released, and the old
/// trigger's release is still consumed.
#[test]
fn cancellation_ends_chords_cancelled_and_keeps_releases_paired() {
    for how in ["registry", "seat", "clear", "device"] {
        let mut router = router();
        press(&mut router, LEFT_ALT, 0);
        let next = opened(press(&mut router, TAB, 0));
        match how {
            "registry" => router.replace_registry(
                WmShortcutRegistry::new(
                    &[],
                    WmCapabilities::all_supported(),
                    2,
                    WmChromePolicy::default(),
                )
                .unwrap(),
            ),
            "seat" => router.cancel_seat_chords(SEAT),
            "clear" => assert!(router.clear_seat(SEAT)),
            _ => router.remove_device(KEYBOARD),
        }
        assert_eq!(
            router.drain_chord_events(),
            [ended(next, PolicyChordEnd::Cancelled)],
            "{how}"
        );
        let tab = release(&mut router, TAB, 1);
        release(&mut router, LEFT_ALT, 2);
        router.poll_chords(u64::MAX);
        assert!(router.drain_chord_events().is_empty(), "{how}");
        // Only bindings changed under the registry and the ledger survived.
        assert_eq!(tab.consumed, how == "registry" || how == "seat", "{how}");
    }
}

#[test]
fn a_flood_of_cancellations_frees_no_credit() {
    let mut router = router();
    for round in 0..WM_CHORD_CREDITS as u64 {
        press(&mut router, LEFT_ALT, round);
        opened(press(&mut router, TAB, round));
        router.cancel_seat_chords(SEAT);
        release(&mut router, TAB, round);
        release(&mut router, LEFT_ALT, round);
    }
    assert_eq!(router.chord_credits_free(), 0);
    press(&mut router, LEFT_ALT, 100);
    assert_eq!(press(&mut router, TAB, 100).action, None);
}

#[test]
fn a_new_epoch_drops_open_and_pending_chords_and_restores_credits() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    release(&mut router, F9, 1);
    press(&mut router, LEFT_ALT, 2);
    opened(press(&mut router, TAB, 2));
    router.reset_chords();
    assert!(router.drain_chord_events().is_empty());
    assert_eq!(router.chord_credits_free(), WM_CHORD_CREDITS);
    assert!(!router.chord_delivered(launch));
    // Interests belong to the next Configuration.
    release(&mut router, TAB, 3);
    assert!(press(&mut router, TAB, 4).chord.is_none());
    release(&mut router, LEFT_ALT, 5);
    assert!(router.drain_chord_events().is_empty());
}

/// An open chord keeps the row it opened under when the WM replaces its
/// Configuration; only chords opened afterwards follow the new rows.
#[test]
fn an_open_chord_keeps_the_configuration_it_opened_under() {
    let mut router = router();
    press(&mut router, LEFT_ALT, 0);
    let next = opened(press(&mut router, TAB, 0));
    router.set_action_lifecycles(&[interest(LAUNCH, 0)]);
    router.poll_chords(150);
    release(&mut router, TAB, 200);
    release(&mut router, LEFT_ALT, 210);
    assert_eq!(
        router.drain_chord_events(),
        [
            WmChordEvent::Held { token: next },
            ended(next, PolicyChordEnd::Released)
        ]
    );
    press(&mut router, LEFT_ALT, 300);
    assert!(press(&mut router, TAB, 300).chord.is_none());
}

/// Review R1: an open chord keeps its eligibility when the declaration goes.
#[test]
fn removing_a_declaration_leaves_its_open_chord_joinable() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    router.set_action_lifecycles(&[]);
    let join = press(&mut router, KP_9, 1);
    assert_eq!(join.action, Some(LAUNCH));
    assert_eq!(
        join.chord,
        Some(WmChordActivation {
            token: launch,
            opens: false
        })
    );
    release(&mut router, F9, 2);
    assert!(router.drain_chord_events().is_empty());
    release(&mut router, KP_9, 3);
    assert_eq!(
        router.drain_chord_events(),
        [ended(launch, PolicyChordEnd::Released)]
    );
    // A new chord follows the current, now empty, declarations.
    assert!(press(&mut router, F9, 4).chord.is_none());
}

/// Review R2: Session learns of an opener's refusal after routing, so the
/// chord may already be released, cancelled, or drained. The credit is
/// reclaimed once in every case, with no event left owed.
#[test]
fn a_refused_opener_is_reclaimed_after_release_cancel_or_drain() {
    for when in ["released", "cancelled", "drained"] {
        let mut router = router();
        let launch = opened(press(&mut router, F9, 0));
        match when {
            "released" => {
                release(&mut router, F9, 1);
            }
            "cancelled" => router.cancel_seat_chords(SEAT),
            _ => {
                release(&mut router, F9, 1);
                assert_eq!(router.drain_chord_events().len(), 1);
            }
        }
        assert!(router.chord_opener_refused(launch), "{when}");
        assert!(!router.chord_opener_refused(launch), "{when}");
        assert!(!router.chord_delivered(launch), "{when}");
        assert_eq!(router.chord_credits_free(), WM_CHORD_CREDITS, "{when}");
        assert!(router.drain_chord_events().is_empty(), "{when}");
    }
}

const MODIFIERS: [u32; 8] = [42, 54, 29, 97, 56, 100, 125, 126];

fn keyboard(raw: u64) -> DeviceId {
    DeviceId::from_raw(raw)
}

/// Four keyboards holding every modifier, as the review's reproductions do.
fn hold_all_modifiers(router: &mut Keys, pressed: bool) {
    for device in 10..14 {
        for keycode in MODIFIERS {
            router.route_key(SEAT, keyboard(device), keycode, pressed, 1);
        }
    }
}

/// Fill every device slot with a keyboard holding one ordinary key.
fn fill_device_slots(router: &mut Keys) {
    for device in 100..100 + WM_MAX_SHORTCUT_DEVICES as u64 {
        router.route_key(SEAT, keyboard(device), 30, true, 0);
    }
}

/// Review R3 (a): one physical key has one release, even after a duplicate
/// press, because the record keeps the key's identity.
#[test]
fn a_duplicate_modifier_press_leaves_no_phantom_hold() {
    let mut router = router();
    hold_all_modifiers(&mut router, true);
    let extra = keyboard(99);
    router.route_key(SEAT, extra, LEFT_ALT, true, 2);
    router.route_key(SEAT, extra, LEFT_ALT, true, 3);
    router.route_key(SEAT, extra, LEFT_ALT, false, 4);
    hold_all_modifiers(&mut router, false);
    assert!(router.shortcut_idle());
    assert_eq!(press(&mut router, F9, 5).action, Some(LAUNCH));
}

/// Review R3 (b): unplugging a keyboard drops its held modifier.
#[test]
fn removing_a_keyboard_removes_its_modifiers() {
    let mut router = router();
    hold_all_modifiers(&mut router, true);
    let extra = keyboard(99);
    router.route_key(SEAT, extra, LEFT_ALT, true, 2);
    router.remove_device(extra);
    hold_all_modifiers(&mut router, false);
    assert!(router.shortcut_idle());
    assert_eq!(press(&mut router, F9, 5).action, Some(LAUNCH));
}

/// Review R3 (c): a release for some other key, here a duplicate release of
/// one already up, cannot hide an Alt that is still down.
#[test]
fn an_unrelated_release_cannot_hide_a_held_modifier() {
    let mut router = router();
    hold_all_modifiers(&mut router, true);
    router.route_key(SEAT, keyboard(99), LEFT_ALT, true, 2);
    hold_all_modifiers(&mut router, false);
    router.route_key(SEAT, keyboard(10), LEFT_SHIFT, false, 5);
    assert_eq!(router.modifier_mask(SEAT).bits, WmModifierMask::ALT);
    // Alt is down, so F9 is Alt+F9, never the bare F9 binding.
    assert_eq!(press(&mut router, F9, 6).action, Some(UNDECLARED));
}

/// Review R3 (d): a press that went to a client stays the client's. A device
/// refused a slot stays refused by identity after a slot frees, so its
/// duplicate press is not a new shortcut and its release reaches the client.
#[test]
fn a_refused_press_stays_with_the_client_after_a_slot_frees() {
    let mut router = router();
    fill_device_slots(&mut router);
    let late = keyboard(99);
    let first = router.route_key(SEAT, late, F9, true, 1);
    assert!(first.action.is_none() && !first.consumed);
    router.route_key(SEAT, keyboard(100), 30, false, 2);
    let duplicate = router.route_key(SEAT, late, F9, true, 3);
    assert!(duplicate.action.is_none() && !duplicate.consumed);
    assert!(!router.route_key(SEAT, late, F9, false, 4).consumed);
    // An ordinary refused key leaves the mask known: other keyboards match.
    assert!(!router.seat_uncertain(SEAT));
    assert_eq!(
        router.route_key(SEAT, keyboard(101), F9, true, 5).action,
        Some(LAUNCH)
    );
}

/// A refused keyboard that presses a modifier makes the mask unknown, so the
/// seat matches nothing until that keyboard is unplugged; its own release is
/// not trusted, since its other presses were never seen.
#[test]
fn a_refused_modifier_suppresses_matching_until_its_keyboard_goes() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    fill_device_slots(&mut router);
    let late = keyboard(99);
    router.route_key(SEAT, late, LEFT_ALT, true, 1);
    assert!(router.seat_uncertain(SEAT));
    let blind = router.route_key(SEAT, keyboard(101), KP_9, true, 2);
    assert!(blind.action.is_none() && !blind.consumed);
    // The chord opened without modifiers still ends on its own trigger, and
    // its consumed press keeps its consumed release.
    assert!(release(&mut router, F9, 3).consumed);
    assert_eq!(
        router.drain_chord_events(),
        [ended(launch, PolicyChordEnd::Released)]
    );
    router.route_key(SEAT, late, LEFT_ALT, false, 4);
    assert!(router.seat_uncertain(SEAT));
    assert!(press(&mut router, F9, 5).action.is_none());
    release(&mut router, F9, 6);
    router.remove_device(late);
    assert!(!router.seat_uncertain(SEAT));
    assert_eq!(press(&mut router, F9, 7).action, Some(LAUNCH));
}

/// More refused keyboards than can be named latch the seat unknown; removing
/// devices cannot clear it, only the trusted seat reset does.
#[test]
fn a_saturated_seat_stays_unknown_until_it_is_reset() {
    let mut router = router();
    fill_device_slots(&mut router);
    for device in 200..200 + WM_MAX_SHORTCUT_DEVICES as u64 + 1 {
        router.route_key(SEAT, keyboard(device), 30, true, 0);
    }
    assert!(router.seat_uncertain(SEAT));
    for device in 200..200 + WM_MAX_SHORTCUT_DEVICES as u64 + 1 {
        router.remove_device(keyboard(device));
    }
    assert!(router.seat_uncertain(SEAT));
    assert!(!router.shortcut_idle());
    assert!(router.clear_seat(SEAT));
    assert!(!router.seat_uncertain(SEAT));
    assert_eq!(press(&mut router, F9, 1).action, Some(LAUNCH));
}

/// Keycodes past evdev's range are never bound: they pass both ways.
#[test]
fn keycodes_past_the_evdev_range_pass_untracked() {
    let mut router = router();
    assert!(!press(&mut router, 0x300, 0).consumed);
    assert!(!release(&mut router, 0x300, 1).consumed);
    assert!(router.shortcut_idle());
}

/// A refused keyboard's later modifier press makes the mask unknown too, not
/// only a modifier it was refused on.
#[test]
fn a_refused_keyboards_later_modifier_also_suppresses_matching() {
    let mut router = router();
    fill_device_slots(&mut router);
    let late = keyboard(99);
    router.route_key(SEAT, late, 30, true, 1);
    assert!(!router.seat_uncertain(SEAT));
    router.route_key(SEAT, late, LEFT_ALT, true, 2);
    assert!(router.seat_uncertain(SEAT));
    assert!(
        router
            .route_key(SEAT, keyboard(101), F9, true, 3)
            .action
            .is_none()
    );
}

/// Matching disabled (CursorOnly) still records presses and releases, so a
/// modifier pressed meanwhile is known on return, a duplicate is no new
/// action, and a press consumed before keeps its consumed release.
#[test]
fn observing_without_matching_keeps_the_record_true_across_modes() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    router.cancel_all_chords();
    assert_eq!(
        router.drain_chord_events(),
        [ended(launch, PolicyChordEnd::Cancelled)]
    );
    // CursorOnly: Alt goes down, F9 repeats, a fresh Tab is not matched.
    assert!(!router.observe_key(SEAT, KEYBOARD, LEFT_ALT, true).consumed);
    let repeat = router.observe_key(SEAT, KEYBOARD, F9, true);
    assert!(repeat.consumed && repeat.action.is_none());
    let tab = router.observe_key(SEAT, KEYBOARD, TAB, true);
    assert!(tab.action.is_none() && !tab.consumed);
    assert!(router.observe_key(SEAT, KEYBOARD, F9, false).consumed);
    assert!(!router.observe_key(SEAT, KEYBOARD, TAB, false).consumed);
    // Back to Full: Alt is still known to be down.
    assert_eq!(router.modifier_mask(SEAT).bits, WmModifierMask::ALT);
    assert_eq!(press(&mut router, F9, 1).action, Some(UNDECLARED));
    assert!(router.drain_chord_events().is_empty());
}

/// Equal bindings keep open chords but take the new registry's metadata;
/// changed bindings cancel them, keeping the record of keys down.
#[test]
fn a_registry_with_equal_bindings_keeps_chords_and_updates_metadata() {
    let mut router = router();
    press(&mut router, LEFT_ALT, 0);
    let next = opened(press(&mut router, TAB, 0));
    let same = |generation| {
        WmShortcutRegistry::new(
            &[
                binding(NEXT, TAB, WmModifierMask::ALT),
                binding(PREVIOUS, TAB, WmModifierMask::ALT | WmModifierMask::SHIFT),
                binding(LAUNCH, F9, 0),
                binding(LAUNCH, KP_9, 0),
                binding(UNDECLARED, F9, WmModifierMask::ALT),
                binding(LAUNCH, KP_8, WmModifierMask::ALT),
            ],
            WmCapabilities::all_supported(),
            generation,
            WmChromePolicy::default(),
        )
        .unwrap()
    };
    router.replace_registry(same(7));
    assert_eq!(router.policy_generation(), 7);
    assert!(router.drain_chord_events().is_empty());
    let changed = WmShortcutRegistry::new(
        &[binding(NEXT, TAB, WmModifierMask::ALT)],
        WmCapabilities::all_supported(),
        8,
        WmChromePolicy::default(),
    )
    .unwrap();
    router.replace_registry(changed);
    assert_eq!(router.policy_generation(), 8);
    assert_eq!(
        router.drain_chord_events(),
        [ended(next, PolicyChordEnd::Cancelled)]
    );
    assert!(release(&mut router, TAB, 1).consumed);
}

/// D2 review R1, through the single-call API: a join pressed with Alt still
/// holds the unmodified opener's trigger-held chord by its own key.
#[test]
fn a_modified_join_holds_an_unmodified_openers_chord() {
    let mut router = router();
    let launch = opened(press(&mut router, F9, 0));
    press(&mut router, LEFT_ALT, 10);
    let join = press(&mut router, KP_8, 20);
    assert_eq!(join.action, Some(LAUNCH));
    assert_eq!(
        join.chord,
        Some(WmChordActivation {
            token: launch,
            opens: false
        })
    );
    release(&mut router, F9, 30);
    assert!(router.drain_chord_events().is_empty());
    release(&mut router, KP_8, 40);
    assert_eq!(
        router.drain_chord_events(),
        [ended(launch, PolicyChordEnd::Released)]
    );
}

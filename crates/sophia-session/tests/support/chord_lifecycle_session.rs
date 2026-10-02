use super::*;
use sophia_engine::{WM_CHORD_CREDITS, WmChordActivation, WmShortcutRegistry, WmShortcutRouter};
use sophia_protocol::{
    DeviceId, PolicyActionLifecycleInterest, PolicyActionRegistration, PolicyChordEnd,
    PolicyChordPhase, PolicyRequestCause, SeatId, WmBindingRegistration, WmCapabilities,
    WmModifierMask,
};

// Session's half of the chord lifecycle (t277): correlating admitted Actions
// with the router's chords, the order and bounds of their causes in the public
// queue, and credit return on handoff only. Routing and capture precedence
// are exercised through the router the owner loop drives; the owner loop's
// macro dispatch delegates to the methods tested here.

const NEXT: WmActionId = WmActionId::from_raw(186);
const PLAIN: WmActionId = WmActionId::from_raw(187);
const SEAT: SeatId = SeatId::from_raw(1);
const KEYBOARD: DeviceId = DeviceId::from_raw(1);
const ALT: u32 = 56;
const TAB: u32 = 15;
const F9: u32 = 67;

struct Chords {
    fixture: ReloadFixture,
    layout: PersistentLiveLayout,
    output: sophia_engine::HeadlessOutput,
}

fn registry(generation: u64, bindings: &[(WmActionId, u32, u32)]) -> WmShortcutRegistry {
    let bindings = bindings
        .iter()
        .map(|&(action, keycode, modifiers)| WmBindingRegistration {
            action,
            keycode,
            modifiers: WmModifierMask { bits: modifiers },
        })
        .collect::<Vec<_>>();
    WmShortcutRegistry::new(
        &bindings,
        WmCapabilities::all_supported(),
        generation,
        sophia_protocol::WmChromePolicy::default(),
    )
    .unwrap()
}

/// Alt+Tab is NEXT, followed as a chord with Held at 150 ms; F9 is PLAIN.
fn chords() -> Chords {
    let mut fixture = ReloadFixture::new();
    let public = fixture.wm.public.as_mut().unwrap();
    public.configured = true;
    public.actions = [(NEXT, "recent-window-next"), (PLAIN, "plain")]
        .map(|(action, name)| PolicyActionRegistration {
            action,
            name: name.into(),
            session_operation_slot: None,
        })
        .to_vec();
    let mut router = WmShortcutRouter::new(registry(
        1,
        &[(NEXT, TAB, WmModifierMask::ALT), (PLAIN, F9, 0)],
    ));
    router.set_action_lifecycles(&[PolicyActionLifecycleInterest {
        action: NEXT,
        held_ms: 150,
    }]);
    fixture.wm.shortcuts = Some(router);
    Chords {
        fixture,
        layout: PersistentLiveLayout::default(),
        output: sophia_engine::HeadlessOutput::deterministic(),
    }
}

impl Chords {
    fn router(&mut self) -> &mut WmShortcutRouter {
        self.fixture.wm.shortcuts.as_mut().unwrap()
    }

    fn key(&mut self, keycode: u32, pressed: bool, now: u64) -> sophia_engine::WmShortcutDecision {
        self.router().route_key(SEAT, KEYBOARD, keycode, pressed, now)
    }

    /// Admit a routed activation the way the owner loop's dispatch does.
    fn admit(&mut self, decision: sophia_engine::WmShortcutDecision, keycode: u32) -> LiveChordActionAdmission {
        let action = decision.action.expect("a shortcut fired");
        let chord = decision.chord.expect("a followed chord");
        self.fixture
            .wm
            .admit_chord_action(action, chord, KEYBOARD, keycode, &self.layout, self.output)
            .unwrap()
    }

    fn plain(&mut self) {
        assert!(matches!(
            self.fixture.wm.enqueue_action(PLAIN, &self.layout, self.output).unwrap(),
            LiveOrderedWmActionAdmission::Admitted { .. }
        ));
    }

    /// Queue every lifecycle event the router reported, in order.
    fn events(&mut self) -> Vec<LiveChordEventAdmission> {
        let events = self.router().drain_chord_events();
        events
            .into_iter()
            .map(|event| self.fixture.wm.enqueue_chord_event(event).unwrap())
            .collect()
    }

    fn causes(&self) -> Vec<PolicyRequestCause> {
        let public = self.fixture.wm.public.as_ref().unwrap();
        public.queue.iter().map(|cause| cause.cause).collect()
    }

    fn fill_queue_to(&mut self, length: usize) {
        while self.fixture.wm.public.as_ref().unwrap().queue.len() < length {
            self.plain();
        }
    }
}

fn serial(cause: PolicyRequestCause) -> u64 {
    match cause {
        PolicyRequestCause::Action { activation_serial, .. }
        | PolicyRequestCause::ActionLifecycle { activation_serial, .. } => activation_serial,
        other => panic!("not an action cause: {other:?}"),
    }
}

fn lifecycle(cause: PolicyRequestCause) -> (PolicyChordPhase, u32) {
    let PolicyRequestCause::ActionLifecycle { action, phase, count, .. } = cause else {
        panic!("not a lifecycle cause: {cause:?}");
    };
    assert_eq!(action, NEXT);
    (phase, count)
}

/// Alt held over Tab twice, then F9 ordinary, then Held and Ended: every
/// cause keeps the order of its event, names the chord's first serial, and
/// counts its admitted Actions.
#[test]
fn lifecycle_causes_follow_their_actions_in_event_order() {
    let mut c = chords();
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    c.key(TAB, false, 10);
    let join = c.key(TAB, true, 20);
    assert_eq!(c.admit(join, TAB), LiveChordActionAdmission::Admitted);
    c.plain();
    c.router().poll_chords(150);
    assert_eq!(c.events(), [LiveChordEventAdmission::Queued]);
    c.key(TAB, false, 200);
    c.key(ALT, false, 210);
    assert_eq!(c.events(), [LiveChordEventAdmission::Queued]);
    let causes = c.causes();
    assert_eq!(causes.len(), 5);
    let first = serial(causes[0]);
    assert!(matches!(causes[1], PolicyRequestCause::Action { action: NEXT, .. }));
    assert!(matches!(causes[2], PolicyRequestCause::Action { action: PLAIN, .. }));
    assert_eq!(serial(causes[3]), first);
    assert_eq!(lifecycle(causes[3]), (PolicyChordPhase::Held, 2));
    assert_eq!(serial(causes[4]), first);
    assert_eq!(
        lifecycle(causes[4]),
        (PolicyChordPhase::Ended(PolicyChordEnd::Released), 2)
    );
    assert!(c.fixture.wm.chord_ledger.is_empty());
}

/// An opener the public queue refuses takes its chord with it: its join is
/// discarded rather than delivered as an ordinary Action, the credit returns,
/// and an Ended drained for it later is discarded too.
#[test]
fn a_refused_opener_discards_its_joins_and_terminal() {
    let mut c = chords();
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    c.key(TAB, false, 1);
    let join = c.key(TAB, true, 2);
    let token = opener.chord.unwrap().token;
    c.fill_queue_to(WM_OWNER_REQUEST_CAPACITY);
    assert!(matches!(
        c.admit(opener, TAB),
        LiveChordActionAdmission::RejectedCapacity { .. }
    ));
    assert_eq!(c.router().chord_credits_free(), WM_CHORD_CREDITS);
    c.fixture.wm.public.as_mut().unwrap().queue.clear();
    // The join was routed while the opener's chord was open; its opener was
    // never admitted, so it is discarded with the chord.
    assert_eq!(
        join.chord,
        Some(WmChordActivation {
            token,
            opens: false
        })
    );
    assert_eq!(c.admit(join, TAB), LiveChordActionAdmission::Discarded);
    assert!(c.causes().is_empty());
    assert_eq!(
        c.fixture.wm.enqueue_chord_event(sophia_engine::WmChordEvent::Ended {
            token,
            end: PolicyChordEnd::Released,
        })
        .unwrap(),
        LiveChordEventAdmission::Discarded
    );
    assert!(c.causes().is_empty());
}

#[test]
fn an_unconfigured_policy_withholds_the_opener_and_returns_its_credit() {
    let mut c = chords();
    c.fixture.wm.public.as_mut().unwrap().configured = false;
    assert!(c.fixture.wm.holds_physical_inputs());
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Withheld);
    assert_eq!(c.router().chord_credits_free(), WM_CHORD_CREDITS);
    assert!(c.causes().is_empty());
    // A dead transport holds nothing: its inputs are refused instead.
    c.fixture.wm.public.as_mut().unwrap().transport_unavailable = true;
    assert!(!c.fixture.wm.holds_physical_inputs());
    c.fixture.wm.public.as_mut().unwrap().transport_unavailable = false;
    c.fixture.wm.degraded = true;
    assert!(!c.fixture.wm.holds_physical_inputs());
}

/// Ended enters a full queue, behind everything before it; Held does not,
/// and its chord still ends.
#[test]
fn ended_passes_the_request_bound_but_no_earlier_cause_and_held_does_not() {
    let mut c = chords();
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    c.fill_queue_to(WM_OWNER_REQUEST_CAPACITY);
    c.router().poll_chords(150);
    assert_eq!(c.events(), [LiveChordEventAdmission::HeldDropped]);
    c.key(TAB, false, 200);
    c.key(ALT, false, 210);
    assert_eq!(c.events(), [LiveChordEventAdmission::Queued]);
    let causes = c.causes();
    assert_eq!(causes.len(), WM_OWNER_REQUEST_CAPACITY + 1);
    assert_eq!(
        lifecycle(*causes.last().unwrap()),
        (PolicyChordPhase::Ended(PolicyChordEnd::Released), 1)
    );
}

/// The credit returns only once Ended is handed off as the in-flight Cycle,
/// never on queueing, and not when the handoff fails.
#[test]
fn a_credit_returns_only_after_a_successful_handoff() {
    for handoff in [false, true] {
        let mut c = chords();
        c.key(ALT, true, 0);
        let opener = c.key(TAB, true, 0);
        assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
        c.key(TAB, false, 1);
        c.key(ALT, false, 2);
        assert_eq!(c.events(), [LiveChordEventAdmission::Queued]);
        // The opener's own Cycle is not under test here.
        c.fixture.wm.public.as_mut().unwrap().queue.pop_front();
        assert_eq!(c.router().chord_credits_free(), WM_CHORD_CREDITS - 1);
        if handoff {
            let (worker, commands, _events) =
                policy_transport_worker::worker_capture::capturing_worker();
            c.fixture.wm.public.as_mut().unwrap().worker = Some(worker);
            assert!(
                c.fixture
                    .wm
                    .poll_request(&mut c.layout, c.output, true)
                    .unwrap()
                    .is_none()
            );
            let policy_transport_worker::PolicyTransportCommand::Cycle { request, .. } =
                commands.try_recv().expect("the Ended issues a cycle")
            else {
                panic!("the first command is the cycle");
            };
            assert!(matches!(
                request.cause,
                PolicyRequestCause::ActionLifecycle {
                    phase: PolicyChordPhase::Ended(PolicyChordEnd::Released),
                    ..
                }
            ));
            assert_eq!(c.router().chord_credits_free(), WM_CHORD_CREDITS);
        } else {
            // The handoff itself fails: the worker's command queue is gone.
            let (worker, commands, _events) =
                policy_transport_worker::worker_capture::capturing_worker();
            drop(commands);
            c.fixture.wm.public.as_mut().unwrap().worker = Some(worker);
            let Err(error) = c.fixture.wm.poll_request(&mut c.layout, c.output, true) else {
                panic!("a failed handoff is an error");
            };
            assert_eq!(error.to_string(), "public WM cycle queue is busy");
            assert_eq!(c.router().chord_credits_free(), WM_CHORD_CREDITS - 1);
        }
    }
}

/// A security cancel may pass queued chord causes; it never reorders them
/// among themselves and takes or returns no credit.
#[test]
fn a_security_cancel_passes_chord_causes_without_reordering_them() {
    let mut c = chords();
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    c.router().poll_chords(150);
    c.key(TAB, false, 160);
    c.key(ALT, false, 170);
    c.events();
    let before = c.causes();
    let credits = c.router().chord_credits_free();
    let surface = SurfaceId::new(9, 1);
    c.fixture.wm.public.as_mut().unwrap().queue_security_cancel(LivePublicPolicyCause {
        source: LiveWmProposalSource::Focus(surface),
        cause: PolicyRequestCause::Interaction {
            phase: sophia_protocol::PolicyInteractionPhase::Cancel,
            kind: sophia_protocol::PolicyInteractionKind::Move,
            axis: sophia_protocol::PolicyInteractionAxis::None,
            target: surface,
            geometry: Rect { x: 0, y: 0, width: 1, height: 1 },
        },
        affected_outputs: vec![c.output.id],
    });
    let after = c.causes();
    assert!(matches!(after[0], PolicyRequestCause::Interaction { .. }));
    assert_eq!(after[1..], before[..]);
    assert_eq!(c.router().chord_credits_free(), credits);
}

/// A reload keeps the router: equal bindings keep the open chord and take the
/// new metadata; changed bindings end it cancelled, and its Ended still
/// reaches the queue for the opener admitted under the old configuration.
#[test]
fn reloading_shortcuts_keeps_the_router_and_its_owed_chords() {
    let mut c = chords();
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    let interests = [PolicyActionLifecycleInterest { action: NEXT, held_ms: 150 }];
    c.fixture.wm.install_shortcuts(
        registry(2, &[(NEXT, TAB, WmModifierMask::ALT), (PLAIN, F9, 0)]),
        &interests,
    );
    assert_eq!(c.router().policy_generation(), 2);
    assert!(c.events().is_empty());
    assert_eq!(c.router().chord_credits_free(), WM_CHORD_CREDITS - 1);
    c.fixture
        .wm
        .install_shortcuts(registry(3, &[(PLAIN, F9, 0)]), &[]);
    assert_eq!(c.events(), [LiveChordEventAdmission::Queued]);
    assert_eq!(
        lifecycle(*c.causes().last().unwrap()),
        (PolicyChordPhase::Ended(PolicyChordEnd::Cancelled), 1)
    );
    // The consumed Tab press keeps its consumed release across the reload.
    assert!(c.key(TAB, false, 1).consumed);
}

/// The per-turn service runs whatever physical input is doing: a Held that
/// falls due while input is suppressed, and a cancellation left by a seat
/// reset with no poller, both come out in order, and the next deadline clears.
#[test]
fn the_chord_service_delivers_timers_and_cancellations_without_physical_input() {
    let mut c = chords();
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    let token = opener.chord.unwrap().token;
    assert_eq!(c.router().next_deadline(), Some(150));
    assert!(c.fixture.wm.service_chords(149).is_empty());
    assert!(c.router().clear_seat(SEAT));
    assert_eq!(
        c.fixture.wm.service_chords(150),
        [
            sophia_engine::WmChordEvent::Ended { token, end: PolicyChordEnd::Cancelled },
        ]
    );
    assert_eq!(c.router().next_deadline(), None);

    let mut c = chords();
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    let token = opener.chord.unwrap().token;
    assert_eq!(
        c.fixture.wm.service_chords(150),
        [sophia_engine::WmChordEvent::Held { token }]
    );
    assert_eq!(c.router().next_deadline(), None);
    assert!(c.fixture.wm.service_chords(10_000).is_empty());
}

/// Review R3: leaving keyboard matching ends the open chords cancelled on the
/// transition itself, once, with no further key or poller; no Held follows,
/// and matching resumes normally.
#[test]
fn leaving_keyboard_matching_cancels_open_chords_once() {
    let mut c = chords();
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    let token = opener.chord.unwrap().token;
    c.fixture.wm.observe_keyboard_matching(false);
    assert_eq!(
        c.fixture.wm.service_chords(10),
        [sophia_engine::WmChordEvent::Ended { token, end: PolicyChordEnd::Cancelled }]
    );
    c.fixture.wm.observe_keyboard_matching(false);
    assert!(c.fixture.wm.service_chords(1_000).is_empty());
    c.fixture.wm.observe_keyboard_matching(true);
    c.key(TAB, false, 1_001);
    let reopened = c.key(TAB, true, 1_002);
    assert!(reopened.chord.is_some_and(|chord| chord.opens && chord.token != token));
}

/// Review R4: a configured policy that is degraded, whose transport is
/// unavailable, or that is restarting cannot spend a credit; the opener is
/// withheld and its credit returns.
#[test]
fn a_configured_but_dead_policy_withholds_chord_openers() {
    for state in ["degraded", "unavailable", "restarting"] {
        let mut c = chords();
        match state {
            "degraded" => c.fixture.wm.degraded = true,
            "unavailable" => c.fixture.wm.public.as_mut().unwrap().transport_unavailable = true,
            _ => {
                let (_sender, completion) = std::sync::mpsc::sync_channel(1);
                c.fixture.wm.control_restart = Some(ControlRestartJob {
                    completion,
                    lifetime: ControlProcessLifetime { stop: None, thread: None },
                });
            }
        }
        assert!(c.fixture.wm.public.as_ref().unwrap().configured, "{state}");
        c.key(ALT, true, 0);
        let opener = c.key(TAB, true, 0);
        assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Withheld, "{state}");
        assert_eq!(c.router().chord_credits_free(), WM_CHORD_CREDITS, "{state}");
        assert!(c.causes().is_empty(), "{state}");
    }
}

const LIFECYCLE: u64 = sophia_protocol::SOPHIA_WM_CAPABILITY_ACTIONS
    | sophia_protocol::SOPHIA_WM_CAPABILITY_CONFIGURATION
    | sophia_protocol::SOPHIA_WM_CAPABILITY_ACTION_LIFECYCLE;
const CHORD_ACTIONS: u64 = LIFECYCLE | sophia_protocol::SOPHIA_WM_CAPABILITY_CHORD_ACTIONS;
const SECOND_SEAT: SeatId = SeatId::from_raw(2);

impl Chords {
    fn select(&mut self, capabilities: u64) {
        self.fixture.wm.public.as_mut().unwrap().selected_capabilities = capabilities;
    }

    fn key_on(&mut self, seat: SeatId, keycode: u32, pressed: bool, now: u64) -> sophia_engine::WmShortcutDecision {
        self.router().route_key(seat, KEYBOARD, keycode, pressed, now)
    }
}

fn chord_action(cause: PolicyRequestCause) -> (u64, u64) {
    let PolicyRequestCause::ChordAction { activation_serial, chord_serial, action } = cause else {
        panic!("not a ChordAction: {cause:?}");
    };
    assert_eq!(action, NEXT);
    (activation_serial, chord_serial)
}

/// With chord_actions, the opener names itself, a join names the opener, and
/// Held and Ended name that same chord serial.
#[test]
fn chord_actions_name_their_activation_and_their_chord() {
    let mut c = chords();
    c.select(CHORD_ACTIONS);
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    c.key(TAB, false, 1);
    let join = c.key(TAB, true, 2);
    assert_eq!(c.admit(join, TAB), LiveChordActionAdmission::Admitted);
    c.router().poll_chords(150);
    c.events();
    c.key(TAB, false, 160);
    c.key(ALT, false, 170);
    c.events();
    let causes = c.causes();
    let (first, chord) = chord_action(causes[0]);
    assert_eq!(first, chord);
    let (second, joined) = chord_action(causes[1]);
    assert_ne!(second, first);
    assert_eq!(joined, chord);
    assert_eq!(serial(causes[2]), chord);
    assert_eq!(lifecycle(causes[2]), (PolicyChordPhase::Held, 2));
    assert_eq!(serial(causes[3]), chord);
    assert_eq!(
        lifecycle(causes[3]),
        (PolicyChordPhase::Ended(PolicyChordEnd::Released), 2)
    );
}

/// The same action on two seats makes two chords with distinct chord serials,
/// each terminal naming its own.
#[test]
fn chords_of_one_action_on_two_seats_keep_their_own_identity() {
    let mut c = chords();
    c.select(CHORD_ACTIONS);
    c.key(ALT, true, 0);
    let first = c.key(TAB, true, 0);
    assert_eq!(c.admit(first, TAB), LiveChordActionAdmission::Admitted);
    c.key_on(SECOND_SEAT, ALT, true, 1);
    let second = c.key_on(SECOND_SEAT, TAB, true, 1);
    assert!(second.chord.unwrap().opens);
    assert_eq!(c.admit(second, TAB), LiveChordActionAdmission::Admitted);
    c.key_on(SECOND_SEAT, TAB, false, 2);
    c.key_on(SECOND_SEAT, ALT, false, 3);
    c.events();
    let causes = c.causes();
    let (_, one) = chord_action(causes[0]);
    let (_, two) = chord_action(causes[1]);
    assert_ne!(one, two);
    assert_eq!(serial(causes[2]), two);
    assert_eq!(
        lifecycle(causes[2]),
        (PolicyChordPhase::Ended(PolicyChordEnd::Released), 1)
    );
}

/// A peer that selected the lifecycle alone keeps its contract: plain Action,
/// then Held and Ended naming the first Action's serial.
#[test]
fn a_lifecycle_only_peer_still_receives_plain_actions() {
    let mut c = chords();
    c.select(LIFECYCLE);
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    c.key(TAB, false, 1);
    c.key(ALT, false, 2);
    c.events();
    let causes = c.causes();
    let PolicyRequestCause::Action { activation_serial, action: NEXT } = causes[0] else {
        panic!("expected a plain Action: {:?}", causes[0]);
    };
    assert_eq!(serial(causes[1]), activation_serial);
    assert!(causes.iter().all(|cause| !matches!(cause, PolicyRequestCause::ChordAction { .. })));
}

/// An ordinary invocation of a followed action while its chord is held stays
/// an ordinary Action: it joins nothing and owes no terminal.
#[test]
fn an_ordinary_invocation_during_a_held_chord_stays_ordinary() {
    let mut c = chords();
    c.select(CHORD_ACTIONS);
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    assert!(matches!(
        c.fixture.wm.enqueue_action(NEXT, &c.layout, c.output).unwrap(),
        LiveOrderedWmActionAdmission::Admitted { .. }
    ));
    c.key(TAB, false, 1);
    c.key(ALT, false, 2);
    c.events();
    let causes = c.causes();
    let (_, chord) = chord_action(causes[0]);
    assert!(matches!(causes[1], PolicyRequestCause::Action { action: NEXT, .. }));
    assert_eq!(serial(causes[2]), chord);
    assert_eq!(
        lifecycle(causes[2]),
        (PolicyChordPhase::Ended(PolicyChordEnd::Released), 1)
    );
}

/// The engine accepts a ChordAction and Session hands it off as a Cycle.
#[test]
fn a_chord_action_is_handed_off_as_a_cycle() {
    let mut c = chords();
    c.select(CHORD_ACTIONS);
    c.key(ALT, true, 0);
    let opener = c.key(TAB, true, 0);
    assert_eq!(c.admit(opener, TAB), LiveChordActionAdmission::Admitted);
    let (worker, commands, _events) = policy_transport_worker::worker_capture::capturing_worker();
    c.fixture.wm.public.as_mut().unwrap().worker = Some(worker);
    assert!(c.fixture.wm.poll_request(&mut c.layout, c.output, true).unwrap().is_none());
    let policy_transport_worker::PolicyTransportCommand::Cycle { request, .. } =
        commands.try_recv().expect("the chord action issues a cycle")
    else {
        panic!("the first command is the cycle");
    };
    let (activation, chord) = chord_action(request.cause);
    assert_eq!(activation, chord);
}

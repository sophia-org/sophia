//! Physical shortcut matching, owed to no protocol revision.
//!
//! Engine matches physical input against registered chords and emits opaque action
//! tokens. The public `sophia_wm_v1` path resolves these bindings from the
//! prepared shortcut and policy catalogs before they arrive here.

use crate::prelude::*;
use sophia_protocol::{
    PolicyActionLifecycleInterest, WmActionId, WmBindingRegistration, WmCapabilities,
    WmChromePolicy, WmModifierMask,
};

mod chord;
mod deferred;
mod ledger;
mod plan;
use chord::ChordBook;
pub use chord::{
    WM_CHORD_CREDITS, WmChordActivation, WmChordCreditsExhausted, WmChordEvent, WmChordToken,
};
use deferred::Pending;
pub use deferred::{
    WmKeyEvent, WmPressKind, WmPressProposal, WmShortcutActivation, WmShortcutOutput,
};
use ledger::WmSeatShortcutState;
use plan::Shapes;
pub use plan::{
    WmHoldBinding, WmKeyStep, WmModifierTapBinding, WmSequenceBinding, WmSequenceLeader,
    WmShortcutPlan, WmShortcutTiming,
};

/// Why a set of bindings could not become a registry.
///
/// A `&'static str` rather than an enum because callers reduce every cause to one
/// preparation message. A parallel enum would duplicate this vocabulary without
/// giving anyone another decision to make.
pub type WmShortcutRegistryError = &'static str;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmShortcutRegistry {
    shapes: Shapes,
    held: BTreeMap<u32, WmActionId>,
    pub(crate) capabilities: WmCapabilities,
    policy_generation: u64,
    chrome: WmChromePolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmShortcutDecision {
    pub action: Option<WmActionId>,
    pub consumed: bool,
    /// The lifecycle chord `action` opened or joined, when the WM declared it.
    pub chord: Option<WmChordActivation>,
}

impl WmShortcutDecision {
    const fn pass() -> Self {
        Self {
            action: None,
            consumed: false,
            chord: None,
        }
    }
}

impl WmShortcutRegistry {
    /// Builds a registry from immediate bindings a caller already resolved.
    pub fn new(
        bindings: &[WmBindingRegistration],
        capabilities: WmCapabilities,
        policy_generation: u64,
        chrome: WmChromePolicy,
    ) -> Result<Self, WmShortcutRegistryError> {
        Self::from_plan(
            &WmShortcutPlan {
                immediate: bindings.to_vec(),
                ..WmShortcutPlan::default()
            },
            capabilities,
            policy_generation,
            chrome,
        )
    }

    /// Builds a registry from every shortcut shape a caller resolved.
    ///
    /// Every rejection here is about the shapes themselves, not about who
    /// asked: an invalid action or keycode, an unsupported modifier, a
    /// reserved chord (emergency recovery, or virtual-terminal switching) in
    /// any step, a duplicate or ambiguous shape, a misplaced or reused
    /// leader, timing out of range, or more entries than the wire admits. A
    /// caller that speaks a protocol revision checks its own version first.
    pub fn from_plan(
        plan: &WmShortcutPlan,
        capabilities: WmCapabilities,
        policy_generation: u64,
        chrome: WmChromePolicy,
    ) -> Result<Self, WmShortcutRegistryError> {
        if capabilities.bits & !WmCapabilities::SUPPORTED != 0 {
            return Err("unsupported WM capability");
        }
        if policy_generation == 0 {
            return Err("invalid WM policy generation");
        }
        if !valid_chrome_policy(chrome) {
            return Err("invalid WM chrome policy");
        }
        Ok(Self {
            shapes: Shapes::build(plan)?,
            held: BTreeMap::new(),
            capabilities,
            policy_generation,
            chrome,
        })
    }

    pub fn handle_key(
        &mut self,
        keycode: u32,
        modifiers: WmModifierMask,
        pressed: bool,
    ) -> WmShortcutDecision {
        if !pressed {
            return WmShortcutDecision {
                consumed: self.held.remove(&keycode).is_some(),
                ..WmShortcutDecision::pass()
            };
        }
        let Some(&action) = self.shapes.immediate.get(&(keycode, modifiers.bits)) else {
            return WmShortcutDecision::pass();
        };
        let first_press = self.held.insert(keycode, action).is_none();
        WmShortcutDecision {
            action: first_press.then_some(action),
            consumed: true,
            chord: None,
        }
    }

    /// Every bound shape: immediate chords, holds, taps, sequences, leaders.
    pub fn binding_count(&self) -> usize {
        self.shapes.count
    }

    pub const fn policy_generation(&self) -> u64 {
        self.policy_generation
    }

    pub const fn chrome(&self) -> WmChromePolicy {
        self.chrome
    }

    pub const fn supports_chrome_policy(&self) -> bool {
        self.capabilities.bits & WmCapabilities::POLICY_CHROME_V2 != 0
    }

    pub fn is_idle(&self) -> bool {
        self.held.is_empty()
    }
}

pub const WM_MAX_SHORTCUT_SEATS: usize = 16;
/// Devices tracked at once per seat while they have keys down. A device's
/// slot frees when its last key goes up.
pub const WM_MAX_SHORTCUT_DEVICES: usize = 16;

/// Matches physical keys for every seat and follows the WM's declared chords.
///
/// Each seat records exactly which keys are down on which device, so two
/// keyboards are told apart, a repeat or duplicate press fires nothing, and
/// the release of every consumed press is consumed too. The modifier mask is
/// derived from those records.
///
/// The record is bounded by device, never by an anonymous count. A device
/// that arrives while every slot is taken is refused by identity: its keys
/// pass unmatched and unconsumed until it is removed or the seat is reset. If
/// it pressed a modifier the mask is unknown, so the seat matches nothing
/// until then. Refusing more devices than there are slots latches the seat
/// unknown until `clear_seat`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmShortcutRouter {
    pub(crate) registry: WmShortcutRegistry,
    seats: BTreeMap<SeatId, WmSeatShortcutState>,
    chords: ChordBook,
    /// At most one undecided tap, hold or sequence per seat.
    pending: BTreeMap<SeatId, Pending>,
    /// Activations and chord events not yet taken, in order.
    outbox: Vec<WmShortcutOutput>,
    /// Orders equal deadlines: every pending decision and every chord gets
    /// the next number when it is created.
    next_created: u64,
}

impl WmShortcutRouter {
    pub fn new(registry: WmShortcutRegistry) -> Self {
        Self {
            registry,
            seats: BTreeMap::new(),
            chords: ChordBook::default(),
            pending: BTreeMap::new(),
            outbox: Vec::new(),
            next_created: 0,
        }
    }

    /// Install a registry. Its metadata (generation, chrome, capabilities)
    /// always replaces the old. Only a change of shapes is a registry change
    /// for chords: open chords then end cancelled and pending decisions are
    /// dropped. The keys down stay recorded either way, so the release of a
    /// press the old shapes consumed is still consumed rather than reaching a
    /// client unpaired.
    pub fn replace_registry(&mut self, registry: WmShortcutRegistry) {
        if self.registry.shapes != registry.shapes {
            self.cancel_all_chords();
        }
        self.registry = registry;
    }

    /// End every open chord cancelled and drop every pending decision, as
    /// when keyboard routing leaves Session.
    pub fn cancel_all_chords(&mut self) {
        self.chords.cancel_all(&mut self.outbox);
        self.pending.clear();
    }

    /// Seats whose modifier state is unknown, for reporting transitions.
    pub fn uncertain_seats(&self) -> impl Iterator<Item = SeatId> + '_ {
        self.seats
            .iter()
            .filter(|(_, state)| !state.mask_known())
            .map(|(seat, _)| *seat)
    }

    /// End every chord on the seat cancelled, drop its pending decision and
    /// forget its keys. This is the trusted reset that also clears a refused
    /// or saturated seat.
    pub fn clear_seat(&mut self, seat: SeatId) -> bool {
        self.cancel_seat_chords(seat);
        self.seats.remove(&seat).is_some()
    }

    /// End every chord on the seat cancelled and drop its pending decision,
    /// keeping its keys recorded. Used before synthetic releases (a VT
    /// switch) so those end nothing released.
    pub fn cancel_seat_chords(&mut self, seat: SeatId) {
        self.chords.cancel_seat(seat, &mut self.outbox);
        self.pending.remove(&seat);
    }

    /// A device went away: its keys, or its refusal, are forgotten, and every
    /// chord and pending decision on a seat it was part of ends cancelled.
    pub fn remove_device(&mut self, device: DeviceId) {
        let mut cancelled = Vec::new();
        for (seat, state) in &mut self.seats {
            let tracked = state.devices.len();
            state.devices.retain(|keys| keys.device != device);
            let refused = state.refused.remove(&device).is_some();
            if refused || state.devices.len() != tracked {
                cancelled.push(*seat);
            }
        }
        for seat in cancelled {
            self.cancel_seat_chords(seat);
        }
    }

    /// Whether the seat's modifier state is unknown, so it matches nothing:
    /// a refused device pressed a modifier, or the seat is saturated.
    pub fn seat_uncertain(&self, seat: SeatId) -> bool {
        self.seats
            .get(&seat)
            .is_some_and(|state| !state.mask_known())
    }

    /// The actions whose chords the WM follows, from its Configuration. Only
    /// chords opened afterwards use them.
    pub fn set_action_lifecycles(&mut self, interests: &[PolicyActionLifecycleInterest]) {
        self.chords.set_interests(interests);
    }

    /// A new WM epoch: chords, pending decisions and untaken outputs are
    /// dropped without a cause, and credits return. The keys down stay
    /// recorded, so old releases stay paired.
    pub fn reset_chords(&mut self) {
        self.chords.reset();
        self.pending.clear();
        self.outbox.clear();
    }

    /// The opener's Action was not admitted; the chord is dropped with its
    /// credit, and any of its events not yet taken with it.
    pub fn chord_opener_refused(&mut self, token: WmChordToken) -> bool {
        let refused = self.chords.opener_refused(token);
        self.outbox.retain(|output| {
            !matches!(
                output,
                WmShortcutOutput::Chord(
                    WmChordEvent::Held { token: event } | WmChordEvent::Ended { token: event, .. }
                ) if *event == token
            )
        });
        refused
    }

    /// A joining Action was not admitted; its trigger stops holding the chord.
    pub fn chord_join_refused(&mut self, token: WmChordToken, device: DeviceId, keycode: u32) {
        self.chords
            .join_refused(token, (device, keycode), &mut self.outbox);
    }

    /// The chord's Ended was handed to the WM as the in-flight Cycle.
    pub fn chord_delivered(&mut self, token: WmChordToken) -> bool {
        self.chords.delivered(token)
    }

    /// The owner time of the next decision, sequence timeout or Held, to
    /// bound how long the owner may wait. None when nothing is pending: an
    /// armed modifier tap is judged at its release and needs no timer.
    pub fn next_deadline(&self) -> Option<u64> {
        match (self.chords.next_deadline(), self.pending_deadline()) {
            (Some(held), Some(pending)) => Some(held.min(pending)),
            (held, pending) => held.or(pending),
        }
    }

    pub fn chord_credits_free(&self) -> usize {
        self.chords.credits_free()
    }

    pub fn modifier_mask(&self, seat: SeatId) -> WmModifierMask {
        self.seats
            .get(&seat)
            .map(WmSeatShortcutState::modifier_mask)
            .unwrap_or(WmModifierMask { bits: 0 })
    }

    pub const fn policy_generation(&self) -> u64 {
        self.registry.policy_generation()
    }

    pub fn binding_count(&self) -> usize {
        self.registry.binding_count()
    }

    pub const fn chrome(&self) -> WmChromePolicy {
        self.registry.chrome()
    }

    pub const fn supports_chrome_policy(&self) -> bool {
        self.registry.supports_chrome_policy()
    }

    pub fn shortcut_idle(&self) -> bool {
        self.chords.is_idle()
            && self.pending.is_empty()
            && self.seats.values().all(|state| {
                state.devices.is_empty() && state.refused.is_empty() && !state.saturated
            })
    }

    // The single-call API Session uses until its two-phase wiring lands
    // (t277 D3). With only immediate chords bound, every key event resolves
    // at once and only chord events wait to be drained.

    /// Route one physical key event, accepting whatever it claims.
    /// `now_msec` is the owner's clock, the one `poll_chords` and
    /// `next_deadline` use.
    pub fn route_key(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        now_msec: u64,
    ) -> WmShortcutDecision {
        let event = self.key_event(seat, device, keycode, pressed, now_msec);
        let consumed = event.consumed();
        event.accept();
        let mut decision = WmShortcutDecision {
            consumed,
            ..WmShortcutDecision::pass()
        };
        self.outbox.retain(|output| match output {
            WmShortcutOutput::Activation(activation) => {
                decision.action = Some(activation.action);
                decision.chord = activation.chord;
                false
            }
            WmShortcutOutput::Chord(_) => true,
        });
        decision
    }

    /// Record one physical key event without matching it.
    pub fn observe_key(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
    ) -> WmShortcutDecision {
        let consumed = self.observe_key_event(seat, device, keycode, pressed, 0);
        WmShortcutDecision {
            consumed,
            ..WmShortcutDecision::pass()
        }
    }

    /// Send Held for every chord due at `now_msec`.
    pub fn poll_chords(&mut self, now_msec: u64) {
        self.poll_shortcuts(now_msec);
    }

    /// Lifecycle events in the order Session must queue them.
    pub fn drain_chord_events(&mut self) -> Vec<WmChordEvent> {
        let mut events = Vec::new();
        self.outbox.retain(|output| match output {
            WmShortcutOutput::Chord(event) => {
                events.push(*event);
                false
            }
            WmShortcutOutput::Activation(_) => true,
        });
        events
    }
}

pub(crate) fn valid_chrome_policy(chrome: WmChromePolicy) -> bool {
    let valid_style = |enabled: bool, width: u32| {
        width <= 64 && ((enabled && width > 0) || (!enabled && width == 0))
    };
    valid_style(chrome.focus_ring.enabled, chrome.focus_ring.width)
        && valid_style(chrome.frame.enabled, chrome.frame.width)
}

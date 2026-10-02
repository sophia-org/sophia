//! Matching that decides after the press (t277 D): a lone modifier tap fires
//! on its release, a chord with a hold variant decides between tap and hold,
//! and a sequence waits for its next step. Each seat has at most one such
//! decision pending.
//!
//! All deadlines are owner-clock milliseconds, the clock route and service
//! calls pass. `advance` settles everything due, in (deadline, creation)
//! order, before any ordinary input is matched, so a timer serviced first and
//! the same input routed first give the same outputs. Trusted cancellation
//! never advances: it drops pending decisions without firing them.
//!
//! A press the router would claim comes back as a proposal on a `WmKeyEvent`
//! that borrows the router. Captures decide, then the proposal is accepted or
//! declined; dropping it declines. Nothing else can touch the router until it
//! is resolved, so outputs are taken only after the decision.

use super::chord::ChordLatch;
use super::ledger::{DeviceKeys, KEYCODE_LIMIT, modifier_bit};
use super::{
    WM_MAX_SHORTCUT_DEVICES, WM_MAX_SHORTCUT_SEATS, WmChordActivation, WmChordEvent, WmChordToken,
    WmShortcutRouter,
};
use crate::prelude::*;
use sophia_protocol::{PolicyChordEnd, WmActionId};

/// One output of a router call, in the order Session must queue them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmShortcutOutput {
    Activation(WmShortcutActivation),
    Chord(WmChordEvent),
}

/// An action fired by a key event or a deadline. `device` and `keycode` name
/// the press it came from, for join refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmShortcutActivation {
    pub seat: SeatId,
    pub device: DeviceId,
    pub keycode: u32,
    pub action: WmActionId,
    pub chord: Option<WmChordActivation>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmPressKind {
    /// A chord bound to an action.
    Immediate,
    /// A chord with a hold variant: tap or hold is decided later.
    Deciding,
    /// The first step of one or more sequences.
    SequenceStart,
    /// The next key of a pending sequence, matched or not.
    SequenceContinue,
    /// A lone modifier press that may become a tap.
    ArmModifierTap,
    /// The release of an armed modifier tap within its window.
    FireModifierTap,
}

/// What a claimed key event may do, for Session's captures to judge.
/// `possible_actions` is every action it can still lead to; `follows_chord`
/// says whether any of them belongs to a chord the WM follows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WmPressProposal {
    pub kind: WmPressKind,
    pub possible_actions: Vec<WmActionId>,
    pub follows_chord: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Resolution {
    Immediate {
        action: WmActionId,
        latch: ChordLatch,
    },
    Deciding {
        tap: Option<WmActionId>,
        hold: WmActionId,
        hold_ms: u32,
        latch: ChordLatch,
    },
    SequenceStart {
        node: usize,
        mask: u32,
    },
    Continue(Step),
    Arm {
        modifier: u32,
    },
    Fire {
        action: WmActionId,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Leaf {
        action: WmActionId,
        latch: ChordLatch,
    },
    Descend {
        node: usize,
        mask: u32,
    },
    Abort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Pending {
    Tap {
        device: DeviceId,
        keycode: u32,
        modifier: u32,
        armed_at: u64,
    },
    Deciding {
        device: DeviceId,
        keycode: u32,
        latch: ChordLatch,
        tap: Option<WmActionId>,
        hold: WmActionId,
        due: u64,
        created: u64,
    },
    Sequence {
        node: usize,
        due: u64,
        created: u64,
        /// Modifier classes of the steps matched so far.
        matched: u32,
        leader: Option<WmChordToken>,
    },
}

/// One routed key event. Holds the router until its proposal is resolved.
pub struct WmKeyEvent<'a> {
    router: &'a mut WmShortcutRouter,
    seat: SeatId,
    device: DeviceId,
    keycode: u32,
    now: u64,
    consumed: bool,
    proposal: Option<(WmPressProposal, Resolution)>,
}

impl WmKeyEvent<'_> {
    /// Whether the router owns this press or release: it reaches no client.
    pub const fn consumed(&self) -> bool {
        self.consumed
    }

    pub fn proposal(&self) -> Option<&WmPressProposal> {
        self.proposal.as_ref().map(|(proposal, _)| proposal)
    }

    /// Read the router while the proposal is outstanding.
    pub fn router(&self) -> &WmShortcutRouter {
        self.router
    }

    /// Everything produced before this event's proposal is resolved, in
    /// order: work that came due before it, and the terminals its release
    /// caused. The proposal's own outputs follow its resolution. Taking them
    /// is not admitting them; the caller still queues them in this order.
    #[must_use = "a key event's outputs must be queued in order"]
    pub fn take_outputs(&mut self) -> Vec<WmShortcutOutput> {
        self.router.take_outputs()
    }

    /// Accept whatever the event claims. Returns every output not yet taken,
    /// in order, ending with the event's own.
    #[must_use = "a key event's outputs must be queued in order"]
    pub fn accept(mut self) -> Vec<WmShortcutOutput> {
        self.resolve(true);
        self.router.take_outputs()
    }

    /// A capture took the event: whatever it would have fired never fires.
    /// The press stays recorded as it was, so its release pairs as before.
    /// Returns every output not yet taken, in order, such as the Ended of a
    /// sequence the event abandoned.
    #[must_use = "a key event's outputs must be queued in order"]
    pub fn decline(mut self) -> Vec<WmShortcutOutput> {
        self.resolve(false);
        self.router.take_outputs()
    }

    fn resolve(&mut self, accept: bool) {
        if let Some((_, resolution)) = self.proposal.take() {
            self.router.resolve(
                self.seat,
                self.device,
                self.keycode,
                self.now,
                resolution,
                accept,
            );
        }
    }
}

impl Drop for WmKeyEvent<'_> {
    fn drop(&mut self) {
        self.resolve(false);
    }
}

impl WmShortcutRouter {
    /// Route one physical key event at owner time `now`.
    pub fn key_event(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        now: u64,
    ) -> WmKeyEvent<'_> {
        self.advance(now);
        let (consumed, proposal) = self.route(seat, device, keycode, pressed, now, true);
        WmKeyEvent {
            router: self,
            seat,
            device,
            keycode,
            now,
            consumed,
            proposal,
        }
    }

    /// Record one physical key event without matching it: presses claim
    /// nothing, releases still end chords and keep consumed pairing. Used
    /// while matching is disabled, so the record stays true for when it
    /// resumes. The event never carries a proposal.
    pub fn observe_key_event(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        now: u64,
    ) -> WmKeyEvent<'_> {
        self.advance(now);
        let (consumed, _) = self.route(seat, device, keycode, pressed, now, false);
        WmKeyEvent {
            router: self,
            seat,
            device,
            keycode,
            now,
            consumed,
            proposal: None,
        }
    }

    /// A pointer button or axis event on `seat`: it disarms a modifier tap,
    /// interrupts a tap-versus-hold decision without firing, and abandons a
    /// pending sequence.
    pub fn pointer_activity(&mut self, seat: SeatId, now: u64) {
        self.advance(now);
        self.drop_pending(seat, PolicyChordEnd::Aborted);
    }

    /// Settle everything due at `now`: holds, sequence timeouts and Helds.
    pub fn poll_shortcuts(&mut self, now: u64) {
        self.advance(now);
    }

    /// The outputs of every call since the last take, in order.
    pub fn take_outputs(&mut self) -> Vec<WmShortcutOutput> {
        core::mem::take(&mut self.outbox)
    }

    pub(crate) fn next_created(&mut self) -> u64 {
        self.next_created += 1;
        self.next_created
    }

    pub(crate) fn pending_deadline(&self) -> Option<u64> {
        self.pending
            .values()
            .filter_map(|pending| match pending {
                Pending::Deciding { due, .. } | Pending::Sequence { due, .. } => Some(*due),
                Pending::Tap { .. } => None,
            })
            .min()
    }

    pub(crate) fn advance(&mut self, now: u64) {
        loop {
            let seat = self
                .pending
                .iter()
                .filter_map(|(seat, pending)| match pending {
                    Pending::Deciding { due, created, .. }
                    | Pending::Sequence { due, created, .. }
                        if *due <= now =>
                    {
                        Some((*due, *created, *seat))
                    }
                    _ => None,
                })
                .min();
            let held = self.chords.next_held().filter(|(due, _, _)| *due <= now);
            let seat = match (seat, held) {
                (None, None) => return,
                (Some((due, created, seat)), Some((held_due, held_created, _)))
                    if (due, created) < (held_due, held_created) =>
                {
                    seat
                }
                (_, Some((_, _, token))) => {
                    self.chords.send_held(token, &mut self.outbox);
                    continue;
                }
                (Some((_, _, seat)), None) => seat,
            };
            match self.pending.remove(&seat) {
                Some(Pending::Deciding {
                    device,
                    keycode,
                    latch,
                    hold,
                    ..
                }) => {
                    self.fire(seat, device, keycode, hold, latch, now);
                }
                Some(Pending::Sequence { leader, .. }) => {
                    self.end_leader(leader, PolicyChordEnd::TimedOut);
                }
                _ => {}
            }
        }
    }

    /// Drop the seat's pending decision without firing it. A sequence's
    /// leader ends with `end`; trusted cancellation ends it through the
    /// chord book instead, so it passes nothing here.
    pub(crate) fn drop_pending(&mut self, seat: SeatId, end: PolicyChordEnd) {
        if let Some(Pending::Sequence { leader, .. }) = self.pending.remove(&seat) {
            self.end_leader(leader, end);
        }
    }

    fn end_leader(&mut self, leader: Option<WmChordToken>, end: PolicyChordEnd) {
        if let Some(token) = leader {
            self.chords.end_leader(token, end, &mut self.outbox);
        }
    }

    /// Write one activation. Its chord is held as `latch` says, checked
    /// against what is down now; an opener already unheld ends released right
    /// after its Action. With no credit free it fires nothing.
    fn fire(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        action: WmActionId,
        latch: ChordLatch,
        now: u64,
    ) -> Option<WmChordToken> {
        let state = self.seats.get(&seat);
        // A modifier hold is checked by end_if_unheld below; a trigger is
        // recorded only while its key is still down.
        let modifiers_down = state.is_some_and(|state| state.modifiers_down());
        let trigger_down = state.is_some_and(|state| state.is_down(device, keycode));
        let created = self.next_created();
        let chord = self
            .chords
            .activate(
                seat,
                action,
                latch,
                (device, keycode),
                trigger_down,
                now,
                created,
            )
            .ok()?;
        self.outbox
            .push(WmShortcutOutput::Activation(WmShortcutActivation {
                seat,
                device,
                keycode,
                action,
                chord,
            }));
        let chord = chord?;
        if chord.opens {
            self.chords
                .end_if_unheld(chord.token, modifiers_down, &mut self.outbox);
        }
        Some(chord.token)
    }

    /// A leader fires only as a chord the WM follows now; otherwise its
    /// sequence continues without it.
    fn fire_leader(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        node: usize,
        now: u64,
    ) -> Option<WmChordToken> {
        let action = self.registry.shapes.nodes[node].leader?;
        if !self.chords.declared(action) {
            return None;
        }
        self.fire(seat, device, keycode, action, ChordLatch::Sequence, now)
    }

    fn proposal(
        &self,
        seat: SeatId,
        kind: WmPressKind,
        possible_actions: Vec<WmActionId>,
        leader: Option<WmChordToken>,
    ) -> WmPressProposal {
        let follows_chord = leader.is_some_and(|token| self.chords.is_open(token))
            || possible_actions
                .iter()
                .any(|action| self.chords.follows(seat, *action));
        WmPressProposal {
            kind,
            possible_actions,
            follows_chord,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn route(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        now: u64,
        matching: bool,
    ) -> (bool, Option<(WmPressProposal, Resolution)>) {
        if !seat.is_valid() || keycode >= KEYCODE_LIMIT {
            return (false, None);
        }
        if !self.seats.contains_key(&seat) {
            if self.seats.len() >= WM_MAX_SHORTCUT_SEATS {
                return (false, None);
            }
            self.seats.insert(seat, Default::default());
        }
        let modifier = modifier_bit(keycode);
        let state = self.seats.get_mut(&seat).expect("seat inserted above");
        let was_known = state.mask_known();
        if let Some(pressed_modifier) = state.refused.get_mut(&device) {
            // A refused device stays refused: one of its presses may already
            // have reached a client, so none of its keys can be matched.
            *pressed_modifier |= pressed && modifier != 0;
            let known = state.mask_known();
            if !pressed {
                let modifiers_down = state.modifiers_down();
                self.chords
                    .key_released(seat, (device, keycode), modifiers_down, &mut self.outbox);
            }
            if was_known && !known {
                self.drop_pending(seat, PolicyChordEnd::Aborted);
            }
            return (false, None);
        }
        if !pressed {
            return self.release(seat, device, keycode, now);
        }
        if let Some(keys) = state.devices.iter().find(|keys| keys.device == device)
            && keys.is_down(keycode)
        {
            // Autorepeat, or a duplicate press: it fires nothing, joins
            // nothing, touches no pending decision, and its consumption
            // follows the original press.
            return (keys.is_consumed(keycode), None);
        }
        let nothing_down = !state.any_down();
        if !state.devices.iter().any(|keys| keys.device == device) {
            if state.devices.len() >= WM_MAX_SHORTCUT_DEVICES {
                if state.refused.len() >= WM_MAX_SHORTCUT_DEVICES {
                    state.saturated = true;
                } else {
                    state.refused.insert(device, modifier != 0);
                }
                if was_known && !state.mask_known() {
                    self.drop_pending(seat, PolicyChordEnd::Aborted);
                }
                return (false, None);
            }
            state.devices.push(DeviceKeys::new(device));
        }
        if !matching || !was_known {
            state.press(device, keycode, false);
            return (false, None);
        }
        let mask = state.modifier_mask().bits;
        let shapes = &self.registry.shapes;
        if modifier != 0 {
            state.press(device, keycode, false);
            return match self.pending.get(&seat).copied() {
                // A second modifier means this is no lone tap.
                Some(Pending::Tap { .. }) => {
                    self.pending.remove(&seat);
                    (false, None)
                }
                None if nothing_down => match shapes.taps.get(&modifier) {
                    Some(&action) => {
                        let proposal =
                            self.proposal(seat, WmPressKind::ArmModifierTap, vec![action], None);
                        (false, Some((proposal, Resolution::Arm { modifier })))
                    }
                    None => (false, None),
                },
                _ => (false, None),
            };
        }
        match self.pending.get(&seat).copied() {
            Some(Pending::Sequence {
                node,
                matched,
                leader,
                ..
            }) => {
                state.press(device, keycode, true);
                let step = self.continuation(node, keycode, mask, matched);
                let possible = match step {
                    Step::Leaf { action, .. } => vec![action],
                    Step::Descend { node, .. } => {
                        self.registry.shapes.nodes[node].reachable.clone()
                    }
                    Step::Abort => Vec::new(),
                };
                let proposal = self.proposal(seat, WmPressKind::SequenceContinue, possible, leader);
                return (true, Some((proposal, Resolution::Continue(step))));
            }
            // Any other key disarms a modifier tap, and interrupts a
            // decision before its deadline: neither tap nor hold fires.
            Some(Pending::Tap { .. } | Pending::Deciding { .. }) => {
                self.pending.remove(&seat);
            }
            None => {}
        }
        let latch = if mask != 0 {
            ChordLatch::Modifiers
        } else {
            ChordLatch::Trigger
        };
        let chord = (keycode, mask);
        let shapes = &self.registry.shapes;
        let resolution = if let Some(&(hold, hold_ms)) = shapes.holds.get(&chord) {
            let tap = shapes.immediate.get(&chord).copied();
            Resolution::Deciding {
                tap,
                hold,
                hold_ms,
                latch,
            }
        } else if let Some(&action) = shapes.immediate.get(&chord) {
            Resolution::Immediate { action, latch }
        } else if let Some(&node) = shapes.roots.get(&chord) {
            Resolution::SequenceStart { node, mask }
        } else {
            state.press(device, keycode, false);
            return (false, None);
        };
        state.press(device, keycode, true);
        let (kind, possible) = match resolution {
            Resolution::Immediate { action, .. } => {
                // A press that would open a chord with no credit free is
                // still the router's, before any capture: it fires nothing.
                if self.chords.would_open(seat, action) && self.chords.credits_free() == 0 {
                    return (true, None);
                }
                (WmPressKind::Immediate, vec![action])
            }
            Resolution::Deciding { tap, hold, .. } => (
                WmPressKind::Deciding,
                tap.into_iter().chain([hold]).collect(),
            ),
            Resolution::SequenceStart { node, .. } => (
                WmPressKind::SequenceStart,
                self.registry.shapes.nodes[node].reachable.clone(),
            ),
            _ => unreachable!("only fresh lookups are resolved here"),
        };
        let proposal = self.proposal(seat, kind, possible, None);
        (true, Some((proposal, resolution)))
    }

    /// The next step of a pending sequence: the live mask must match a child
    /// exactly, or match once the modifier classes of the steps so far are
    /// set aside. Anything else abandons it; Escape always does, because the
    /// plan admits no Escape step.
    fn continuation(&self, node: usize, keycode: u32, mask: u32, matched: u32) -> Step {
        let children = &self.registry.shapes.nodes[node].children;
        let Some((&(_, step_mask), &child)) = children
            .get_key_value(&(keycode, mask))
            .or_else(|| children.get_key_value(&(keycode, mask & !matched)))
        else {
            return Step::Abort;
        };
        match self.registry.shapes.nodes[child].leaf {
            Some(action) => Step::Leaf {
                action,
                latch: if mask != 0 {
                    ChordLatch::Modifiers
                } else {
                    ChordLatch::Trigger
                },
            },
            None => Step::Descend {
                node: child,
                mask: step_mask,
            },
        }
    }

    fn release(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        now: u64,
    ) -> (bool, Option<(WmPressProposal, Resolution)>) {
        let state = self.seats.get_mut(&seat).expect("routed seat exists");
        let consumed = state.release(device, keycode);
        let mut proposal = None;
        match self.pending.get(&seat).copied() {
            // Released before its deadline (a due one was settled first):
            // the tap variant fires, if there is one.
            Some(Pending::Deciding {
                device: pending_device,
                keycode: pending_keycode,
                latch,
                tap,
                ..
            }) if (pending_device, pending_keycode) == (device, keycode) => {
                self.pending.remove(&seat);
                if let Some(action) = tap {
                    self.fire(seat, device, keycode, action, latch, now);
                }
            }
            Some(Pending::Tap {
                device: pending_device,
                keycode: pending_keycode,
                modifier,
                armed_at,
            }) if (pending_device, pending_keycode) == (device, keycode) => {
                self.pending.remove(&seat);
                if now.saturating_sub(armed_at) < u64::from(self.registry.shapes.timing.tap_ms)
                    && let Some(&action) = self.registry.shapes.taps.get(&modifier)
                {
                    let fire =
                        self.proposal(seat, WmPressKind::FireModifierTap, vec![action], None);
                    proposal = Some((fire, Resolution::Fire { action }));
                }
            }
            _ => {}
        }
        let modifiers_down = self
            .seats
            .get(&seat)
            .is_some_and(|state| state.modifiers_down());
        self.chords
            .key_released(seat, (device, keycode), modifiers_down, &mut self.outbox);
        (consumed, proposal)
    }

    fn resolve(
        &mut self,
        seat: SeatId,
        device: DeviceId,
        keycode: u32,
        now: u64,
        resolution: Resolution,
        accept: bool,
    ) {
        let sequence_ms = u64::from(self.registry.shapes.timing.sequence_ms);
        match resolution {
            Resolution::Immediate { action, latch } if accept => {
                self.fire(seat, device, keycode, action, latch, now);
            }
            Resolution::Deciding {
                tap,
                hold,
                hold_ms,
                latch,
            } if accept => {
                let created = self.next_created();
                self.pending.insert(
                    seat,
                    Pending::Deciding {
                        device,
                        keycode,
                        latch,
                        tap,
                        hold,
                        due: now.saturating_add(u64::from(hold_ms)),
                        created,
                    },
                );
            }
            Resolution::SequenceStart { node, mask } if accept => {
                let created = self.next_created();
                let leader = self.fire_leader(seat, device, keycode, node, now);
                self.pending.insert(
                    seat,
                    Pending::Sequence {
                        node,
                        due: now.saturating_add(sequence_ms),
                        created,
                        matched: mask,
                        leader,
                    },
                );
            }
            Resolution::Continue(step) => {
                let Some(Pending::Sequence {
                    created,
                    matched,
                    leader,
                    ..
                }) = self.pending.remove(&seat)
                else {
                    return;
                };
                match (accept, step) {
                    (true, Step::Leaf { action, latch }) => {
                        self.fire(seat, device, keycode, action, latch, now);
                        self.end_leader(leader, PolicyChordEnd::Completed);
                    }
                    (true, Step::Descend { node, mask }) => {
                        let leader = match leader {
                            Some(token) => Some(token),
                            None => self.fire_leader(seat, device, keycode, node, now),
                        };
                        self.pending.insert(
                            seat,
                            Pending::Sequence {
                                node,
                                due: now.saturating_add(sequence_ms),
                                created,
                                matched: matched | mask,
                                leader,
                            },
                        );
                    }
                    _ => self.end_leader(leader, PolicyChordEnd::Aborted),
                }
            }
            Resolution::Arm { modifier } if accept => {
                self.pending.insert(
                    seat,
                    Pending::Tap {
                        device,
                        keycode,
                        modifier,
                        armed_at: now,
                    },
                );
            }
            Resolution::Fire { action } if accept => {
                self.fire(seat, device, keycode, action, ChordLatch::Trigger, now);
            }
            _ => {}
        }
    }
}

//! The chord lifecycle a WM asks for with `action_lifecycle` (t277).
//!
//! A declared action's keyboard activation opens a chord; further activations
//! of the same action on the same seat join it while it is open. This owner
//! decides when a chord is held and when it ends. Session correlates chord
//! tokens with admitted Actions and builds the causes; nothing here knows a
//! serial, a queue or a WM connection.
//!
//! Credits bound the WM's terminal obligations: opening a chord takes one,
//! held while the chord is open and while its Ended waits. It returns only
//! when Session reports that Ended handed off as the in-flight Cycle, or that
//! the opener was never admitted. Ending a chord early never returns it.
//!
//! Every event is written, in order, to the router's output batch.

use super::WmShortcutOutput;
use crate::prelude::*;
use sophia_protocol::{PolicyActionLifecycleInterest, PolicyChordEnd, WmActionId};

/// Open chords plus Ended causes still waiting in Session, per WM epoch.
pub const WM_CHORD_CREDITS: usize = 8;

/// One chord's identity, unique for the life of the book.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WmChordToken(u64);

/// The chord an activation belongs to. `opens` is true for the activation
/// that opened it, whose Action Session must admit for the chord to exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmChordActivation {
    pub token: WmChordToken,
    pub opens: bool,
}

/// A lifecycle event for Session to correlate and queue, in order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WmChordEvent {
    Held {
        token: WmChordToken,
    },
    Ended {
        token: WmChordToken,
        end: PolicyChordEnd,
    },
}

/// No credit was free, so the activation opens nothing and queues no Action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmChordCreditsExhausted;

/// What holds a chord, decided by the press that opened it. A join keeps
/// the opener's mode whatever its own press was.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChordLatch {
    /// Pressed with modifiers: held while any modifier is down on the seat.
    Modifiers,
    /// Pressed without: held while any trigger key of its Actions is down.
    Trigger,
    /// A sequence leader: keys never end it, only its sequence's outcome.
    Sequence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ChordHold {
    Modifiers,
    Triggers(BTreeSet<(DeviceId, u32)>),
    Sequence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OpenChord {
    token: WmChordToken,
    seat: SeatId,
    action: WmActionId,
    hold: ChordHold,
    /// Owner time at which Held is due, and the router-wide creation number
    /// that orders equal deadlines; cleared once Held is sent.
    held_at: Option<(u64, u64)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ChordBook {
    /// held_ms per declared action. Read only when a chord opens, so an open
    /// chord keeps the row it opened under.
    interests: BTreeMap<WmActionId, u32>,
    /// In open order, which is token order.
    open: Vec<OpenChord>,
    /// Every chord whose credit is taken: open, or ended and undelivered.
    obligations: BTreeSet<WmChordToken>,
    next_token: u64,
}

impl ChordBook {
    pub(crate) fn set_interests(&mut self, interests: &[PolicyActionLifecycleInterest]) {
        self.interests = interests
            .iter()
            .map(|interest| (interest.action, interest.held_ms))
            .collect();
    }

    pub(crate) fn declared(&self, action: WmActionId) -> bool {
        self.interests.contains_key(&action)
    }

    fn open_chord(&self, seat: SeatId, action: WmActionId) -> Option<&OpenChord> {
        self.open
            .iter()
            .find(|chord| chord.seat == seat && chord.action == action)
    }

    /// Whether an activation of `action` would open a new chord, taking a
    /// credit, rather than join one or be ordinary.
    pub(crate) fn would_open(&self, seat: SeatId, action: WmActionId) -> bool {
        self.declared(action) && self.open_chord(seat, action).is_none()
    }

    /// Whether an activation of `action` belongs to a chord the WM follows:
    /// it is declared now, or an open chord keeps its frozen eligibility.
    pub(crate) fn follows(&self, seat: SeatId, action: WmActionId) -> bool {
        self.declared(action) || self.open_chord(seat, action).is_some()
    }

    /// An activation of `action` on `seat` at owner time `time`, pressed on
    /// `trigger`. A new chord is held as `latch` says; a join keeps the
    /// opener's hold, and a trigger-held chord gains this trigger whatever
    /// this press's modifiers were. `trigger_down` is whether that key is
    /// still down: a delayed activation whose key is already up adds no
    /// trigger, and the caller ends an unheld opener once its Action is
    /// written.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn activate(
        &mut self,
        seat: SeatId,
        action: WmActionId,
        latch: ChordLatch,
        trigger: (DeviceId, u32),
        trigger_down: bool,
        time: u64,
        created: u64,
    ) -> Result<Option<WmChordActivation>, WmChordCreditsExhausted> {
        let trigger = trigger_down.then_some(trigger);
        // An open chord keeps the eligibility and hold it opened under, so a
        // join is found before the current declarations are consulted.
        if let Some(chord) = self
            .open
            .iter_mut()
            .find(|chord| chord.seat == seat && chord.action == action)
        {
            if let (ChordHold::Triggers(keys), Some(trigger)) = (&mut chord.hold, trigger) {
                keys.insert(trigger);
            }
            return Ok(Some(WmChordActivation {
                token: chord.token,
                opens: false,
            }));
        }
        let Some(&held_ms) = self.interests.get(&action) else {
            return Ok(None);
        };
        if self.obligations.len() >= WM_CHORD_CREDITS {
            return Err(WmChordCreditsExhausted);
        }
        self.next_token += 1;
        let token = WmChordToken(self.next_token);
        self.obligations.insert(token);
        self.open.push(OpenChord {
            token,
            seat,
            action,
            hold: match latch {
                ChordLatch::Modifiers => ChordHold::Modifiers,
                ChordLatch::Trigger => ChordHold::Triggers(trigger.into_iter().collect()),
                ChordLatch::Sequence => ChordHold::Sequence,
            },
            held_at: (held_ms != 0).then(|| (time.saturating_add(u64::from(held_ms)), created)),
        });
        Ok(Some(WmChordActivation { token, opens: true }))
    }

    /// End a just-opened chord released if nothing holds it already. Called
    /// after its opening Action is written, so the Ended follows it.
    pub(crate) fn end_if_unheld(
        &mut self,
        token: WmChordToken,
        modifiers_down: bool,
        out: &mut Vec<WmShortcutOutput>,
    ) {
        self.end(
            |chord| chord.token == token && !chord.held(modifiers_down),
            PolicyChordEnd::Released,
            out,
        );
    }

    /// The opener's Action was refused, so the chord never existed for the WM:
    /// it is dropped with its credit and reports nothing. Session may learn of
    /// the refusal after the chord was released or cancelled, and after its
    /// events were extracted; the credit is reclaimed in every case, once.
    /// Events already extracted are Session's to discard.
    pub(crate) fn opener_refused(&mut self, token: WmChordToken) -> bool {
        if !self.obligations.remove(&token) {
            return false;
        }
        self.open.retain(|chord| chord.token != token);
        true
    }

    /// A joining Action was refused, so its trigger does not hold the chord.
    pub(crate) fn join_refused(
        &mut self,
        token: WmChordToken,
        trigger: (DeviceId, u32),
        out: &mut Vec<WmShortcutOutput>,
    ) {
        let Some(chord) = self.open.iter_mut().find(|chord| chord.token == token) else {
            return;
        };
        if let ChordHold::Triggers(keys) = &mut chord.hold {
            keys.remove(&trigger);
            if keys.is_empty() {
                self.end(|chord| chord.token == token, PolicyChordEnd::Released, out);
            }
        }
    }

    /// Session handed this chord's Ended to the WM as the in-flight Cycle.
    pub(crate) fn delivered(&mut self, token: WmChordToken) -> bool {
        !self.open.iter().any(|chord| chord.token == token) && self.obligations.remove(&token)
    }

    /// A key on `seat` went up. `modifiers_down` is whether any modifier is
    /// still down on the seat after it. Sequence leaders are unaffected.
    pub(crate) fn key_released(
        &mut self,
        seat: SeatId,
        key: (DeviceId, u32),
        modifiers_down: bool,
        out: &mut Vec<WmShortcutOutput>,
    ) {
        for chord in &mut self.open {
            if chord.seat == seat
                && let ChordHold::Triggers(keys) = &mut chord.hold
            {
                keys.remove(&key);
            }
        }
        self.end(
            |chord| chord.seat == seat && !chord.held(modifiers_down),
            PolicyChordEnd::Released,
            out,
        );
    }

    /// End a sequence leader's chord with its sequence's outcome.
    pub(crate) fn end_leader(
        &mut self,
        token: WmChordToken,
        end: PolicyChordEnd,
        out: &mut Vec<WmShortcutOutput>,
    ) {
        self.end(
            |chord| chord.token == token && chord.hold == ChordHold::Sequence,
            end,
            out,
        );
    }

    /// Whether the chord is still open.
    pub(crate) fn is_open(&self, token: WmChordToken) -> bool {
        self.open.iter().any(|chord| chord.token == token)
    }

    /// The earliest Held still owed, by (deadline, creation).
    pub(crate) fn next_held(&self) -> Option<(u64, u64, WmChordToken)> {
        self.open
            .iter()
            .filter_map(|chord| {
                chord
                    .held_at
                    .map(|(due, created)| (due, created, chord.token))
            })
            .min()
    }

    pub(crate) fn send_held(&mut self, token: WmChordToken, out: &mut Vec<WmShortcutOutput>) {
        if let Some(chord) = self.open.iter_mut().find(|chord| chord.token == token)
            && chord.held_at.take().is_some()
        {
            out.push(WmShortcutOutput::Chord(WmChordEvent::Held { token }));
        }
    }

    pub(crate) fn next_deadline(&self) -> Option<u64> {
        self.next_held().map(|(due, _, _)| due)
    }

    pub(crate) fn cancel_seat(&mut self, seat: SeatId, out: &mut Vec<WmShortcutOutput>) {
        self.end(|chord| chord.seat == seat, PolicyChordEnd::Cancelled, out);
    }

    pub(crate) fn cancel_all(&mut self, out: &mut Vec<WmShortcutOutput>) {
        self.end(|_| true, PolicyChordEnd::Cancelled, out);
    }

    /// A new WM epoch: every chord is discarded without a cause, and all
    /// credits return, because those Actions belonged to the old epoch.
    /// Declared interests belong to the next Configuration.
    pub(crate) fn reset(&mut self) {
        self.open.clear();
        self.obligations.clear();
        self.interests.clear();
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.open.is_empty()
    }

    pub(crate) fn credits_free(&self) -> usize {
        WM_CHORD_CREDITS - self.obligations.len()
    }

    /// End every open chord matching `ends`, in open order. The credit stays
    /// taken until Session reports the Ended delivered.
    fn end(
        &mut self,
        mut ends: impl FnMut(&OpenChord) -> bool,
        end: PolicyChordEnd,
        out: &mut Vec<WmShortcutOutput>,
    ) {
        let mut index = 0;
        while index < self.open.len() {
            if ends(&self.open[index]) {
                let chord = self.open.remove(index);
                out.push(WmShortcutOutput::Chord(WmChordEvent::Ended {
                    token: chord.token,
                    end,
                }));
            } else {
                index += 1;
            }
        }
    }
}

impl OpenChord {
    fn held(&self, modifiers_down: bool) -> bool {
        match &self.hold {
            ChordHold::Modifiers => modifiers_down,
            ChordHold::Triggers(keys) => !keys.is_empty(),
            ChordHold::Sequence => true,
        }
    }
}

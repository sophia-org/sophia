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

/// No credit was free, so the press opens nothing and queues no Action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WmChordCreditsExhausted;

/// What keeps a chord open, fixed by the activation that opened it.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ChordHold {
    /// Opened with modifiers: open while any modifier is down on the seat.
    Modifiers,
    /// Opened without: open while any trigger key of its Actions is down.
    Triggers(BTreeSet<(DeviceId, u32)>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OpenChord {
    token: WmChordToken,
    seat: SeatId,
    action: WmActionId,
    hold: ChordHold,
    /// Event time at which Held is due; cleared once Held is sent.
    held_at: Option<u64>,
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
    outbox: Vec<WmChordEvent>,
}

impl ChordBook {
    pub(crate) fn set_interests(&mut self, interests: &[PolicyActionLifecycleInterest]) {
        self.interests = interests
            .iter()
            .map(|interest| (interest.action, interest.held_ms))
            .collect();
    }

    /// An activation of `action` on `seat`, pressed on `trigger` at `time`.
    /// `modifiers` is whether any modifier was down when it fired.
    pub(crate) fn activate(
        &mut self,
        seat: SeatId,
        action: WmActionId,
        trigger: (DeviceId, u32),
        modifiers: bool,
        time: u64,
    ) -> Result<Option<WmChordActivation>, WmChordCreditsExhausted> {
        // An open chord keeps the eligibility it opened under, so a join is
        // found before the current declarations are consulted.
        if let Some(chord) = self
            .open
            .iter_mut()
            .find(|chord| chord.seat == seat && chord.action == action)
        {
            if let ChordHold::Triggers(keys) = &mut chord.hold {
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
            hold: if modifiers {
                ChordHold::Modifiers
            } else {
                ChordHold::Triggers(BTreeSet::from([trigger]))
            },
            held_at: (held_ms != 0).then(|| time.saturating_add(u64::from(held_ms))),
        });
        Ok(Some(WmChordActivation { token, opens: true }))
    }

    /// The opener's Action was refused, so the chord never existed for the WM:
    /// it is dropped with its credit and reports nothing. Session may learn of
    /// the refusal after the chord was released or cancelled, and after its
    /// events were drained; the credit is reclaimed in every case, once. Events
    /// already drained are Session's to discard.
    pub(crate) fn opener_refused(&mut self, token: WmChordToken) -> bool {
        if !self.obligations.remove(&token) {
            return false;
        }
        self.open.retain(|chord| chord.token != token);
        self.outbox.retain(|event| match event {
            WmChordEvent::Held { token: held } | WmChordEvent::Ended { token: held, .. } => {
                *held != token
            }
        });
        true
    }

    /// A joining Action was refused, so its trigger does not hold the chord.
    pub(crate) fn join_refused(&mut self, token: WmChordToken, trigger: (DeviceId, u32)) {
        let Some(chord) = self.open.iter_mut().find(|chord| chord.token == token) else {
            return;
        };
        if let ChordHold::Triggers(keys) = &mut chord.hold {
            keys.remove(&trigger);
            if keys.is_empty() {
                self.end(|chord| chord.token == token, PolicyChordEnd::Released);
            }
        }
    }

    /// Session handed this chord's Ended to the WM as the in-flight Cycle.
    pub(crate) fn delivered(&mut self, token: WmChordToken) -> bool {
        !self.open.iter().any(|chord| chord.token == token) && self.obligations.remove(&token)
    }

    /// A key on `seat` went up. `modifiers_down` is whether any modifier is
    /// still down on the seat after it.
    pub(crate) fn key_released(
        &mut self,
        seat: SeatId,
        key: (DeviceId, u32),
        modifiers_down: bool,
    ) {
        for chord in &mut self.open {
            if chord.seat == seat
                && let ChordHold::Triggers(keys) = &mut chord.hold
            {
                keys.remove(&key);
            }
        }
        self.end(
            |chord| {
                chord.seat == seat
                    && match &chord.hold {
                        ChordHold::Modifiers => !modifiers_down,
                        ChordHold::Triggers(keys) => keys.is_empty(),
                    }
            },
            PolicyChordEnd::Released,
        );
    }

    /// Held for every open chord due at `now`, in open order.
    pub(crate) fn poll(&mut self, now: u64) {
        for chord in &mut self.open {
            if chord.held_at.is_some_and(|due| due <= now) {
                chord.held_at = None;
                self.outbox.push(WmChordEvent::Held { token: chord.token });
            }
        }
    }

    pub(crate) fn next_deadline(&self) -> Option<u64> {
        self.open.iter().filter_map(|chord| chord.held_at).min()
    }

    pub(crate) fn cancel_seat(&mut self, seat: SeatId) {
        self.end(|chord| chord.seat == seat, PolicyChordEnd::Cancelled);
    }

    pub(crate) fn cancel_all(&mut self) {
        self.end(|_| true, PolicyChordEnd::Cancelled);
    }

    /// A new WM epoch: every chord and pending event is discarded without a
    /// cause, and all credits return, because those Actions belonged to the
    /// old epoch. Declared interests belong to the next Configuration.
    pub(crate) fn reset(&mut self) {
        self.open.clear();
        self.obligations.clear();
        self.outbox.clear();
        self.interests.clear();
    }

    pub(crate) fn drain(&mut self) -> Vec<WmChordEvent> {
        core::mem::take(&mut self.outbox)
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.open.is_empty()
    }

    pub(crate) fn credits_free(&self) -> usize {
        WM_CHORD_CREDITS - self.obligations.len()
    }

    /// End every open chord matching `ends`, in open order. The credit stays
    /// taken until Session reports the Ended delivered.
    fn end(&mut self, mut ends: impl FnMut(&OpenChord) -> bool, end: PolicyChordEnd) {
        let mut index = 0;
        while index < self.open.len() {
            if ends(&self.open[index]) {
                let chord = self.open.remove(index);
                self.outbox.push(WmChordEvent::Ended {
                    token: chord.token,
                    end,
                });
            } else {
                index += 1;
            }
        }
    }
}

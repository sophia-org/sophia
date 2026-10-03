//! Physical keys while the session is locked (t292).
//!
//! The VT and emergency recognizers see every key first, exactly as they do
//! unlocked; then the key goes to Engine's lock keyboard and nowhere else. The
//! secret it builds lives here until a submission hands it to the
//! authenticator. Providers are told what the secret did, never what it holds.

use crate::emergency_input::{EmergencyChordAction, EmergencyChordState};
use crate::session_keyboard::{VirtualTerminalChordAction, VirtualTerminalChordState};
use sophia_engine::{SessionLockKey, SessionLockKeyboard};
use sophia_protocol::DeviceId;

/// The most secret bytes a lock accepts, as lockme did. Text past it is
/// refused whole, never truncated into a different secret.
pub const SESSION_LOCK_SECRET_CAPACITY: usize = 1024;

/// The secret being typed. Its storage is allocated once at full capacity,
/// so it never reallocates and leaves no unzeroed copy behind; every clear
/// and drop zeroes all of it. Memory locking belongs to the authenticator's
/// hardening (t293).
pub struct SessionLockSecret {
    bytes: Box<[u8; SESSION_LOCK_SECRET_CAPACITY]>,
    len: usize,
}

impl Default for SessionLockSecret {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionLockSecret {
    pub fn new() -> Self {
        Self {
            bytes: Box::new([0; SESSION_LOCK_SECRET_CAPACITY]),
            len: 0,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Appends `text` whole; `false`, changing nothing, if it would not fit.
    pub fn push_str(&mut self, text: &str) -> bool {
        let Some(end) = self
            .len
            .checked_add(text.len())
            .filter(|end| *end <= SESSION_LOCK_SECRET_CAPACITY)
        else {
            return false;
        };
        self.bytes[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        true
    }

    /// Removes the last character; `false` if the secret was empty.
    pub fn pop_char(&mut self) -> bool {
        let Some(last) = self.as_str().chars().next_back() else {
            return false;
        };
        let start = self.len - last.len_utf8();
        self.bytes[start..self.len].fill(0);
        std::hint::black_box(&self.bytes[start..self.len]);
        self.len = start;
        true
    }

    /// Zeroes the whole buffer, not only its used prefix.
    pub fn clear(&mut self) {
        self.bytes.fill(0);
        std::hint::black_box(&self.bytes);
        self.len = 0;
    }

    /// The secret, for handing to the authenticator and nothing else.
    pub fn as_str(&self) -> &str {
        // Only whole UTF-8 strings are ever appended and only whole characters
        // removed, so the prefix is always valid; an empty secret otherwise.
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or_default()
    }
}

impl Drop for SessionLockSecret {
    fn drop(&mut self) {
        self.clear();
    }
}

impl core::fmt::Debug for SessionLockSecret {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Neither the text nor its length.
        formatter.write_str("SessionLockSecret(..)")
    }
}

/// What the secret did, as a provider may be told it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLockEdit {
    Insert,
    Delete,
    Clear,
    Submit,
}

/// How one key was settled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLockKeyOutcome {
    /// The lock took the key; nothing outside the lock sees it.
    Consumed,
    /// The operator's emergency chord: the session ends.
    EmergencyExit,
    /// A VT switch chord; the trigger and held modifiers are reported as for
    /// an unlocked session.
    VirtualTerminal {
        terminal: u8,
        trigger: u32,
        modifiers: [Option<u32>; 4],
    },
}

/// One lock's keyboard, secret and pending reports. A new lock gets a new
/// one, so no secret, composition or chord carries from one lock to the next.
pub struct SessionLockInput {
    keyboard: SessionLockKeyboard,
    secret: SessionLockSecret,
    edits: Vec<(SessionLockEdit, bool)>,
    chords: Vec<u16>,
    submitted: bool,
}

impl SessionLockInput {
    pub fn new(keyboard: SessionLockKeyboard) -> Self {
        Self {
            keyboard,
            secret: SessionLockSecret::new(),
            edits: Vec::new(),
            chords: Vec::new(),
            submitted: false,
        }
    }

    pub fn observe_key(
        &mut self,
        device: DeviceId,
        keycode: u32,
        pressed: bool,
        time_msec: u64,
        emergency: &mut EmergencyChordState,
        virtual_terminal: &mut VirtualTerminalChordState,
    ) -> SessionLockKeyOutcome {
        let outcome = match virtual_terminal.observe_at_device(device, keycode, pressed, time_msec)
        {
            VirtualTerminalChordAction::Pass => None,
            VirtualTerminalChordAction::Consume => Some(SessionLockKeyOutcome::Consumed),
            VirtualTerminalChordAction::Activate(terminal) => {
                Some(SessionLockKeyOutcome::VirtualTerminal {
                    terminal,
                    trigger: keycode,
                    modifiers: virtual_terminal.pressed_modifier_keycodes_for(device),
                })
            }
        };
        let outcome = outcome.or_else(|| {
            (emergency.observe_at_device(device, keycode, pressed)
                == EmergencyChordAction::Triggered)
                .then_some(SessionLockKeyOutcome::EmergencyExit)
        });
        // The keymap sees every key; only an unclaimed one edits.
        let secret = &mut self.secret;
        let mut fitted = true;
        let key = self
            .keyboard
            .observe(keycode, pressed, outcome.is_none(), &mut |text| {
                fitted = secret.push_str(text);
            });
        if let Some(outcome) = outcome {
            return outcome;
        }
        match key {
            SessionLockKey::Inserted if fitted => self.edits.push((SessionLockEdit::Insert, false)),
            SessionLockKey::Delete if self.secret.pop_char() => {
                self.edits
                    .push((SessionLockEdit::Delete, self.secret.is_empty()));
            }
            SessionLockKey::Clear if !self.secret.is_empty() => {
                self.secret.clear();
                self.edits.push((SessionLockEdit::Clear, true));
            }
            // An empty secret is never submitted.
            SessionLockKey::Submit if !self.secret.is_empty() && !self.submitted => {
                self.submitted = true;
                self.edits.push((SessionLockEdit::Submit, false));
            }
            SessionLockKey::Chord(chord) => self.chords.push(chord),
            _ => {}
        }
        SessionLockKeyOutcome::Consumed
    }

    /// Edits since the last call, each with whether the secret was empty after.
    pub fn take_edits(&mut self) -> Vec<(SessionLockEdit, bool)> {
        std::mem::take(&mut self.edits)
    }

    pub fn take_chords(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.chords)
    }

    /// The submitted secret, once per submission, for the authenticator.
    pub fn submission(&self) -> Option<&SessionLockSecret> {
        self.submitted.then_some(&self.secret)
    }

    /// Ends a submission whatever its verdict: the secret is zeroed and
    /// typing starts afresh.
    pub fn settle_submission(&mut self) {
        self.submitted = false;
        self.secret.clear();
    }
}

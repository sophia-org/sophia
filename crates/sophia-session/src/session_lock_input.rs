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

/// The secret being typed. Its storage is one page, allocated once at full
/// capacity, locked in memory and left out of core dumps, so it is never
/// swapped, never reallocated and never dumped; every clear and drop zeroes
/// all of it.
pub struct SessionLockSecret {
    page: sophia_factotum_pam::LockedPage,
    len: usize,
}

/// No page could be both locked and left out of core dumps. A lock that
/// could not keep its secret so is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLockSecretUnavailable;

impl SessionLockSecret {
    pub fn new() -> Result<Self, SessionLockSecretUnavailable> {
        let page = sophia_factotum_pam::LockedPage::new(
            SESSION_LOCK_SECRET_CAPACITY.next_multiple_of(4096),
        )
        .ok_or(SessionLockSecretUnavailable)?;
        Ok(Self { page, len: 0 })
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
        self.page.as_mut_slice()[self.len..end].copy_from_slice(text.as_bytes());
        self.len = end;
        true
    }

    /// Removes the last character; `false` if the secret was empty.
    pub fn pop_char(&mut self) -> bool {
        let Some(last) = self.as_str().chars().next_back() else {
            return false;
        };
        let start = self.len - last.len_utf8();
        let removed = &mut self.page.as_mut_slice()[start..self.len];
        removed.fill(0);
        std::hint::black_box(removed);
        self.len = start;
        true
    }

    /// Zeroes the whole page, not only its used prefix.
    pub fn clear(&mut self) {
        self.page.zero();
        self.len = 0;
    }

    /// The secret, for handing to the authenticator and nothing else.
    pub fn as_str(&self) -> &str {
        // Only whole UTF-8 strings are ever appended and only whole characters
        // removed, so the prefix is always valid; an empty secret otherwise.
        std::str::from_utf8(&self.page.as_slice()[..self.len]).unwrap_or_default()
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
    /// Devices whose keys this lock consumed, in first-seen order, at most
    /// `HELD_DEVICES`; `held_reported` of them have been reported. Proof only:
    /// it names devices, never keys, and changes no routing.
    held_devices: Vec<DeviceId>,
    held_reported: usize,
}

/// The bound on devices a lock records as held.
const HELD_DEVICES: usize = 8;

impl SessionLockInput {
    pub fn new(keyboard: SessionLockKeyboard) -> Result<Self, SessionLockSecretUnavailable> {
        Ok(Self {
            keyboard,
            secret: SessionLockSecret::new()?,
            edits: Vec::new(),
            chords: Vec::new(),
            submitted: false,
            held_devices: Vec::new(),
            held_reported: 0,
        })
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
        if pressed && self.held_devices.len() < HELD_DEVICES && !self.held_devices.contains(&device)
        {
            self.held_devices.push(device);
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

    /// Devices this lock first consumed a pressed key from since the last call.
    pub fn take_unreported_held_devices(&mut self) -> &[DeviceId] {
        let start = self.held_reported;
        self.held_reported = self.held_devices.len();
        &self.held_devices[start..]
    }

    /// Edits since the last call, each with whether the secret was empty after.
    pub fn take_edits(&mut self) -> Vec<(SessionLockEdit, bool)> {
        std::mem::take(&mut self.edits)
    }

    /// The chords the current provider was granted; a chord's ID is its
    /// index. Replacing them drops any chord not yet reported.
    pub fn set_chords(&mut self, chords: Vec<sophia_engine::SessionLockChord>) {
        self.keyboard.set_chords(chords);
        self.chords.clear();
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

/// The authority that alone decides an unlock (`sophia-factotum`, t293).
///
/// Session hands it a submitted secret for one attempt and later polls for
/// the verdict; neither call blocks. A verdict names its attempt, and only
/// the current attempt of the current lock can end the lock.
pub trait SessionUnlockAuthenticator {
    /// Whether an attempt begun now could be decided. Session locks only
    /// while this holds.
    fn available(&self) -> bool;

    /// Starts verifying `secret` for `attempt`. The secret is borrowed only
    /// for this call; an `Err` means the attempt cannot be decided.
    fn begin(
        &mut self,
        attempt: crate::session_lock::SessionUnlockAttempt,
        secret: &str,
    ) -> Result<(), SessionUnlockUnavailable>;

    /// A verdict that has arrived, if any.
    fn poll(
        &mut self,
    ) -> Option<(
        crate::session_lock::SessionUnlockAttempt,
        crate::session_lock::SessionUnlockVerdict,
    )>;
}

/// The authenticator could not take the attempt: it is absent, failed or
/// replaced. The lock stays.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionUnlockUnavailable;

/// The lock keyboard's chords for a provider's granted requests, in order,
/// so each chord's ID stays its index. All or none: a request the keyboard
/// cannot hold (custody refuses those at negotiation) grants no chord at all
/// rather than shifting the others' IDs.
pub fn session_lock_chords(
    granted: &[sophia_protocol::lock_files::LockChordRequest],
) -> Vec<sophia_engine::SessionLockChord> {
    granted
        .iter()
        .map(|request| {
            Some(sophia_engine::SessionLockChord {
                keysym: request.keysym,
                modifiers: sophia_engine::SessionLockModifiers::for_chord(request.modifiers)?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

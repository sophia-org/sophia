//! Engine-owned keyboard handling while the session is locked.
//!
//! Every key the lock receives goes through this and only this. Committed
//! text goes to the caller's secret and nowhere else; editing keys and the
//! chords a lock provider registered become edits and opaque chord IDs. The
//! keymap and compose state are Engine's, as for the launcher, so no client
//! ever sees a keycode, keysym or character of the secret.

use xkbcommon::xkb;

/// A chord a lock provider registered: an XKB keysym and a modifier mask
/// holding at least one modifier other than Shift.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionLockChord {
    pub keysym: u32,
    pub modifiers: SessionLockModifiers,
}

/// Modifier bits, in the lock file contract's order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SessionLockModifiers(u16);

impl SessionLockModifiers {
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);

    /// `None` for unknown bits or a mask with no modifier besides Shift.
    pub const fn for_chord(bits: u16) -> Option<Self> {
        if bits & !0b1111 != 0 || bits & 0b1110 == 0 {
            None
        } else {
            Some(Self(bits))
        }
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    const fn with(self, other: Self, active: bool) -> Self {
        if active { Self(self.0 | other.0) } else { self }
    }
}

/// What one key press did to the lock. Releases do nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLockKey {
    /// Committed text was appended to the secret.
    Inserted,
    /// The secret's last character is to be removed.
    Delete,
    /// The secret is to be emptied without a submit.
    Clear,
    /// The secret is to be submitted.
    Submit,
    /// A registered chord, by its index.
    Chord(u16),
    /// A key that edits nothing: a dead key mid-composition, a modifier, or
    /// a command chord nobody registered.
    Ignored,
}

pub struct SessionLockKeyboard {
    state: xkb::State,
    compose: Option<xkb::compose::State>,
    chords: Vec<SessionLockChord>,
}

impl SessionLockKeyboard {
    pub fn new(
        rules: &str,
        model: &str,
        layout: &str,
        variant: &str,
        options: &str,
        locale: &std::ffi::OsStr,
    ) -> Result<Self, &'static str> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            rules,
            model,
            layout,
            variant,
            Some(options.to_owned()),
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or("lock keymap unavailable")?;
        let compose =
            xkb::compose::Table::new_from_locale(&context, locale, xkb::compose::COMPILE_NO_FLAGS)
                .ok()
                .map(|table| xkb::compose::State::new(&table, xkb::compose::STATE_NO_FLAGS));
        Ok(Self {
            state: xkb::State::new(&keymap),
            compose,
            chords: Vec::new(),
        })
    }

    /// Replaces the registered chords, as granted to the current provider.
    pub fn set_chords(&mut self, chords: Vec<SessionLockChord>) {
        self.chords = chords;
    }

    /// Drops any half-composed sequence, so a dead key typed before a lock,
    /// an unlock or a provider change never commits into the next secret.
    pub fn reset_composition(&mut self) {
        if let Some(compose) = self.compose.as_mut() {
            compose.reset();
        }
    }

    /// Feeds one evdev key. Every key updates the keymap state, so modifiers
    /// stay true across keys a session recognizer consumed; only a key with
    /// `edit` set can edit the secret or fire a chord. Committed text is
    /// handed to `secret` and nothing else retains it here.
    pub fn observe(
        &mut self,
        keycode: u32,
        pressed: bool,
        edit: bool,
        secret: &mut impl FnMut(&str),
    ) -> SessionLockKey {
        let key = xkb::Keycode::new(keycode.saturating_add(8));
        self.state.update_key(
            key,
            if pressed {
                xkb::KeyDirection::Down
            } else {
                xkb::KeyDirection::Up
            },
        );
        if !pressed || !edit {
            return SessionLockKey::Ignored;
        }
        let active = |name| {
            self.state
                .mod_name_is_active(name, xkb::STATE_MODS_EFFECTIVE)
        };
        let modifiers = SessionLockModifiers::default()
            .with(SessionLockModifiers::SHIFT, active(xkb::MOD_NAME_SHIFT))
            .with(SessionLockModifiers::CONTROL, active(xkb::MOD_NAME_CTRL))
            .with(SessionLockModifiers::ALT, active(xkb::MOD_NAME_ALT))
            .with(SessionLockModifiers::SUPER, active(xkb::MOD_NAME_LOGO));
        let keysym = self.state.key_get_one_sym(key).raw();
        if modifiers.bits() & !SessionLockModifiers::SHIFT.bits() != 0 {
            // A command chord never reaches the secret.
            self.reset_composition();
            if let Some(index) = self
                .chords
                .iter()
                .position(|chord| chord.keysym == keysym && chord.modifiers == modifiers)
            {
                return u16::try_from(index).map_or(SessionLockKey::Ignored, SessionLockKey::Chord);
            }
            return if modifiers == SessionLockModifiers::CONTROL && keysym == xkb::keysyms::KEY_u {
                SessionLockKey::Clear
            } else {
                SessionLockKey::Ignored
            };
        }
        match keysym {
            xkb::keysyms::KEY_Return | xkb::keysyms::KEY_KP_Enter => {
                self.reset_composition();
                return SessionLockKey::Submit;
            }
            xkb::keysyms::KEY_BackSpace => {
                self.reset_composition();
                return SessionLockKey::Delete;
            }
            xkb::keysyms::KEY_Escape => {
                self.reset_composition();
                return SessionLockKey::Clear;
            }
            _ => {}
        }
        if let Some(compose) = self.compose.as_mut() {
            compose.feed(xkb::Keysym::new(keysym));
            match compose.status() {
                xkb::compose::Status::Composing => return SessionLockKey::Ignored,
                xkb::compose::Status::Composed => {
                    let text = compose.utf8();
                    compose.reset();
                    return commit(text.as_deref(), secret);
                }
                xkb::compose::Status::Cancelled => {
                    compose.reset();
                    return SessionLockKey::Ignored;
                }
                xkb::compose::Status::Nothing => {}
            }
        }
        commit(Some(&self.state.key_get_utf8(key)), secret)
    }
}

/// Printable text only: a control character is an editing key handled
/// above, never part of a secret.
fn commit(text: Option<&str>, secret: &mut impl FnMut(&str)) -> SessionLockKey {
    match text {
        Some(text) if !text.is_empty() && !text.chars().any(char::is_control) => {
            secret(text);
            SessionLockKey::Inserted
        }
        _ => SessionLockKey::Ignored,
    }
}

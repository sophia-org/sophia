//! Server controls a client may set and read back: pointer acceleration,
//! the screen saver, and the keyboard's bell, click, LEDs and repeat.
//!
//! Advisory, and named so: this authority does not act on them. The Engine
//! owns pointer acceleration, the session owns key repeat, and nothing here
//! blanks a screen. What a client sets is what it reads back, validated as
//! the protocol validates it, so `xset` and its kind see a server that
//! keeps its word rather than one that refuses to speak.

/// Pointer acceleration and threshold, as GetPointerControl reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XPointerControl {
    pub acceleration_numerator: i16,
    pub acceleration_denominator: i16,
    pub threshold: i16,
}

impl Default for XPointerControl {
    /// The reference server's defaults: two to one past four pixels.
    fn default() -> Self {
        Self {
            acceleration_numerator: 2,
            acceleration_denominator: 1,
            threshold: 4,
        }
    }
}

/// The screen saver's timing and modes, as GetScreenSaver reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XScreenSaverControl {
    pub timeout: i16,
    pub interval: i16,
    pub prefer_blanking: u8,
    pub allow_exposures: u8,
}

pub const X_SCREEN_SAVER_DEFAULT_TIMEOUT: i16 = 600;
pub const X_SCREEN_SAVER_DEFAULT_INTERVAL: i16 = 600;

impl Default for XScreenSaverControl {
    /// The reference server's defaults: ten minutes, blanking preferred,
    /// exposures allowed.
    fn default() -> Self {
        Self {
            timeout: X_SCREEN_SAVER_DEFAULT_TIMEOUT,
            interval: X_SCREEN_SAVER_DEFAULT_INTERVAL,
            prefer_blanking: 1,
            allow_exposures: 1,
        }
    }
}

/// The keyboard's bell, click, LEDs and repeat, as GetKeyboardControl
/// reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XKeyboardControl {
    pub key_click_percent: u8,
    pub bell_percent: u8,
    pub bell_pitch: u16,
    pub bell_duration: u16,
    pub led_mask: u32,
    pub global_auto_repeat: u8,
    /// One bit per keycode, set when that key repeats.
    pub auto_repeats: [u8; 32],
}

impl Default for XKeyboardControl {
    /// What GetKeyboardControl reported before a client could change it:
    /// no click, a half-volume bell at 400 Hz for 100 ms, no LEDs, and every
    /// key repeating.
    fn default() -> Self {
        Self {
            key_click_percent: 0,
            bell_percent: 50,
            bell_pitch: 400,
            bell_duration: 100,
            led_mask: 0,
            global_auto_repeat: 1,
            auto_repeats: [0xff; 32],
        }
    }
}

/// One ChangeKeyboardControl request's values, in mask order, each present
/// when its bit is set. Validated at decode: a value outside its set is the
/// Value error the protocol names, carrying the value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XKeyboardControlChange {
    pub key_click_percent: Option<i8>,
    pub bell_percent: Option<i8>,
    pub bell_pitch: Option<i16>,
    pub bell_duration: Option<i16>,
    pub led: Option<u8>,
    pub led_mode: Option<u8>,
    pub key: Option<u8>,
    pub auto_repeat_mode: Option<u8>,
}

/// Why a ChangeKeyboardControl was refused whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XKeyboardControlRefusal {
    /// A led without a led-mode, or a key without an auto-repeat-mode: the
    /// Match error the protocol names.
    Match,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XServerControls {
    pub pointer: XPointerControl,
    pub screen_saver: XScreenSaverControl,
    pub keyboard: XKeyboardControl,
}

impl XServerControls {
    /// Applies a ChangeKeyboardControl, the protocol's way: -1 restores a
    /// default; a led without a mode, or a key without a repeat mode, is
    /// BadMatch, and the request is refused whole.
    pub fn change_keyboard(
        &mut self,
        change: XKeyboardControlChange,
    ) -> Result<(), XKeyboardControlRefusal> {
        let keyboard = &mut self.keyboard;
        let defaults = XKeyboardControl::default();
        if change.led.is_some() && change.led_mode.is_none() {
            return Err(XKeyboardControlRefusal::Match);
        }
        if change.key.is_some() && change.auto_repeat_mode.is_none() {
            return Err(XKeyboardControlRefusal::Match);
        }
        if let Some(percent) = change.key_click_percent {
            keyboard.key_click_percent = if percent < 0 {
                defaults.key_click_percent
            } else {
                percent as u8
            };
        }
        if let Some(percent) = change.bell_percent {
            keyboard.bell_percent = if percent < 0 {
                defaults.bell_percent
            } else {
                percent as u8
            };
        }
        if let Some(pitch) = change.bell_pitch {
            keyboard.bell_pitch = if pitch < 0 {
                defaults.bell_pitch
            } else {
                pitch as u16
            };
        }
        if let Some(duration) = change.bell_duration {
            keyboard.bell_duration = if duration < 0 {
                defaults.bell_duration
            } else {
                duration as u16
            };
        }
        if let Some(mode) = change.led_mode {
            let bits = match change.led {
                Some(led) => 1u32 << (led - 1),
                None => u32::MAX,
            };
            if mode == 1 {
                keyboard.led_mask |= bits;
            } else {
                keyboard.led_mask &= !bits;
            }
        }
        if let Some(mode) = change.auto_repeat_mode {
            match change.key {
                Some(key) => {
                    let (byte, bit) = (usize::from(key / 8), key % 8);
                    let on = match mode {
                        0 => false,
                        1 => true,
                        _ => keyboard.auto_repeats[byte] & (1 << bit) == 0,
                    };
                    if on {
                        keyboard.auto_repeats[byte] |= 1 << bit;
                    } else {
                        keyboard.auto_repeats[byte] &= !(1 << bit);
                    }
                }
                None => {
                    keyboard.global_auto_repeat = match mode {
                        0 => 0,
                        1 => 1,
                        _ => defaults.global_auto_repeat,
                    };
                }
            }
        }
        Ok(())
    }

    /// Applies a SetScreenSaver: -1 restores a default, 2 keeps a mode.
    pub fn set_screen_saver(
        &mut self,
        timeout: i16,
        interval: i16,
        prefer_blanking: u8,
        allow_exposures: u8,
    ) {
        let defaults = XScreenSaverControl::default();
        let saver = &mut self.screen_saver;
        saver.timeout = if timeout < 0 {
            defaults.timeout
        } else {
            timeout
        };
        saver.interval = if interval < 0 {
            defaults.interval
        } else {
            interval
        };
        if prefer_blanking < 2 {
            saver.prefer_blanking = prefer_blanking;
        }
        if allow_exposures < 2 {
            saver.allow_exposures = allow_exposures;
        }
    }

    /// Applies a ChangePointerControl for the parts it asks to change.
    pub fn change_pointer(
        &mut self,
        numerator: i16,
        denominator: i16,
        threshold: i16,
        do_acceleration: bool,
        do_threshold: bool,
    ) {
        let defaults = XPointerControl::default();
        if do_acceleration {
            if numerator < 0 {
                self.pointer.acceleration_numerator = defaults.acceleration_numerator;
                self.pointer.acceleration_denominator = defaults.acceleration_denominator;
            } else {
                self.pointer.acceleration_numerator = numerator;
                self.pointer.acceleration_denominator = denominator;
            }
        }
        if do_threshold {
            self.pointer.threshold = if threshold < 0 {
                defaults.threshold
            } else {
                threshold
            };
        }
    }
}

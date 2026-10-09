//! t292: keys while locked. The VT and emergency recognizers still act; every
//! other key edits only the lock's secret; providers learn edits, never text.

use sophia_engine::{SessionLockChord, SessionLockKeyboard, SessionLockModifiers};
use sophia_protocol::DeviceId;
use sophia_session::emergency_input::EmergencyChordState;
use sophia_session::session_keyboard::VirtualTerminalChordState;
use sophia_session::session_lock_input::{
    SESSION_LOCK_SECRET_CAPACITY, SessionLockEdit, SessionLockInput, SessionLockKeyOutcome,
    SessionLockSecret, session_lock_chords,
};

const A: u32 = 30;
const B: u32 = 48;
const ENTER: u32 = 28;
const BACKSPACE: u32 = 14;
const ESCAPE: u32 = 1;
const LEFT_CTRL: u32 = 29;
const LEFT_ALT: u32 = 56;
const F2: u32 = 60;

struct Seat {
    lock: SessionLockInput,
    emergency: EmergencyChordState,
    terminal: VirtualTerminalChordState,
    device: DeviceId,
}

impl Seat {
    fn new() -> Self {
        let keyboard = SessionLockKeyboard::new(
            "evdev",
            "pc105",
            "us",
            "",
            "",
            std::ffi::OsStr::new("C.UTF-8"),
        )
        .unwrap();
        Self {
            lock: SessionLockInput::new(keyboard).unwrap(),
            emergency: EmergencyChordState::default(),
            terminal: VirtualTerminalChordState::default(),
            device: DeviceId::from_raw(3),
        }
    }

    fn key(&mut self, keycode: u32, pressed: bool) -> SessionLockKeyOutcome {
        self.lock.observe_key(
            self.device,
            keycode,
            pressed,
            0,
            &mut self.emergency,
            &mut self.terminal,
        )
    }

    fn tap(&mut self, keycode: u32) -> SessionLockKeyOutcome {
        let outcome = self.key(keycode, true);
        self.key(keycode, false);
        outcome
    }

    fn secret(&self) -> Option<String> {
        self.lock
            .submission()
            .map(|secret| secret.as_str().to_owned())
    }
}

#[test]
fn typing_edits_the_secret_and_reports_edits_without_text() {
    let mut seat = Seat::new();
    for key in [A, B, A] {
        assert_eq!(seat.tap(key), SessionLockKeyOutcome::Consumed);
    }
    assert_eq!(seat.tap(BACKSPACE), SessionLockKeyOutcome::Consumed);
    assert_eq!(
        seat.lock.take_edits(),
        vec![
            (SessionLockEdit::Insert, false),
            (SessionLockEdit::Insert, false),
            (SessionLockEdit::Insert, false),
            (SessionLockEdit::Delete, false),
        ]
    );
    assert_eq!(seat.secret(), None, "nothing is submitted yet");
    seat.tap(ENTER);
    assert_eq!(
        seat.lock.take_edits(),
        vec![(SessionLockEdit::Submit, false)]
    );
    assert_eq!(seat.secret().as_deref(), Some("ab"));
}

#[test]
fn an_empty_secret_is_never_submitted_and_a_submission_is_not_repeated() {
    let mut seat = Seat::new();
    seat.tap(ENTER);
    assert_eq!(seat.secret(), None);
    assert!(seat.lock.take_edits().is_empty());
    seat.tap(A);
    seat.tap(ENTER);
    seat.tap(ENTER);
    assert_eq!(
        seat.lock.take_edits(),
        vec![
            (SessionLockEdit::Insert, false),
            (SessionLockEdit::Submit, false)
        ]
    );
    seat.lock.settle_submission();
    assert_eq!(seat.secret(), None, "the verdict ends the submission");
    seat.tap(BACKSPACE);
    assert!(
        seat.lock.take_edits().is_empty(),
        "the settled secret was zeroed"
    );
}

#[test]
fn clearing_and_deleting_report_when_the_secret_becomes_empty() {
    let mut seat = Seat::new();
    seat.tap(A);
    seat.tap(A);
    seat.tap(ESCAPE);
    seat.tap(ESCAPE);
    seat.tap(A);
    seat.tap(BACKSPACE);
    seat.tap(BACKSPACE);
    assert_eq!(
        seat.lock.take_edits(),
        vec![
            (SessionLockEdit::Insert, false),
            (SessionLockEdit::Insert, false),
            (SessionLockEdit::Clear, true),
            (SessionLockEdit::Insert, false),
            (SessionLockEdit::Delete, true),
        ],
        "clearing or deleting nothing reports nothing"
    );
}

#[test]
fn a_vt_switch_still_acts_and_its_keys_edit_nothing() {
    let mut seat = Seat::new();
    seat.key(LEFT_CTRL, true);
    seat.key(LEFT_ALT, true);
    assert_eq!(
        seat.key(F2, true),
        SessionLockKeyOutcome::VirtualTerminal {
            terminal: 2,
            trigger: F2,
            modifiers: seat.terminal.pressed_modifier_keycodes_for(seat.device),
        }
    );
    seat.key(F2, false);
    seat.key(LEFT_ALT, false);
    seat.key(LEFT_CTRL, false);
    assert!(seat.lock.take_edits().is_empty());
    assert!(seat.lock.take_chords().is_empty());
}

#[test]
fn the_emergency_chord_still_ends_the_session() {
    let mut seat = Seat::new();
    let chord = |seat: &mut Seat| {
        seat.key(LEFT_CTRL, true);
        seat.key(LEFT_ALT, true);
        let outcome = seat.key(BACKSPACE, true);
        seat.key(BACKSPACE, false);
        seat.key(LEFT_ALT, false);
        seat.key(LEFT_CTRL, false);
        outcome
    };
    // The first complete chord arms; the second fires.
    assert_eq!(chord(&mut seat), SessionLockKeyOutcome::Consumed);
    assert_eq!(chord(&mut seat), SessionLockKeyOutcome::EmergencyExit);
}

#[test]
fn a_registered_chord_reaches_the_provider_and_not_the_secret() {
    let mut seat = Seat::new();
    let mut keyboard = SessionLockKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap();
    keyboard.set_chords(vec![SessionLockChord {
        keysym: 0x62,
        modifiers: SessionLockModifiers::for_chord(SessionLockModifiers::ALT.bits()).unwrap(),
    }]);
    seat.lock = SessionLockInput::new(keyboard).unwrap();
    seat.key(LEFT_ALT, true);
    seat.tap(B);
    seat.key(LEFT_ALT, false);
    assert_eq!(seat.lock.take_chords(), vec![0]);
    assert!(seat.lock.take_edits().is_empty());
}

#[test]
fn the_secret_refuses_text_past_its_capacity_whole() {
    let mut secret = SessionLockSecret::new().unwrap();
    assert!(secret.push_str(&"x".repeat(SESSION_LOCK_SECRET_CAPACITY - 1)));
    assert!(!secret.push_str("é"), "two bytes do not fit in one");
    assert!(secret.push_str("y"));
    assert_eq!(secret.as_str().len(), SESSION_LOCK_SECRET_CAPACITY);
    assert!(secret.pop_char());
    assert!(secret.as_str().ends_with('x'));
    secret.clear();
    assert!(secret.is_empty());
    assert_eq!(format!("{secret:?}"), "SessionLockSecret(..)");
}

#[test]
fn deleting_removes_a_whole_character() {
    let mut secret = SessionLockSecret::new().unwrap();
    assert!(secret.push_str("aé"));
    assert!(secret.pop_char());
    assert_eq!(secret.as_str(), "a");
}

#[test]
fn a_providers_granted_chords_fire_by_index_and_never_edit_the_secret() {
    use sophia_protocol::lock_files::LockChordRequest;
    const ALT: u16 = 0b0100;
    let granted = [
        LockChordRequest {
            keysym: 0x61, // a
            modifiers: ALT,
        },
        LockChordRequest {
            keysym: 0x62, // b
            modifiers: ALT,
        },
    ];
    let mut seat = Seat::new();
    seat.lock.set_chords(session_lock_chords(&granted));
    seat.key(LEFT_ALT, true);
    seat.tap(B);
    seat.key(LEFT_ALT, false);
    assert_eq!(seat.lock.take_chords(), [1], "Alt+b is the second grant");
    assert!(
        seat.lock.take_edits().is_empty(),
        "nothing reached the secret"
    );
    // A replacement provider's grants replace the old ones.
    seat.lock.set_chords(session_lock_chords(&granted[1..]));
    seat.key(LEFT_ALT, true);
    seat.tap(B);
    seat.key(LEFT_ALT, false);
    assert_eq!(seat.lock.take_chords(), [0]);
    // No provider: Alt+b is not a chord.
    seat.lock.set_chords(Vec::new());
    seat.key(LEFT_ALT, true);
    seat.tap(B);
    seat.key(LEFT_ALT, false);
    assert!(seat.lock.take_chords().is_empty());
}

#[test]
fn granted_chords_convert_all_or_none_so_ids_never_shift() {
    use sophia_protocol::lock_files::LockChordRequest;
    let good = LockChordRequest {
        keysym: 0x62,
        modifiers: 0b0100,
    };
    let shift_only = LockChordRequest {
        keysym: 0x61,
        modifiers: 0b0001,
    };
    assert_eq!(session_lock_chords(&[good, good]).len(), 2);
    assert!(session_lock_chords(&[good, shift_only]).is_empty());
}

#[test]
fn consumed_keys_name_their_devices_once_bounded_and_without_keys() {
    let mut seat = Seat::new();
    assert!(seat.lock.take_unreported_held_devices().is_empty());
    seat.tap(A);
    seat.tap(B);
    assert_eq!(seat.lock.take_unreported_held_devices(), [DeviceId::from_raw(3)]);
    // A device already reported is not reported again for the same lock.
    seat.tap(A);
    assert!(seat.lock.take_unreported_held_devices().is_empty());
    // A release alone holds nothing; a new device's press is new.
    seat.device = DeviceId::from_raw(4);
    seat.key(A, false);
    assert!(seat.lock.take_unreported_held_devices().is_empty());
    seat.tap(ENTER);
    assert_eq!(seat.lock.take_unreported_held_devices(), [DeviceId::from_raw(4)]);
    // At most eight devices, so a flood of identities stays bounded.
    for raw in 10..30 {
        seat.device = DeviceId::from_raw(raw);
        seat.tap(A);
    }
    assert_eq!(seat.lock.take_unreported_held_devices().len(), 6);
    assert!(seat.lock.take_unreported_held_devices().is_empty());
}

#[test]
fn a_vt_switch_still_acts_after_its_modifiers_hold_the_device() {
    let mut seat = Seat::new();
    // The modifiers are consumed by the lock before the chord fires, so they
    // hold the device once; the trigger still switches and adds nothing.
    seat.key(LEFT_CTRL, true);
    seat.key(LEFT_ALT, true);
    assert_eq!(seat.lock.take_unreported_held_devices(), [DeviceId::from_raw(3)]);
    assert!(matches!(
        seat.key(F2, true),
        SessionLockKeyOutcome::VirtualTerminal { .. }
    ));
    assert!(seat.lock.take_unreported_held_devices().is_empty());
}

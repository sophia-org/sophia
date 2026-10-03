//! t292: the lock keyboard. Committed text reaches only the caller's secret;
//! editing keys and registered chords become edits and opaque chord IDs.

use sophia_engine::{SessionLockChord, SessionLockKey, SessionLockKeyboard, SessionLockModifiers};

const A: u32 = 30;
const B: u32 = 48;
const U: u32 = 22;
const ENTER: u32 = 28;
const BACKSPACE: u32 = 14;
const ESCAPE: u32 = 1;
const LEFT_SHIFT: u32 = 42;
const LEFT_CTRL: u32 = 29;
const LEFT_ALT: u32 = 56;
const XK_B_LOWER: u32 = 0x62;

fn keyboard() -> SessionLockKeyboard {
    SessionLockKeyboard::new(
        "evdev",
        "pc105",
        "us",
        "",
        "",
        std::ffi::OsStr::new("C.UTF-8"),
    )
    .unwrap()
}

/// Presses and releases one key, returning the press's effect and the text it
/// committed.
fn tap(keyboard: &mut SessionLockKeyboard, keycode: u32) -> (SessionLockKey, String) {
    let mut secret = String::new();
    let key = keyboard.observe(keycode, true, true, &mut |text| secret.push_str(text));
    assert_eq!(
        keyboard.observe(keycode, false, true, &mut |_| panic!(
            "a release committed text"
        )),
        SessionLockKey::Ignored
    );
    (key, secret)
}

fn hold(keyboard: &mut SessionLockKeyboard, keycode: u32, pressed: bool) {
    keyboard.observe(keycode, pressed, true, &mut |_| {
        panic!("a modifier committed text")
    });
}

#[test]
fn text_reaches_only_the_secret_and_is_shift_aware() {
    let mut keyboard = keyboard();
    assert_eq!(
        tap(&mut keyboard, A),
        (SessionLockKey::Inserted, "a".into())
    );
    hold(&mut keyboard, LEFT_SHIFT, true);
    assert_eq!(
        tap(&mut keyboard, A),
        (SessionLockKey::Inserted, "A".into())
    );
    hold(&mut keyboard, LEFT_SHIFT, false);
}

#[test]
fn editing_keys_are_edits_and_never_text() {
    let mut keyboard = keyboard();
    assert_eq!(
        tap(&mut keyboard, ENTER),
        (SessionLockKey::Submit, String::new())
    );
    assert_eq!(
        tap(&mut keyboard, BACKSPACE),
        (SessionLockKey::Delete, String::new())
    );
    assert_eq!(
        tap(&mut keyboard, ESCAPE),
        (SessionLockKey::Clear, String::new())
    );
    hold(&mut keyboard, LEFT_CTRL, true);
    assert_eq!(
        tap(&mut keyboard, U),
        (SessionLockKey::Clear, String::new())
    );
    hold(&mut keyboard, LEFT_CTRL, false);
}

#[test]
fn a_registered_chord_is_its_id_and_never_text() {
    let mut keyboard = keyboard();
    keyboard.set_chords(vec![SessionLockChord {
        keysym: XK_B_LOWER,
        modifiers: SessionLockModifiers::for_chord(SessionLockModifiers::ALT.bits()).unwrap(),
    }]);
    hold(&mut keyboard, LEFT_ALT, true);
    assert_eq!(
        tap(&mut keyboard, B),
        (SessionLockKey::Chord(0), String::new())
    );
    hold(&mut keyboard, LEFT_ALT, false);
    assert_eq!(
        tap(&mut keyboard, B),
        (SessionLockKey::Inserted, "b".into()),
        "the same key without its modifier is ordinary text"
    );
}

#[test]
fn an_unregistered_command_chord_does_nothing() {
    let mut keyboard = keyboard();
    hold(&mut keyboard, LEFT_ALT, true);
    assert_eq!(
        tap(&mut keyboard, B),
        (SessionLockKey::Ignored, String::new())
    );
    hold(&mut keyboard, LEFT_ALT, false);
}

#[test]
fn a_chord_must_hold_a_modifier_other_than_shift() {
    assert_eq!(SessionLockModifiers::for_chord(0), None);
    assert_eq!(
        SessionLockModifiers::for_chord(SessionLockModifiers::SHIFT.bits()),
        None
    );
    assert_eq!(SessionLockModifiers::for_chord(1 << 4), None, "unknown bit");
    assert!(
        SessionLockModifiers::for_chord(
            SessionLockModifiers::SHIFT.bits() | SessionLockModifiers::SUPER.bits()
        )
        .is_some()
    );
}

#[test]
fn a_key_a_recognizer_consumed_edits_nothing_but_keeps_modifiers_true() {
    let mut keyboard = keyboard();
    // Shift goes down while a session recognizer owns the key stream.
    keyboard.observe(LEFT_SHIFT, true, false, &mut |_| panic!("consumed"));
    assert_eq!(
        keyboard.observe(A, true, false, &mut |_| panic!(
            "consumed key committed text"
        )),
        SessionLockKey::Ignored
    );
    keyboard.observe(A, false, false, &mut |_| panic!("consumed"));
    // The modifier state followed anyway.
    assert_eq!(
        tap(&mut keyboard, A),
        (SessionLockKey::Inserted, "A".into())
    );
    keyboard.observe(LEFT_SHIFT, false, true, &mut |_| panic!("release"));
    assert_eq!(
        tap(&mut keyboard, A),
        (SessionLockKey::Inserted, "a".into())
    );
}

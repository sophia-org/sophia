//! Profile syntax for modifier taps, tap-versus-hold, key sequences and their
//! leaders (t277 D1): what a candidate admits, how each shape reads in help,
//! and what it refuses.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use sophia_config::{
    ConfigGeneration, DesktopAuthority, DesktopProfileError, DesktopSessionShortcut,
    DesktopShortcutCandidate, DesktopShortcutModifiers, DesktopShortcutTarget,
    DesktopShortcutTiming, desktop_shortcut_evdev_keycode, load_desktop_profile,
    prepare_desktop_shortcut_candidate,
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sophia-shortcut-shapes-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create test directory");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .expect("make test directory private");
        Self(path)
    }

    fn load(&self, shortcut: &str) -> Result<DesktopShortcutCandidate, DesktopProfileError> {
        let path = self.0.join("config.kdl");
        fs::write(&path, format!("schema 1\nshortcut {{\n{shortcut}\n}}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let profile = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL)?;
        prepare_desktop_shortcut_candidate(&profile.candidates[&DesktopAuthority::Shortcut])
    }

    fn refuses(&self, shortcut: &str, reason: &str) {
        match self.load(shortcut) {
            // A duplicate setting is refused while the profile loads, before
            // the shortcut candidate is prepared, so only the reason is named.
            Err(DesktopProfileError::Schema(message)) => {
                assert!(message.contains(reason), "{shortcut}: {message}")
            }
            other => panic!("{shortcut}: expected a refusal naming {reason:?}, got {other:?}"),
        }
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_shape_is_admitted_and_reads_in_help() {
    let profile = Profile::new();
    let candidate = profile
        .load(
            r#"profile "daily"
bind "Super" "policy:launcher"
bind "Super+q" "session:close-window"
bind "Super+q" "policy:force-close" hold-ms=500
bind "Super+x" "policy:power" hold-ms=1000
bind "Super+w k" "policy:kill"
bind "Super+w Super+j k l" "policy:deep"
leader "Super+w" "policy:hint"
shortcut-timing tap-ms=300 sequence-ms=2000"#,
        )
        .unwrap();
    let shown = candidate
        .bindings
        .iter()
        .map(|binding| candidate.binding_display(binding))
        .collect::<Vec<_>>();
    assert_eq!(
        shown,
        [
            "Super (tap)",
            "Super+Q",
            "Super+Q (hold)",
            "Super+X (hold only)",
            "Super+W, K",
            "Super+W, Super+J, K, L",
        ]
    );
    assert_eq!(
        candidate.bindings[0].modifier_tap(),
        Some(DesktopShortcutModifiers::SUPER)
    );
    assert_eq!(candidate.bindings[2].hold_ms, Some(500));
    assert_eq!(candidate.bindings[5].steps.len(), 3);
    assert_eq!(candidate.leaders.len(), 1);
    assert_eq!(candidate.leaders[0].action, "hint");
    assert_eq!(candidate.leaders[0].display(), "Super+W ...");
    assert_eq!(
        candidate.timing,
        DesktopShortcutTiming {
            tap_ms: 300,
            sequence_ms: 2000
        }
    );
}

#[test]
fn timing_defaults_when_absent_and_fills_an_omitted_value() {
    let profile = Profile::new();
    let absent = profile.load(r#"profile "daily""#).unwrap();
    assert_eq!(
        absent.timing,
        DesktopShortcutTiming {
            tap_ms: 400,
            sequence_ms: 1000
        }
    );
    let partial = profile
        .load("profile \"daily\"\nshortcut-timing sequence-ms=1500")
        .unwrap();
    assert_eq!(partial.timing.tap_ms, 400);
    assert_eq!(partial.timing.sequence_ms, 1500);
}

#[test]
fn every_modifier_class_has_a_tap_and_control_is_ctrl() {
    let profile = Profile::new();
    let candidate = profile
        .load(
            r#"profile "daily"
bind "Shift" "policy:a"
bind "Control" "policy:b"
bind "Alt" "policy:c"
bind "Super" "policy:d""#,
        )
        .unwrap();
    assert_eq!(
        candidate
            .bindings
            .iter()
            .map(|binding| binding.modifier_tap().unwrap())
            .collect::<Vec<_>>(),
        [
            DesktopShortcutModifiers::SHIFT,
            DesktopShortcutModifiers::CONTROL,
            DesktopShortcutModifiers::ALT,
            DesktopShortcutModifiers::SUPER,
        ]
    );
    profile.refuses(
        "profile \"daily\"\nbind \"Ctrl\" \"policy:a\"\nbind \"Control\" \"policy:b\"",
        "duplicate physical chord",
    );
}

#[test]
fn malformed_shapes_are_refused_by_name() {
    let profile = Profile::new();
    for (shortcut, reason) in [
        // Modifier taps.
        ("bind \"Super+Shift\" \"policy:a\"", "modifier tap"),
        ("bind \"Super\" \"policy:a\" hold-ms=500", "hold-ms"),
        ("bind \"Super w\" \"policy:a\"", "modifier tap"),
        // Holds.
        (
            "bind \"Super+q\" \"policy:a\" hold-ms=99",
            "hold-ms is out of range",
        ),
        (
            "bind \"Super+q\" \"policy:a\" hold-ms=5001",
            "hold-ms is out of range",
        ),
        (
            "bind \"Super+q\" \"policy:a\" hold-ms=\"500\"",
            "hold-ms is out of range",
        ),
        ("bind \"Super+w k\" \"policy:a\" hold-ms=500", "hold-ms"),
        (
            "pointer-bind \"Super+left\" \"policy:move\" hold-ms=500",
            "hold-ms",
        ),
        (
            "bind \"Super+q\" \"policy:a\" hold-ms=500\nbind \"super+Q\" \"policy:b\" hold-ms=600",
            "duplicate physical chord",
        ),
        // Sequences.
        ("bind \"Super+w a b c d\" \"policy:a\"", "number of steps"),
        ("bind \"Super+w  k\" \"policy:a\"", "trigger"),
        ("bind \"Super+w k \" \"policy:a\"", "trigger"),
        ("bind \"Super+w Escape\" \"policy:a\"", "Escape"),
        (
            "pointer-bind \"Super+left right\" \"policy:move\"",
            "sequences",
        ),
        (
            "bind \"Super+w\" \"policy:a\"\nbind \"Super+w k\" \"policy:b\"",
            "extend",
        ),
        (
            "bind \"Super+w k\" \"policy:a\"\nbind \"Super+w k j\" \"policy:b\"",
            "extend",
        ),
        (
            "bind \"Super+w k\" \"policy:a\"\nbind \"super+W K\" \"policy:b\"",
            "duplicate",
        ),
        // Leaders.
        ("leader \"Super+w\" \"policy:hint\"", "prefix a sequence"),
        (
            "bind \"Super+w k\" \"policy:a\"\nleader \"Super+w k\" \"policy:hint\"",
            "prefix a sequence",
        ),
        (
            "bind \"Super+w k\" \"policy:a\"\nleader \"Super+w\" \"session:logout\"",
            "policy action",
        ),
        (
            "bind \"Super+w k\" \"policy:a\"\nleader \"Super\" \"policy:hint\"",
            "modifier tap",
        ),
        (
            "bind \"Super+w k j\" \"policy:a\"\nleader \"Super+w\" \"policy:one\"\nleader \"Super+w k\" \"policy:two\"",
            "nest",
        ),
        (
            "bind \"Super+w k\" \"policy:a\"\nleader \"Super+w\" \"policy:one\"\nleader \"super+W\" \"policy:two\"",
            "duplicate leader",
        ),
        (
            "bind \"Super+w k\" \"policy:a\"\nleader \"Super+w\" \"policy:a\"",
            "anything else",
        ),
        (
            "bind \"Super+w k\" \"policy:a\"\nbind \"Super+e k\" \"policy:b\"\nleader \"Super+w\" \"policy:hint\"\nleader \"Super+e\" \"policy:hint\"",
            "anything else",
        ),
        (
            "bind \"Super+w k\" \"policy:a\"\nleader \"Super+w\" \"policy:h\" hold-ms=500",
            "leader requires",
        ),
        // Timing.
        ("shortcut-timing", "shortcut-timing"),
        ("shortcut-timing tap-ms=49", "tap-ms is out of range"),
        (
            "shortcut-timing sequence-ms=10001",
            "sequence-ms is out of range",
        ),
        ("shortcut-timing hold-ms=500", "shortcut-timing"),
        (
            "shortcut-timing tap-ms=400\nshortcut-timing tap-ms=500",
            "duplicate",
        ),
    ] {
        profile.refuses(&format!("profile \"daily\"\n{shortcut}"), reason);
    }
}

#[test]
fn virtual_terminal_chords_are_reserved_in_every_shape() {
    let profile = Profile::new();
    for (shortcut, terminal) in [
        ("bind \"Ctrl+Alt+f5\" \"session:restart-wm\"", 5),
        ("bind \"Super+Ctrl+Alt+F1\" \"policy:a\"", 1),
        ("bind \"Shift+Control+Alt+f12\" \"policy:a\"", 12),
        ("bind \"Ctrl+Alt+f2\" \"policy:a\" hold-ms=500", 2),
        ("bind \"Super+w Ctrl+Alt+f3\" \"policy:a\"", 3),
        ("bind \"Ctrl+Alt+f4 k\" \"policy:a\"", 4),
        (
            "bind \"Super+w k\" \"policy:a\"\nleader \"Ctrl+Alt+f6\" \"policy:hint\"",
            6,
        ),
    ] {
        profile.refuses(
            &format!("profile \"daily\"\n{shortcut}"),
            &format!("reserved virtual-terminal chord Ctrl+Alt+F{terminal} cannot be bound"),
        );
    }
    profile.refuses(
        "profile \"daily\"\nbind \"Super+w Ctrl+Alt+Backspace\" \"policy:a\"",
        "reserved emergency chord",
    );
    // Neither Ctrl+F5 nor Alt+F5 alone switches terminals.
    profile
        .load("profile \"daily\"\nbind \"Super+Ctrl+f5\" \"session:restart-wm\"\nbind \"Alt+f5\" \"policy:a\"")
        .unwrap();
}

#[test]
fn bindings_and_leaders_share_the_catalog_bound() {
    let profile = Profile::new();
    // 255 distinct two-step sequences, then leaders up to the bound.
    let keys = "abcdefghijklmnopqrstuvwxyz0123456789";
    let mut source = String::from("profile \"daily\"\n");
    let mut count = 0;
    'fill: for first in keys.chars() {
        for second in keys.chars() {
            if count == 255 {
                break 'fill;
            }
            source.push_str(&format!("bind \"Super+{first} {second}\" \"policy:a\"\n"));
            count += 1;
        }
    }
    let at_bound = format!("{source}leader \"Super+a\" \"policy:hint\"\n");
    assert_eq!(profile.load(&at_bound).unwrap().leaders.len(), 1);
    profile.refuses(
        &format!("{at_bound}leader \"Super+b\" \"policy:other\"\n"),
        "binding count exceeds 256",
    );
}

#[test]
fn a_help_row_must_fit_the_catalog_width() {
    let profile = Profile::new();
    // The trigger fits its 64 bytes, but each step separator renders one
    // byte wider in help.
    profile.refuses(
        "profile \"daily\"\nbind \"Super+Ctrl+Shift+Alt+page_down Super+Ctrl+Shift+Alt+page_up k j\" \"policy:a\"",
        "help",
    );
}

#[test]
fn a_hold_variant_is_its_own_profile_setting() {
    let profile = Profile::new();
    let candidate = profile
        .load(
            "profile \"daily\"\nbind \"Super+q\" \"policy:a\"\nbind \"Super+q\" \"policy:b\" hold-ms=500",
        )
        .unwrap();
    assert_eq!(candidate.bindings.len(), 2);
    profile.refuses(
        "profile \"daily\"\nbind \"Super+q\" \"policy:a\"\nbind \"Super+q\" \"policy:b\"",
        "duplicate",
    );
}

/// Every key name the profile accepts prepares to one spelling per physical
/// key and modifier set, so shape checks agree with resolution. The list
/// mirrors desktop_shortcut_evdev_keycode's arms.
#[test]
fn every_key_alias_prepares_to_one_physical_spelling() {
    let profile = Profile::new();
    let mut spelling = std::collections::BTreeMap::new();
    for name in [
        "escape",
        "f1",
        "f2",
        "f3",
        "f4",
        "f5",
        "f6",
        "f7",
        "f8",
        "f9",
        "f10",
        "f11",
        "f12",
        "1",
        "2",
        "3",
        "4",
        "5",
        "6",
        "7",
        "8",
        "9",
        "0",
        "-",
        "=",
        "backspace",
        "tab",
        "q",
        "w",
        "e",
        "r",
        "t",
        "y",
        "u",
        "i",
        "o",
        "p",
        "[",
        "]",
        "return",
        "enter",
        "a",
        "s",
        "d",
        "f",
        "g",
        "h",
        "j",
        "k",
        "l",
        "grave",
        "z",
        "x",
        "c",
        "v",
        "b",
        "n",
        "m",
        ",",
        ".",
        "?",
        "/",
        "slash",
        "question",
        "space",
        "print",
        "up",
        "left",
        "right",
        "down",
        "home",
        "page_up",
        "end",
        "page_down",
        "insert",
        "delete",
    ] {
        let source = format!("profile \"daily\"\nbind \"Super+{name}\" \"policy:a\"");
        let chord = profile.load(&source).unwrap().bindings.remove(0).chord;
        let keycode = desktop_shortcut_evdev_keycode(&chord.trigger)
            .unwrap_or_else(|| panic!("{name} prepared to an unresolvable {:?}", chord.trigger));
        let physical = (keycode, chord.modifiers);
        let first = spelling
            .entry(physical)
            .or_insert_with(|| chord.trigger.clone());
        assert_eq!(
            *first, chord.trigger,
            "{name} is a second spelling of {physical:?}"
        );
    }
}

/// Enter and Return are one key: every shape check sees them as one (D1
/// review R1).
#[test]
fn enter_and_return_are_one_physical_shape() {
    let profile = Profile::new();
    for shortcut in [
        "bind \"Super+Return k\" \"policy:a\"\nbind \"Super+Enter k\" \"policy:b\"",
        "bind \"Super+Return\" \"policy:a\" hold-ms=500\nbind \"Super+Enter\" \"policy:b\" hold-ms=600",
    ] {
        profile.refuses(
            &format!("profile \"daily\"\n{shortcut}"),
            "duplicate physical chord",
        );
    }
    profile.refuses(
        "profile \"daily\"\nbind \"Super+Return\" \"policy:a\"\nbind \"Super+Enter k\" \"policy:b\"",
        "extend",
    );
    let leader = profile
        .load("profile \"daily\"\nbind \"Super+Return k\" \"policy:a\"\nleader \"Super+Enter\" \"policy:hint\"")
        .unwrap();
    assert_eq!(leader.leaders[0].display(), "Super+Enter ...");
    let pair = profile
        .load("profile \"daily\"\nbind \"Super+Return\" \"policy:a\"\nbind \"Super+Enter\" \"policy:b\" hold-ms=500")
        .unwrap();
    assert_eq!(
        pair.bindings
            .iter()
            .map(|binding| pair.binding_display(binding))
            .collect::<Vec<_>>(),
        ["Super+Enter", "Super+Enter (hold)"]
    );
}

#[test]
fn session_lock_is_a_session_shortcut() {
    let profile = Profile::new();
    let candidate = profile
        .load("profile \"daily\"\nbind \"Super+l\" \"session:lock\"")
        .unwrap();
    assert_eq!(
        candidate.bindings[0].target,
        DesktopShortcutTarget::Session(DesktopSessionShortcut::Lock)
    );
    assert_eq!(DesktopSessionShortcut::Lock.profile_name(), "lock");
}

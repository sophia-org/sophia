use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use kdl::{KdlDocument, KdlNode};

use crate::{
    ConfigDigest, ConfigGeneration, DesktopAuthority, DesktopAuthorityCandidate,
    DesktopProfileError,
};

pub const DESKTOP_SHORTCUT_MAX_BINDINGS: usize = 256;
pub const DESKTOP_SHORTCUT_MAX_TRIGGER_BYTES: usize = 64;
pub const DESKTOP_SHORTCUT_MAX_TARGET_BYTES: usize = 128;
/// Steps in a key sequence, its first chord included.
pub const DESKTOP_SHORTCUT_MAX_SEQUENCE_STEPS: usize = 4;
/// The width of a help row's chord text in the shell's shortcut catalog.
pub const DESKTOP_SHORTCUT_MAX_DISPLAY_BYTES: usize = 64;
pub const DESKTOP_SHORTCUT_HOLD_MS: RangeInclusive<u32> = 100..=5000;
pub const DESKTOP_SHORTCUT_TAP_MS: RangeInclusive<u32> = 50..=2000;
pub const DESKTOP_SHORTCUT_SEQUENCE_MS: RangeInclusive<u32> = 200..=10000;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DesktopShortcutModifiers(u8);

impl DesktopShortcutModifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const ALT: Self = Self(1 << 2);
    pub const SUPER: Self = Self(1 << 3);

    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DesktopShortcutBindingKind {
    Key,
    Pointer,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DesktopShortcutChord {
    pub kind: DesktopShortcutBindingKind,
    pub modifiers: DesktopShortcutModifiers,
    pub trigger: String,
}

impl DesktopShortcutChord {
    /// The modifier class a lone modifier tap names, as in `bind "Super" ..`.
    /// Such a chord fires on the modifier's release, never as a key press.
    pub fn modifier_tap(&self) -> Option<DesktopShortcutModifiers> {
        if self.kind != DesktopShortcutBindingKind::Key || self.modifiers.bits() != 0 {
            return None;
        }
        modifier_class(&self.trigger)
    }

    /// How the chord reads in help: modifiers in a fixed order, then the key.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        for (bit, name) in [
            (DesktopShortcutModifiers::SUPER, "Super"),
            (DesktopShortcutModifiers::CONTROL, "Ctrl"),
            (DesktopShortcutModifiers::SHIFT, "Shift"),
            (DesktopShortcutModifiers::ALT, "Alt"),
        ] {
            if self.modifiers.bits() & bit.bits() != 0 {
                parts.push(name.to_owned());
            }
        }
        let trigger = match self.trigger.as_str() {
            "slash" => "/".to_owned(),
            "return" => "Enter".to_owned(),
            trigger if self.kind == DesktopShortcutBindingKind::Pointer => {
                format!("Mouse {trigger}")
            }
            trigger => {
                let mut characters = trigger.chars();
                characters.next().map_or(String::new(), |first| {
                    first.to_uppercase().collect::<String>() + characters.as_str()
                })
            }
        };
        parts.push(trigger);
        parts.join("+")
    }
}

fn modifier_class(trigger: &str) -> Option<DesktopShortcutModifiers> {
    Some(match trigger {
        "shift" => DesktopShortcutModifiers::SHIFT,
        "ctrl" => DesktopShortcutModifiers::CONTROL,
        "alt" => DesktopShortcutModifiers::ALT,
        "super" => DesktopShortcutModifiers::SUPER,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopSessionShortcut {
    CloseFocused,
    Logout,
    LaunchTerminal,
    LaunchBrowser,
    WindowSwitcher,
    ShortcutHelp,
    ApplicationLauncher,
    /// Re-read the desktop profile and put it into effect, the way a window
    /// manager that ships its config in a file has to offer.
    ReloadProfile,
    /// Replace the policy client with a fresh process, keeping the windows.
    RestartWm,
}

impl DesktopSessionShortcut {
    /// The name this shortcut is written as in a profile, and the name it is
    /// reported by. One vocabulary, so a record naming a dropped shortcut can
    /// be matched against the profile line that asked for it.
    pub const fn profile_name(self) -> &'static str {
        match self {
            Self::CloseFocused => "close-window",
            Self::Logout => "logout",
            Self::LaunchTerminal => "spawn-terminal",
            Self::LaunchBrowser => "spawn-browser",
            Self::WindowSwitcher => "window-switcher",
            Self::ShortcutHelp => "shortcut-help",
            Self::ApplicationLauncher => "application-launcher",
            Self::ReloadProfile => "reload-profile",
            Self::RestartWm => "restart-wm",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesktopShortcutTarget {
    PolicyAction(String),
    Session(DesktopSessionShortcut),
    LaunchApplication(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopShortcutBinding {
    /// The first chord. A sequence continues with `steps`.
    pub chord: DesktopShortcutChord,
    pub steps: Vec<DesktopShortcutChord>,
    /// A hold variant: fires when the chord is still held after this long.
    /// The binding on the same chord without it is the tap variant.
    pub hold_ms: Option<u32>,
    pub target: DesktopShortcutTarget,
    pub label: Option<String>,
    pub group: Option<String>,
}

impl DesktopShortcutBinding {
    /// Every chord of the binding in order: one, or a sequence's steps.
    pub fn path(&self) -> impl Iterator<Item = &DesktopShortcutChord> {
        core::iter::once(&self.chord).chain(&self.steps)
    }

    /// A lone modifier tap, as in `bind "Super" ..`.
    pub fn modifier_tap(&self) -> Option<DesktopShortcutModifiers> {
        (self.steps.is_empty() && self.hold_ms.is_none())
            .then(|| self.chord.modifier_tap())
            .flatten()
    }
}

/// An action fired when a pending sequence reaches its prefix, such as a hint
/// of the keys that may follow. It is always a policy action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopShortcutLeader {
    pub chord: DesktopShortcutChord,
    pub steps: Vec<DesktopShortcutChord>,
    pub action: String,
    pub label: Option<String>,
    pub group: Option<String>,
}

impl DesktopShortcutLeader {
    pub fn path(&self) -> impl Iterator<Item = &DesktopShortcutChord> {
        core::iter::once(&self.chord).chain(&self.steps)
    }

    pub fn display(&self) -> String {
        format!("{} ...", display_path(self.path()))
    }
}

/// How long a modifier tap may be held, and how long a sequence waits for its
/// next step.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DesktopShortcutTiming {
    pub tap_ms: u32,
    pub sequence_ms: u32,
}

impl Default for DesktopShortcutTiming {
    fn default() -> Self {
        Self {
            tap_ms: 400,
            sequence_ms: 1000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopShortcutCandidate {
    pub generation: ConfigGeneration,
    pub digest: ConfigDigest,
    pub profile: String,
    pub bindings: Vec<DesktopShortcutBinding>,
    pub leaders: Vec<DesktopShortcutLeader>,
    pub timing: DesktopShortcutTiming,
}

impl DesktopShortcutCandidate {
    /// How a binding reads in help: its chords, then what kind of press it is.
    pub fn binding_display(&self, binding: &DesktopShortcutBinding) -> String {
        let path = display_path(binding.path());
        if binding.modifier_tap().is_some() {
            return format!("{path} (tap)");
        }
        let Some(_) = binding.hold_ms else {
            return path;
        };
        let tapped = self.bindings.iter().any(|other| {
            other.hold_ms.is_none() && other.chord == binding.chord && other.steps.is_empty()
        });
        if tapped {
            format!("{path} (hold)")
        } else {
            format!("{path} (hold only)")
        }
    }
}

fn display_path<'a>(path: impl Iterator<Item = &'a DesktopShortcutChord>) -> String {
    path.map(DesktopShortcutChord::display)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The settings a profile's shortcut section may contain.
pub(crate) const SETTINGS: [&str; 5] = [
    "profile",
    "bind",
    "pointer-bind",
    "leader",
    "shortcut-timing",
];

/// The name a shortcut setting is keyed by. A hold variant shares its trigger
/// with the tap variant on that chord, so it is keyed apart.
pub(crate) fn setting_name(node: &KdlNode) -> &str {
    match node.name().value() {
        "bind" if node.get("hold-ms").is_some() => "bind-hold",
        name => name,
    }
}

fn schema_error(message: impl Into<String>) -> DesktopProfileError {
    DesktopProfileError::Schema(format!("shortcut candidate: {}", message.into()))
}

fn single_node(encoded: &str) -> Result<KdlNode, DesktopProfileError> {
    let document = KdlDocument::parse_v2(encoded)
        .map_err(|error| schema_error(format!("invalid staged value: {error}")))?;
    if document.nodes().len() != 1 {
        return Err(schema_error("staged value must contain exactly one node"));
    }
    Ok(document.nodes()[0].clone())
}

fn positional_string(node: &KdlNode, index: usize) -> Option<&str> {
    node.get(index).and_then(|value| value.as_string())
}

fn exact_profile(node: &KdlNode) -> Result<String, DesktopProfileError> {
    if node.entries().len() != 1 || node.children().is_some() || node.ty().is_some() {
        return Err(schema_error("profile requires one string argument"));
    }
    let profile = positional_string(node, 0)
        .filter(|profile| !profile.is_empty() && profile.len() <= 64)
        .ok_or_else(|| schema_error("profile identity is invalid"))?;
    if !profile
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(schema_error(
            "profile identity contains unsupported characters",
        ));
    }
    Ok(profile.to_owned())
}

fn parse_chord(
    kind: DesktopShortcutBindingKind,
    source: &str,
) -> Result<DesktopShortcutChord, DesktopProfileError> {
    if source.is_empty() || source.len() > DESKTOP_SHORTCUT_MAX_TRIGGER_BYTES {
        return Err(schema_error("trigger length is invalid"));
    }
    let parts = source.split('+').collect::<Vec<_>>();
    let (trigger, modifiers) = parts
        .split_last()
        .ok_or_else(|| schema_error("trigger is empty"))?;
    if trigger.is_empty()
        || !trigger.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'_' | b'-' | b'=' | b'?' | b'/' | b',' | b'.' | b'[' | b']'
                )
        })
    {
        return Err(schema_error("trigger key contains unsupported characters"));
    }
    let mut modifier_bits = 0_u8;
    for modifier in modifiers {
        let bit = match modifier.to_ascii_lowercase().as_str() {
            "shift" => DesktopShortcutModifiers::SHIFT.bits(),
            "ctrl" | "control" => DesktopShortcutModifiers::CONTROL.bits(),
            "alt" => DesktopShortcutModifiers::ALT.bits(),
            "super" => DesktopShortcutModifiers::SUPER.bits(),
            _ => return Err(schema_error(format!("unsupported modifier {modifier:?}"))),
        };
        if modifier_bits & bit != 0 {
            return Err(schema_error(format!("duplicate modifier {modifier:?}")));
        }
        modifier_bits |= bit;
    }
    let trigger = match trigger.to_ascii_lowercase().as_str() {
        "?" | "question" => {
            modifier_bits |= DesktopShortcutModifiers::SHIFT.bits();
            "slash".to_owned()
        }
        "/" | "slash" => "slash".to_owned(),
        // One spelling per physical key, so shapes compare as resolution
        // will see them (both are evdev 28).
        "enter" => "return".to_owned(),
        "control" => "ctrl".to_owned(),
        _ => trigger.to_ascii_lowercase(),
    };
    if kind == DesktopShortcutBindingKind::Key
        && modifier_class(&trigger).is_some()
        && modifier_bits != 0
    {
        return Err(schema_error(
            "a modifier tap names one modifier and nothing else",
        ));
    }
    if kind == DesktopShortcutBindingKind::Pointer
        && !["left", "middle", "right"].contains(&trigger.as_str())
    {
        return Err(schema_error(
            "pointer trigger must name left, middle, or right",
        ));
    }
    if kind == DesktopShortcutBindingKind::Key
        && trigger == "backspace"
        && modifier_bits
            & (DesktopShortcutModifiers::CONTROL.bits() | DesktopShortcutModifiers::ALT.bits())
            == DesktopShortcutModifiers::CONTROL.bits() | DesktopShortcutModifiers::ALT.bits()
    {
        return Err(schema_error(
            "reserved emergency chord cannot be overridden",
        ));
    }
    // Ctrl+Alt+F1..F12 switch virtual terminals before any shortcut is
    // matched, whatever other modifiers are down.
    let control_alt =
        DesktopShortcutModifiers::CONTROL.bits() | DesktopShortcutModifiers::ALT.bits();
    if kind == DesktopShortcutBindingKind::Key
        && modifier_bits & control_alt == control_alt
        && let Some(terminal) = trigger
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
            .filter(|number| (1..=12).contains(number))
    {
        return Err(schema_error(format!(
            "reserved virtual-terminal chord Ctrl+Alt+F{terminal} cannot be bound"
        )));
    }
    Ok(DesktopShortcutChord {
        kind,
        modifiers: DesktopShortcutModifiers(modifier_bits),
        trigger,
    })
}

/// Resolve normalized profile key names to the evdev key identity consumed by
/// the session input authority. This table is deliberately independent from
/// policy action semantics.
pub fn desktop_shortcut_evdev_keycode(trigger: &str) -> Option<u32> {
    Some(match trigger {
        "escape" => 1,
        // Function keys. F1..F10 are contiguous from 59; F11 and F12 sit
        // after the numeric block rather than continuing it, which is an
        // accident of the original keyboard and not a mistake here.
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "f6" => 64,
        "f7" => 65,
        "f8" => 66,
        "f9" => 67,
        "f10" => 68,
        "f11" => 87,
        "f12" => 88,
        "1" => 2,
        "2" => 3,
        "3" => 4,
        "4" => 5,
        "5" => 6,
        "6" => 7,
        "7" => 8,
        "8" => 9,
        "9" => 10,
        "0" => 11,
        "-" => 12,
        "=" => 13,
        "backspace" => 14,
        "tab" => 15,
        "q" => 16,
        "w" => 17,
        "e" => 18,
        "r" => 19,
        "t" => 20,
        "y" => 21,
        "u" => 22,
        "i" => 23,
        "o" => 24,
        "p" => 25,
        "[" => 26,
        "]" => 27,
        "return" | "enter" => 28,
        "a" => 30,
        "s" => 31,
        "d" => 32,
        "f" => 33,
        "g" => 34,
        "h" => 35,
        "j" => 36,
        "k" => 37,
        "l" => 38,
        "grave" => 41,
        "z" => 44,
        "x" => 45,
        "c" => 46,
        "v" => 47,
        "b" => 48,
        "n" => 49,
        "m" => 50,
        "," => 51,
        "." => 52,
        "?" | "/" | "slash" | "question" => 53,
        "space" => 57,
        "print" => 99,
        "up" => 103,
        "left" => 105,
        "right" => 106,
        "down" => 108,
        "home" => 102,
        "page_up" => 104,
        "end" => 107,
        "page_down" => 109,
        "insert" => 110,
        "delete" => 111,
        _ => return None,
    })
}

fn policy_action(target: &str) -> Result<String, DesktopProfileError> {
    if target.is_empty()
        || target.trim() != target
        || target.len() > DESKTOP_SHORTCUT_MAX_TARGET_BYTES
    {
        return Err(schema_error("policy action length is invalid"));
    }
    if !target
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b' ' | b'.'))
    {
        return Err(schema_error(
            "policy action contains unsupported characters",
        ));
    }
    Ok(target.to_owned())
}

fn parse_target(
    kind: DesktopShortcutBindingKind,
    source: &str,
) -> Result<DesktopShortcutTarget, DesktopProfileError> {
    if source.len() > DESKTOP_SHORTCUT_MAX_TARGET_BYTES {
        return Err(schema_error("target length is invalid"));
    }
    let (authority, target) = source
        .split_once(':')
        .ok_or_else(|| schema_error("target must have an explicit authority prefix"))?;
    match authority {
        "application" if kind == DesktopShortcutBindingKind::Key => {
            Ok(DesktopShortcutTarget::LaunchApplication(
                crate::application_command::application_identity(target)?.to_owned(),
            ))
        }
        "policy" => Ok(DesktopShortcutTarget::PolicyAction(policy_action(target)?)),
        "session" if kind == DesktopShortcutBindingKind::Pointer => Err(schema_error(
            "pointer bindings cannot invoke session capabilities",
        )),
        "session" => {
            let shortcut = match target {
                "close-window" => DesktopSessionShortcut::CloseFocused,
                "logout" => DesktopSessionShortcut::Logout,
                "spawn-terminal" => DesktopSessionShortcut::LaunchTerminal,
                "spawn-browser" => DesktopSessionShortcut::LaunchBrowser,
                "window-switcher" => DesktopSessionShortcut::WindowSwitcher,
                "shortcut-help" => DesktopSessionShortcut::ShortcutHelp,
                "application-launcher" => DesktopSessionShortcut::ApplicationLauncher,
                "reload-profile" => DesktopSessionShortcut::ReloadProfile,
                "restart-wm" => DesktopSessionShortcut::RestartWm,
                _ => return Err(schema_error("unknown session shortcut capability")),
            };
            Ok(DesktopShortcutTarget::Session(shortcut))
        }
        _ => Err(schema_error(format!(
            "unsupported shortcut target authority {authority:?}"
        ))),
    }
}

/// A trigger's chords: one, or a key sequence of steps one space apart.
fn parse_path(
    kind: DesktopShortcutBindingKind,
    source: &str,
    steps: RangeInclusive<usize>,
) -> Result<(DesktopShortcutChord, Vec<DesktopShortcutChord>), DesktopProfileError> {
    if source.is_empty() || source.len() > DESKTOP_SHORTCUT_MAX_TRIGGER_BYTES {
        return Err(schema_error("trigger length is invalid"));
    }
    let parts = source.split(' ').collect::<Vec<_>>();
    if !steps.contains(&parts.len()) {
        return Err(schema_error("trigger has an unsupported number of steps"));
    }
    if parts.len() > 1 && kind != DesktopShortcutBindingKind::Key {
        return Err(schema_error("only key bindings may be sequences"));
    }
    let mut chords = parts
        .into_iter()
        .map(|part| parse_chord(kind, part))
        .collect::<Result<Vec<_>, _>>()?;
    if chords.len() > 1 {
        if chords.iter().any(|chord| chord.modifier_tap().is_some()) {
            return Err(schema_error("a sequence step cannot be a modifier tap"));
        }
        if chords[1..].iter().any(|chord| chord.trigger == "escape") {
            return Err(schema_error(
                "Escape abandons a sequence and cannot continue one",
            ));
        }
    }
    let first = chords.remove(0);
    Ok((first, chords))
}

fn integer_property(
    node: &KdlNode,
    name: &str,
    range: RangeInclusive<u32>,
) -> Result<Option<u32>, DesktopProfileError> {
    let entries = node
        .entries()
        .iter()
        .filter(|entry| entry.name().is_some_and(|entry| entry.value() == name))
        .collect::<Vec<_>>();
    match entries.as_slice() {
        [] => Ok(None),
        [entry] => entry
            .value()
            .as_integer()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| range.contains(value))
            .map(Some)
            .ok_or_else(|| schema_error(format!("{name} is out of range"))),
        _ => Err(schema_error(format!("duplicate {name}"))),
    }
}

fn display_metadata(
    node: &KdlNode,
    name: &str,
    limit: usize,
) -> Result<Option<String>, DesktopProfileError> {
    if node
        .entries()
        .iter()
        .filter(|e| e.name().is_some_and(|n| n.value() == name))
        .count()
        > 1
    {
        return Err(schema_error("duplicate binding metadata"));
    }
    node.get(name)
        .map(|v| {
            v.as_string()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= limit
                        && !s.chars().any(|c| {
                            c.is_control()
                                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                        })
                })
                .map(str::to_owned)
                .ok_or_else(|| schema_error("invalid binding metadata"))
        })
        .transpose()
}

fn parse_binding(node: &KdlNode) -> Result<DesktopShortcutBinding, DesktopProfileError> {
    if node.entries().iter().filter(|e| e.name().is_none()).count() != 2
        || node.children().is_some()
        || node.ty().is_some()
        || node
            .entries()
            .iter()
            .filter_map(|e| e.name())
            .any(|n| !matches!(n.value(), "label" | "group" | "hold-ms"))
    {
        return Err(schema_error(
            "binding requires trigger and target strings with optional label/group/hold-ms",
        ));
    }
    let kind = match node.name().value() {
        "bind" => DesktopShortcutBindingKind::Key,
        "pointer-bind" => DesktopShortcutBindingKind::Pointer,
        _ => return Err(schema_error("unsupported binding kind")),
    };
    let trigger = positional_string(node, 0)
        .ok_or_else(|| schema_error("binding trigger must be a string"))?;
    let target = positional_string(node, 1)
        .ok_or_else(|| schema_error("binding target must be a string"))?;
    let (chord, steps) = parse_path(kind, trigger, 1..=DESKTOP_SHORTCUT_MAX_SEQUENCE_STEPS)?;
    let hold_ms = integer_property(node, "hold-ms", DESKTOP_SHORTCUT_HOLD_MS)?;
    if hold_ms.is_some()
        && (kind != DesktopShortcutBindingKind::Key
            || !steps.is_empty()
            || chord.modifier_tap().is_some())
    {
        return Err(schema_error(
            "hold-ms applies only to a single key chord with a key",
        ));
    }
    Ok(DesktopShortcutBinding {
        chord,
        steps,
        hold_ms,
        target: parse_target(kind, target)?,
        label: display_metadata(node, "label", 128)?,
        group: display_metadata(node, "group", 64)?,
    })
}

fn parse_leader(node: &KdlNode) -> Result<DesktopShortcutLeader, DesktopProfileError> {
    if node.entries().iter().filter(|e| e.name().is_none()).count() != 2
        || node.children().is_some()
        || node.ty().is_some()
        || node
            .entries()
            .iter()
            .filter_map(|e| e.name())
            .any(|n| !matches!(n.value(), "label" | "group"))
    {
        return Err(schema_error(
            "leader requires prefix and target strings with optional label/group",
        ));
    }
    let prefix =
        positional_string(node, 0).ok_or_else(|| schema_error("leader prefix must be a string"))?;
    let target =
        positional_string(node, 1).ok_or_else(|| schema_error("leader target must be a string"))?;
    let (chord, steps) = parse_path(
        DesktopShortcutBindingKind::Key,
        prefix,
        1..=DESKTOP_SHORTCUT_MAX_SEQUENCE_STEPS - 1,
    )?;
    if chord.modifier_tap().is_some() {
        return Err(schema_error("a leader prefix cannot be a modifier tap"));
    }
    let action = target
        .strip_prefix("policy:")
        .ok_or_else(|| schema_error("a leader target must be a policy action"))?;
    Ok(DesktopShortcutLeader {
        chord,
        steps,
        action: policy_action(action)?,
        label: display_metadata(node, "label", 128)?,
        group: display_metadata(node, "group", 64)?,
    })
}

fn parse_timing(node: &KdlNode) -> Result<DesktopShortcutTiming, DesktopProfileError> {
    if node.entries().iter().any(|entry| {
        entry
            .name()
            .is_none_or(|name| !matches!(name.value(), "tap-ms" | "sequence-ms"))
    }) || node.entries().is_empty()
        || node.children().is_some()
        || node.ty().is_some()
    {
        return Err(schema_error(
            "shortcut-timing takes only tap-ms and sequence-ms",
        ));
    }
    let defaults = DesktopShortcutTiming::default();
    Ok(DesktopShortcutTiming {
        tap_ms: integer_property(node, "tap-ms", DESKTOP_SHORTCUT_TAP_MS)?
            .unwrap_or(defaults.tap_ms),
        sequence_ms: integer_property(node, "sequence-ms", DESKTOP_SHORTCUT_SEQUENCE_MS)?
            .unwrap_or(defaults.sequence_ms),
    })
}

/// The cross-binding rules: every physical shape is bound once, no sequence
/// extends another binding, leaders sit on sequence prefixes without nesting
/// and own their actions, and every help row fits its catalog width.
fn validate_shapes(candidate: &DesktopShortcutCandidate) -> Result<(), DesktopProfileError> {
    let mut shapes = BTreeSet::new();
    let mut key_paths = BTreeSet::new();
    for binding in &candidate.bindings {
        let path = binding.path().cloned().collect::<Vec<_>>();
        if !shapes.insert((path.clone(), binding.hold_ms.is_some())) {
            return Err(schema_error("duplicate physical chord"));
        }
        if binding.chord.kind == DesktopShortcutBindingKind::Key {
            key_paths.insert(path);
        }
        if candidate.binding_display(binding).len() > DESKTOP_SHORTCUT_MAX_DISPLAY_BYTES {
            return Err(schema_error("binding is too long to show in help"));
        }
    }
    for path in &key_paths {
        if (1..path.len()).any(|length| key_paths.contains(&path[..length])) {
            return Err(schema_error("a sequence cannot extend another binding"));
        }
    }
    let mut leaders = BTreeMap::new();
    for leader in &candidate.leaders {
        let prefix = leader.path().cloned().collect::<Vec<_>>();
        if !key_paths
            .iter()
            .any(|path| path.len() > prefix.len() && path.starts_with(&prefix))
        {
            return Err(schema_error("a leader must prefix a sequence"));
        }
        if leaders.insert(prefix, &leader.action).is_some() {
            return Err(schema_error("duplicate leader"));
        }
        if leader.display().len() > DESKTOP_SHORTCUT_MAX_DISPLAY_BYTES {
            return Err(schema_error("leader is too long to show in help"));
        }
    }
    for prefix in leaders.keys() {
        if leaders
            .keys()
            .any(|other| other.len() < prefix.len() && prefix.starts_with(other))
        {
            return Err(schema_error("leaders cannot nest on one sequence"));
        }
    }
    let mut leader_actions = BTreeSet::new();
    for action in leaders.values() {
        let bound = candidate.bindings.iter().any(|binding| {
            matches!(&binding.target, DesktopShortcutTarget::PolicyAction(name) if name == *action)
        });
        if bound || !leader_actions.insert(*action) {
            return Err(schema_error(
                "a leader's action cannot be bound to anything else",
            ));
        }
    }
    Ok(())
}

pub fn prepare_desktop_shortcut_candidate(
    candidate: &DesktopAuthorityCandidate,
) -> Result<DesktopShortcutCandidate, DesktopProfileError> {
    if candidate.authority != DesktopAuthority::Shortcut {
        return Err(schema_error("candidate crossed its authority boundary"));
    }
    let mut prepared = DesktopShortcutCandidate {
        generation: candidate.generation,
        digest: candidate.digest,
        profile: String::new(),
        bindings: Vec::new(),
        leaders: Vec::new(),
        timing: DesktopShortcutTiming::default(),
    };
    let mut timed = false;
    for value in &candidate.values {
        let node = single_node(&value.encoded)?;
        match node.name().value() {
            "profile" => {
                if !prepared.profile.is_empty() {
                    return Err(schema_error("duplicate profile identity"));
                }
                prepared.profile = exact_profile(&node)?;
            }
            "bind" | "pointer-bind" => {
                if prepared.bindings.len() + prepared.leaders.len() >= DESKTOP_SHORTCUT_MAX_BINDINGS
                {
                    return Err(schema_error("binding count exceeds 256"));
                }
                prepared.bindings.push(parse_binding(&node)?);
            }
            "leader" => {
                if prepared.bindings.len() + prepared.leaders.len() >= DESKTOP_SHORTCUT_MAX_BINDINGS
                {
                    return Err(schema_error("binding count exceeds 256"));
                }
                prepared.leaders.push(parse_leader(&node)?);
            }
            "shortcut-timing" => {
                if timed {
                    return Err(schema_error("duplicate shortcut-timing"));
                }
                timed = true;
                prepared.timing = parse_timing(&node)?;
            }
            _ => return Err(schema_error("candidate contains a non-shortcut setting")),
        }
    }
    if prepared.profile.is_empty() && !(prepared.bindings.is_empty() && prepared.leaders.is_empty())
    {
        return Err(schema_error("profile identity is required"));
    }
    validate_shapes(&prepared)?;
    Ok(prepared)
}

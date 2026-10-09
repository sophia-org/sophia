//! The controlled-repaint guest profile (t307) as the guest writes it: one
//! logical output mirrored onto both virtio heads unscaled, and F9 as the one
//! shortcut, bound to the generic test WM's hold-shift action. The profile is
//! read from tools/qemu_guest_init.sh itself, so the guest cannot drift from
//! what this checks.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use sophia_config::{
    ConfigGeneration, DesktopAuthority, DesktopMirrorFit, DesktopShortcutModifiers,
    DesktopShortcutTarget, desktop_shortcut_evdev_keycode, load_desktop_profile,
    prepare_desktop_output_candidate, prepare_desktop_shortcut_candidate,
};

const DELIMITER: &str = "CONTROLLED_REPAINT_PROFILE";

fn guest_profile() -> String {
    let init = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/qemu_guest_init.sh"),
    )
    .unwrap();
    let opening = format!("<<'{DELIMITER}'");
    let lines = init.lines().collect::<Vec<_>>();
    let open = lines
        .iter()
        .position(|line| line.ends_with(&opening))
        .expect("the guest writes the profile");
    assert_eq!(
        init.matches(&opening).count(),
        1,
        "the guest writes the profile once"
    );
    let close = lines[open + 1..]
        .iter()
        .position(|line| *line == DELIMITER)
        .expect("the profile is closed by its delimiter");
    lines[open + 1..open + 1 + close].join("\n") + "\n"
}

#[test]
fn guest_profile_mirrors_both_heads_and_binds_f9_to_hold_shift() {
    let directory = std::env::temp_dir().join(format!(
        "sophia-controlled-repaint-profile-{}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let path = directory.join("desktop.kdl");
    fs::write(&path, guest_profile()).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let loaded = load_desktop_profile(Some(&path), ConfigGeneration::INITIAL);
    fs::remove_dir_all(&directory).unwrap();
    let profile = loaded.unwrap();

    let output =
        prepare_desktop_output_candidate(&profile.candidates[&DesktopAuthority::Output]).unwrap();
    assert_eq!(output.named.len(), 1);
    let group = &output.named[0];
    assert_eq!(group.connector, "Virtual-1");
    assert_eq!(group.mirror, ["Virtual-2"]);
    assert_eq!(group.mirror_fit, Some(DesktopMirrorFit::Exact));

    let shortcut =
        prepare_desktop_shortcut_candidate(&profile.candidates[&DesktopAuthority::Shortcut])
            .unwrap();
    assert_eq!(shortcut.profile, "controlled-repaint");
    assert!(shortcut.leaders.is_empty());
    assert_eq!(shortcut.bindings.len(), 1);
    let binding = &shortcut.bindings[0];
    assert_eq!(binding.chord.modifiers, DesktopShortcutModifiers::NONE);
    assert_eq!(
        desktop_shortcut_evdev_keycode(&binding.chord.trigger),
        Some(67)
    );
    assert!(binding.steps.is_empty() && binding.hold_ms.is_none());
    assert_eq!(
        binding.target,
        DesktopShortcutTarget::PolicyAction("hold-shift".to_owned())
    );
}

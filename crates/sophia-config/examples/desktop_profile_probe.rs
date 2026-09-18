//! Offline probe composition. WM policy and shortcuts always come from the
//! selected profile; the probe may override other settings by their typed key.
//! Output is ordinary KDL, reloaded by the normal Session preparation path.

use sophia_config::{
    ConfigGeneration, DesktopAuthority, DesktopProfileError, load_prepared_desktop_profile,
    render_desktop_profile_source,
};
use std::path::Path;

pub fn compose(base: &Path, probe: &Path) -> Result<String, DesktopProfileError> {
    let mut base = load_prepared_desktop_profile(Some(base), ConfigGeneration::INITIAL)?.profile;
    let probe = load_prepared_desktop_profile(Some(probe), ConfigGeneration::INITIAL)?.profile;
    for authority in [DesktopAuthority::Policy, DesktopAuthority::Shortcut] {
        if !probe.candidates[&authority].values.is_empty() {
            return Err(DesktopProfileError::Schema(
                "probe must not replace WM policy or shortcut definitions".into(),
            ));
        }
    }
    for authority in DesktopAuthority::ALL {
        let replacement = &probe.candidates[&authority];
        let original = base
            .candidates
            .get_mut(&authority)
            .expect("complete profile");
        original
            .values
            .retain(|value| !replacement.values.iter().any(|next| next.key == value.key));
        original.values.extend(replacement.values.iter().cloned());
    }
    render_desktop_profile_source(&base)
}

pub fn require_launcher_binding(path: &Path) -> Result<(), DesktopProfileError> {
    let profile = load_prepared_desktop_profile(Some(path), ConfigGeneration::INITIAL)?;
    if !profile.candidates.shortcut.bindings.iter().any(|binding| {
        binding.chord.kind == sophia_config::DesktopShortcutBindingKind::Key
            && binding.target
                == sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::ApplicationLauncher,
                )
    }) {
        return Err(DesktopProfileError::Schema(
            "launcher probe requires an application-launcher key binding in the selected WM profile".into()));
    }
    Ok(())
}

#[cfg(not(test))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 && !(args.len() == 3 && args[2] == "--require-launcher-binding") {
        return Err(
            "usage: desktop_profile_probe WM_PROFILE PROBE_OVERRIDES [--require-launcher-binding]"
                .into(),
        );
    }
    if args.len() == 3 {
        require_launcher_binding(Path::new(&args[0]))?;
    }
    print!("{}", compose(Path::new(&args[0]), Path::new(&args[1]))?);
    Ok(())
}

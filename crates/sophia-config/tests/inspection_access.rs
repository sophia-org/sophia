use sophia_config::{
    ConfigGeneration, DesktopControlAccess, DesktopInspectionAccess, load_prepared_desktop_profile,
};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Profile(PathBuf);

impl Profile {
    fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sophia-inspection-profile-{}-{}.kdl",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed),
        ));
        Self(path)
    }

    fn write(&self, settings: &str) {
        std::fs::write(&self.0, format!("schema 1\nsession {{ {settings} }}\n")).unwrap();
        std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn inspection_defaults_to_disabled_and_is_independent_of_control() {
    let profile = Profile::new();
    for (settings, control, inspection) in [
        (
            "",
            DesktopControlAccess::Disabled,
            DesktopInspectionAccess::Disabled,
        ),
        (
            "inspection \"disabled\";",
            DesktopControlAccess::Disabled,
            DesktopInspectionAccess::Disabled,
        ),
        (
            "control \"host-admin\";",
            DesktopControlAccess::HostAdmin,
            DesktopInspectionAccess::Disabled,
        ),
        (
            "inspection \"host-admin\";",
            DesktopControlAccess::Disabled,
            DesktopInspectionAccess::HostAdmin,
        ),
        (
            "inspection \"disabled\"; control \"host-admin\";",
            DesktopControlAccess::HostAdmin,
            DesktopInspectionAccess::Disabled,
        ),
        (
            "inspection \"host-admin\"; control \"host-admin\";",
            DesktopControlAccess::HostAdmin,
            DesktopInspectionAccess::HostAdmin,
        ),
    ] {
        profile.write(settings);
        let prepared =
            load_prepared_desktop_profile(Some(&profile.0), ConfigGeneration::from_raw(1)).unwrap();
        assert_eq!(prepared.candidates.session.control, control, "{settings}");
        assert_eq!(
            prepared.candidates.session.inspection, inspection,
            "{settings}"
        );
    }
}

#[test]
fn inspection_rejects_invalid_typed_duplicate_and_ambiguous_values() {
    let profile = Profile::new();
    for settings in [
        "inspection;",
        "inspection #true;",
        "inspection #null;",
        "inspection 1;",
        "inspection \"host\";",
        "inspection \"enabled\";",
        "inspection \"Host-Admin\";",
        "inspection \"\";",
        "inspection mode=\"host-admin\";",
        "inspection (mode)\"host-admin\";",
        "(mode)inspection \"host-admin\";",
        "inspection \"disabled\" \"host-admin\";",
        "inspection \"host-admin\" {};",
        "inspection \"disabled\"; inspection \"host-admin\";",
        "inspection \"host-admin\"; inspection \"host-admin\";",
        "inspection \"disabled\"; } session { inspection \"host-admin\";",
    ] {
        profile.write(settings);
        assert!(
            load_prepared_desktop_profile(Some(&profile.0), ConfigGeneration::from_raw(1)).is_err(),
            "unexpectedly accepted {settings}",
        );
    }
}

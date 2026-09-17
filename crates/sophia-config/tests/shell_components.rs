use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use sophia_config::{
    ConfigGeneration, DesktopAuthority, ShellComponentRole, ShellGpuMode,
    load_prepared_desktop_profile, stage_desktop_profile,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Profile(std::path::PathBuf);
impl Profile {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "shell-components-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    fn write(&self, session: &str, enabled: bool) -> std::path::PathBuf {
        let path = self.0.join("desktop.kdl");
        fs::write(
            &path,
            format!("schema 1\nshell {{ enabled #{enabled}; }}\nsession {{ {session} }}\n"),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }
}
impl Drop for Profile {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

const BAR: &str = r#"shell-component "panel" "bar" { executable "/opt/lom"; config "/home/user/lom.kdl"; gpu "direct"; };"#;
const LAUNCHER: &str =
    r#"shell-component "menu" "application-launcher" { executable "/opt/bemenu-sophia"; };"#;

#[test]
fn two_components_keep_independent_configuration_and_default_denied_gpu() {
    let fixture = Profile::new();
    let path = fixture.write(&format!("{BAR} {LAUNCHER}"), true);
    let prepared = load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    let components = &prepared.candidates.session.components;
    assert!(components.shell_client.is_none());
    assert!(components.shell_config.is_none());
    let [bar, launcher] = components.shell_components.as_slice() else {
        panic!("two components");
    };
    assert_eq!(bar.id, "panel");
    assert_eq!(bar.role, ShellComponentRole::Bar);
    assert_eq!(bar.executable, Path::new("/opt/lom"));
    assert_eq!(bar.config.as_deref(), Some(Path::new("/home/user/lom.kdl")));
    assert_eq!(bar.gpu, ShellGpuMode::Direct);
    assert_eq!(launcher.id, "menu");
    assert_eq!(launcher.role, ShellComponentRole::ApplicationLauncher);
    assert_eq!(launcher.executable, Path::new("/opt/bemenu-sophia"));
    assert_eq!(launcher.config, None);
    assert_eq!(launcher.gpu, ShellGpuMode::Denied);

    let staged = fixture.0.join("staged");
    fs::create_dir(&staged).unwrap();
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o700)).unwrap();
    let fragments = stage_desktop_profile(&prepared.profile, &staged).unwrap();
    for authority in DesktopAuthority::ALL {
        let text = fs::read_to_string(fragments.path(authority)).unwrap();
        assert_eq!(
            text.contains("/opt/bemenu-sophia"),
            authority == DesktopAuthority::Session
        );
        assert_eq!(
            text.contains("/home/user/lom.kdl"),
            authority == DesktopAuthority::Session
        );
    }
}

#[test]
fn conflicting_provider_identity_role_and_legacy_selection_refuse_atomically() {
    let fixture = Profile::new();
    for extra in [
        BAR.to_owned(),
        BAR.replace("panel", "other"),
        LAUNCHER.replace("menu", "panel"),
        format!("{LAUNCHER} {}", LAUNCHER.replace("menu", "second")),
        r#"shell-client "/opt/legacy";"#.to_owned(),
        r#"shell-config "/home/user/legacy.kdl";"#.to_owned(),
    ] {
        for source in [format!("{BAR} {extra}"), format!("{extra} {BAR}")] {
            let path = fixture.write(&source, true);
            assert!(
                load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err(),
                "{source}"
            );
        }
    }
    let path = fixture.write(LAUNCHER, false);
    assert!(load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
}

#[test]
fn malformed_component_never_becomes_a_selection() {
    let fixture = Profile::new();
    for source in [
        r#"shell-component "" "bar" { executable "/opt/lom"; };"#,
        r#"shell-component "../menu" "bar" { executable "/opt/lom"; };"#,
        r#"shell-component "menu" "lock-screen" { executable "/opt/lom"; };"#,
        r#"shell-component "menu" role="bar" { executable "/opt/lom"; };"#,
        r#"shell-component (str)"menu" "bar" { executable "/opt/lom"; };"#,
        r#"shell-component "menu" "bar";"#,
        r#"shell-component "menu" "bar" {};"#,
        r#"shell-component "menu" "bar" { executable "relative"; };"#,
        r#"shell-component "menu" "bar" { executable "/opt/lom" "argument"; };"#,
        r#"shell-component "menu" "bar" { executable "/opt/lom"; executable "/bin/true"; };"#,
        r#"shell-component "menu" "bar" { executable "/opt/lom"; config "~/menu"; };"#,
        r#"shell-component "menu" "bar" { executable "/opt/lom"; gpu #true; };"#,
        r#"shell-component "menu" "bar" { executable "/opt/lom"; gpu mode="direct"; };"#,
        r#"shell-component "menu" "bar" { executable "/opt/lom"; gpu "direct"; gpu "denied"; };"#,
        r#"shell-component "menu" "bar" { executable "/opt/lom"; exec "arbitrary"; };"#,
    ] {
        let path = fixture.write(source, true);
        assert!(
            load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err(),
            "{source}"
        );
    }
    let too_long = "a".repeat(65);
    let path = fixture.write(&BAR.replace("panel", &too_long), true);
    assert!(load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_err());
    let path = fixture.write(&BAR.replace("panel", &"a".repeat(64)), true);
    assert!(load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).is_ok());
}

#[test]
fn legacy_selection_is_preserved_without_implicit_component_permissions() {
    let fixture = Profile::new();
    let path = fixture.write(
        r#"shell-client "/opt/legacy"; shell-config "/home/user/legacy.kdl";"#,
        true,
    );
    let prepared = load_prepared_desktop_profile(Some(&path), ConfigGeneration::INITIAL).unwrap();
    let components = &prepared.candidates.session.components;
    assert!(components.shell_components.is_empty());
    assert_eq!(
        components.shell_client.as_deref(),
        Some(Path::new("/opt/legacy"))
    );
    assert_eq!(
        components.shell_config.as_deref(),
        Some(Path::new("/home/user/legacy.kdl"))
    );
}

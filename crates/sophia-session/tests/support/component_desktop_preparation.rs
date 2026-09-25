//! The selected desktop's preparation boundary, without executing any client.
//! Paths and KDL are fixture-owned; no installed process identity is inferred.
use super::super::{PersistentXtermSessionConfig, component_lifecycle};
use crate::shell_component_launch::ShellComponentLaunch;
use sophia_config::{ShellComponentRole, ShellGpuMode};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Desktop(PathBuf);
impl Desktop {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "component-desktop-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(root.join("lom.kdl"), "// independent private asset\n").unwrap();
        let profile = format!(
            r#"schema 1
shell {{ enabled #true; content #true; content-input #true; panel 24; gpu "denied"; }}
shortcut {{ profile "component-desktop"; bind Super+Space session:application-launcher; }}
session {{
    shell-component "panel" "bar" {{ executable "{}/lom"; config "{}/lom.kdl"; gpu "direct"; reservation "top" 24; }}
    shell-component "menu" "application-launcher" {{ executable "{}/bemenu-sophia"; gpu "denied"; }}
    application-catalog "native-launcher-gate"
    startup
}}
"#,
            root.display(),
            root.display(),
            root.display()
        );
        std::fs::write(root.join("desktop.kdl"), profile).unwrap();
        std::fs::set_permissions(
            root.join("desktop.kdl"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        Self(root)
    }

    fn config(&self) -> PersistentXtermSessionConfig {
        let core = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/fixtures/native_launcher_core.kdl");
        PersistentXtermSessionConfig::from_args(&[
            format!("--config={}", core.display()),
            format!("--desktop-profile={}", self.0.join("desktop.kdl").display()),
            "--session-mode=normal".into(),
            "--wm-process=/absent/hagia".into(),
            "--shell-process-default=/absent/narthex".into(),
        ])
        .unwrap()
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn component_desktop_profile_keeps_private_assets_distinct_from_artifact_admission() {
    let fixture = Desktop::new();
    let config = fixture.config();
    assert!(config.shell_process.is_none());
    assert!(config.shell_config.is_none());
    let components = &config
        .session_profile
        .candidate()
        .components
        .shell_components;
    let [panel, menu] = components.as_slice() else {
        panic!("the selected desktop must have exactly two components");
    };
    assert_eq!(
        (panel.id.as_str(), panel.role),
        ("panel", ShellComponentRole::Bar)
    );
    assert_eq!(
        (menu.id.as_str(), menu.role),
        ("menu", ShellComponentRole::ApplicationLauncher)
    );
    assert_eq!(panel.gpu, ShellGpuMode::Direct);
    assert_eq!(menu.gpu, ShellGpuMode::Denied);
    assert_eq!(panel.reservation.unwrap().max_thickness, 24);
    assert_eq!(panel.config.as_ref(), Some(&fixture.0.join("lom.kdl")));
    assert!(menu.config.is_none());
    // Parsing is not an executable or private-KDL verifier. These files have
    // never existed, yet preparation has a valid selection. Do not turn this
    // fixture into evidence of a runnable package.
    assert!(!panel.executable.exists());
    assert!(!menu.executable.exists());
    // Isolate the existing private-path owner from the unavailable GPU grant.
    // This denied copy is a fixture, not the selected desktop's GPU policy.
    let mut denied = panel.clone();
    denied.gpu = ShellGpuMode::Denied;
    let plan = ShellComponentLaunch::new(denied.clone(), Some(24), None).unwrap();
    assert_eq!(plan.selection().config, panel.config);
    std::fs::remove_file(fixture.0.join("lom.kdl")).unwrap();
    assert_eq!(
        fixture
            .config()
            .session_profile
            .candidate()
            .components
            .shell_components,
        *components
    );
    assert!(ShellComponentLaunch::new(denied, Some(24), None).is_err());
    assert!(ShellComponentLaunch::new(menu.clone(), None, None).is_ok());
}

#[test]
fn component_desktop_without_gpu_prepares_only_menu_and_mints_no_launch_evidence() {
    let fixture = Desktop::new();
    let config = fixture.config();
    // Call the real outer preparation owner: a requested direct panel is
    // refused without a coordinator; its denied neighbor may still prepare.
    let (owner, directory) = component_lifecycle::prepare(&config, None).unwrap();
    let owner = owner.expect("the denied launcher remains selected");
    let directory = directory.unwrap();
    assert!(directory.join("menu").exists());
    assert!(!directory.join("panel").exists());
    assert_eq!(owner.retained_processes(), 0);
    assert!(owner.attempt(0).is_none());
    assert!(owner.attempt(1).is_none());
    assert!(owner.work_area_bands().is_empty());
    drop(owner);
    std::fs::remove_dir_all(directory).unwrap();
}

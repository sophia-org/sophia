#![cfg(test)]

#[path = "desktop_launch_reload.rs"]
mod desktop_launch_reload;
#[path = "session_command_registry.rs"]
mod session_command_registry;

use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const CORE: &str = r#"schema 2
session {
    application "brave-origin" id=5 executable="/usr/bin/sophia" {
        arg "client-launch"
        arg "--"
        arg "/usr/bin/brave-origin"
    }
    application "panel" id=4 executable="/usr/bin/true"
    startup 4
}
"#;
const DIRECT_CORE: &str = r#"schema 2
session {
    application "brave-origin" id=5 executable="/usr/bin/brave-origin"
    application "panel" id=4 executable="/usr/bin/true"
    startup 4
}
"#;

struct ConfigFixture {
    directory: PathBuf,
    config: PersistentXtermSessionConfig,
}

impl ConfigFixture {
    fn new(extra_args: &[&str]) -> Self {
        Self::from_documents(
            CORE,
            "schema 1\nshell { enabled #false; }\nsession { terminal \"terminal\"; browser \"brave-origin\"; startup \"panel\"; }\n",
            extra_args,
        )
    }

    fn from_documents(core_bytes: &str, desktop_bytes: &str, extra_args: &[&str]) -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "sophia-core-reload-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&directory).unwrap();
        let core = directory.join("core.kdl");
        let desktop = directory.join("desktop.kdl");
        for (path, bytes) in [(&core, core_bytes), (&desktop, desktop_bytes)] {
            std::fs::write(path, bytes).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut args = vec![
            format!("--config={}", core.display()),
            format!("--desktop-profile={}", desktop.display()),
            "--no-input".to_owned(),
            "--session-mode=normal".to_owned(),
            "--session-app=terminal=/usr/bin/kitty".to_owned(),
            "--session-app-arg=terminal=--single-instance".to_owned(),
            "--session-action-app=terminal=terminal".to_owned(),
        ];
        args.extend(extra_args.iter().map(|arg| (*arg).to_owned()));
        let config = PersistentXtermSessionConfig::from_args(&args).unwrap();
        Self { directory, config }
    }
}

impl Drop for ConfigFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

fn activate_session_profile(config: &mut PersistentXtermSessionConfig) {
    let profile = config.session_profile.candidate();
    let key = sophia_config::DesktopProfileActivationKey::new(profile.generation, profile.digest);
    *config.session_profile.slot_mut() =
        sophia_config::activate_desktop_profile_candidate_slot(config.session_profile.slot(), key)
            .unwrap();
    assert_eq!(
        config.session_profile.slot().participant().phase(),
        sophia_config::DesktopProfileParticipantPhase::Activated,
    );
    assert!(config.session_profile.slot().active().is_some());
}

fn assert_launches(config: &PersistentXtermSessionConfig, browser: &str) {
    let terminal = config
        .application_for_action(WmSessionAction::LaunchApplication {
            application: TERMINAL_APPLICATION_ID,
        })
        .unwrap();
    assert_eq!(terminal.executable, Path::new("/usr/bin/kitty"));
    assert_eq!(terminal.arguments, ["--single-instance"]);
    let browser = config
        .application_for_action(WmSessionAction::LaunchApplication {
            application: BROWSER_APPLICATION_ID,
        })
        .filter(|application| application.id == browser)
        .unwrap();
    assert_eq!(browser.executable, Path::new("/usr/bin/brave-origin"));
    assert!(browser.arguments.is_empty());
}

#[test]
fn direct_browser_reload_preserves_cli_terminal_and_profile_browser() {
    let mut fixture = ConfigFixture::new(&[]);
    let config = &mut fixture.config;
    let generation = config.core_config_state.active().generation;
    let report = config.reload_core_config(DIRECT_CORE.as_bytes()).unwrap();
    assert_eq!(
        report.disposition,
        sophia_config::ReloadDisposition::Applied
    );
    assert!(report.generation > generation);
    assert_launches(config, "brave-origin");
    assert_eq!(config.applications.startup, ["panel"]);

    let second = format!("{DIRECT_CORE}diagnostics verbose=#true\n");
    let report = config.reload_core_config(second.as_bytes()).unwrap();
    assert_eq!(
        report.disposition,
        sophia_config::ReloadDisposition::Applied
    );
    assert_launches(config, "brave-origin");
    let applications = config.applications.clone();
    let unchanged = config.reload_core_config(second.as_bytes()).unwrap();
    assert_eq!(
        unchanged.disposition,
        sophia_config::ReloadDisposition::Deferred
    );
    assert_eq!(unchanged.generation, report.generation);
    assert_eq!(config.applications, applications);
}

#[test]
fn core_reload_uses_activated_profile_even_while_a_replacement_is_prepared() {
    let mut fixture = ConfigFixture::new(&[]);
    let config = &mut fixture.config;
    activate_session_profile(config);
    config.reload_core_config(DIRECT_CORE.as_bytes()).unwrap();
    assert_launches(config, "brave-origin");

    let mut staged = config.session_profile.slot().active().unwrap().clone();
    staged.generation = sophia_config::ConfigGeneration::from_raw(staged.generation.raw() + 1);
    staged.digest = sophia_config::ConfigDigest::new([0x69; 32]);
    staged.browser = Some("panel".to_owned());
    staged.startup = Some(vec!["missing-staged-application".to_owned()]);
    *config.session_profile.slot_mut() = sophia_config::prepare_desktop_profile_candidate_slot(
        config.session_profile.slot(),
        staged.clone(),
    )
    .unwrap();
    let profile_slot = config.session_profile.slot().clone();
    let changed = format!("{DIRECT_CORE}diagnostics verbose=#true\n");
    config.reload_core_config(changed.as_bytes()).unwrap();
    assert_launches(config, "brave-origin");
    assert_eq!(config.applications.startup, ["panel"]);
    assert_eq!(config.session_profile.slot(), &profile_slot);
    assert_eq!(config.session_profile.slot().candidate(), Some(&staged));
}

#[test]
fn core_reload_preserves_explicit_cli_action_and_startup_precedence() {
    let mut fixture = ConfigFixture::new(&[
        "--session-app=cli-browser=/usr/bin/brave-origin",
        "--session-action-app=browser=cli-browser",
        "--session-start=terminal",
    ]);
    let config = &mut fixture.config;
    activate_session_profile(config);
    config.reload_core_config(DIRECT_CORE.as_bytes()).unwrap();
    assert_launches(config, "cli-browser");
    assert_eq!(config.applications.startup, ["terminal"]);
}

#[test]
fn failed_application_preparation_keeps_active_and_pending_restart_state() {
    let mut fixture = ConfigFixture::new(&[]);
    let config = &mut fixture.config;
    activate_session_profile(config);
    let mut live_profile = config.session_profile.slot().active().unwrap().clone();
    live_profile
        .applications
        .push(sophia_config::DesktopApplication {
            name: "retained-panel".to_owned(),
            command: sophia_config::DesktopApplicationCommand::UseCore("panel".to_owned()),
            provenance: sophia_config::DesktopValueProvenance {
                path: fixture.directory.join("desktop.kdl"),
                ordinal: 0,
            },
        });
    config.applications = config
        .session_application_overrides
        .prepare_live_reload(
            PersistentXtermSessionConfig::applications_from_core(config.core_config_state.active())
                .unwrap(),
            &live_profile,
            &config.applications.startup,
        )
        .unwrap();
    config.active_launch_profile = Some(live_profile);
    let active = config.core_config_state.active().clone();
    let applications = config.applications.clone();
    let restart = format!("{DIRECT_CORE}input {{ seat \"seat-for-restart\"; }}\n");
    let report = config.reload_core_config(restart.as_bytes()).unwrap();
    assert_eq!(
        report.disposition,
        sophia_config::ReloadDisposition::PendingRestart
    );
    assert_eq!(config.core_config_state.active(), &active);
    assert_eq!(config.applications, applications);
    let pending = config.core_config_state.pending_restart().unwrap().clone();

    let invalid_reference = DIRECT_CORE.replace("    startup 4", "    startup 99");
    let missing = DIRECT_CORE
        .replace(
            "    application \"panel\" id=4 executable=\"/usr/bin/true\"\n",
            "",
        )
        .replace("    startup 4\n", "");
    for rejected in [
        invalid_reference.clone(),
        format!("{invalid_reference}input {{ seat \"another-restart\"; }}\n"),
        missing,
        "not a valid core configuration".to_owned(),
    ] {
        assert!(config.reload_core_config(rejected.as_bytes()).is_err());
        assert_eq!(config.core_config_state.active(), &active);
        assert_eq!(config.applications, applications);
        assert_eq!(config.core_config_state.pending_restart(), Some(&pending));
    }

    config.reload_core_config(DIRECT_CORE.as_bytes()).unwrap();
    assert_launches(config, "brave-origin");
    assert!(config.core_config_state.pending_restart().is_none());
}

#[test]
fn unchanged_core_reload_clears_only_the_pending_restart_candidate() {
    let mut fixture = ConfigFixture::new(&[]);
    let config = &mut fixture.config;
    let active = config.core_config_state.active().clone();
    let applications = config.applications.clone();
    let restart = format!("{CORE}input {{ seat \"seat-for-restart\"; }}\n");
    config.reload_core_config(restart.as_bytes()).unwrap();
    assert!(config.core_config_state.pending_restart().is_some());
    let report = config.reload_core_config(CORE.as_bytes()).unwrap();
    assert_eq!(
        report.disposition,
        sophia_config::ReloadDisposition::Deferred
    );
    assert_eq!(config.core_config_state.active(), &active);
    assert_eq!(config.applications, applications);
    assert!(config.core_config_state.pending_restart().is_none());
}

#[test]
fn core_reload_without_active_or_prepared_session_authority_is_rejected() {
    let mut fixture = ConfigFixture::new(&[]);
    let config = &mut fixture.config;
    let active = config.core_config_state.active().clone();
    let applications = config.applications.clone();
    let restart = format!("{CORE}input {{ seat \"seat-for-restart\"; }}\n");
    config.reload_core_config(restart.as_bytes()).unwrap();
    let pending = config.core_config_state.pending_restart().cloned();
    let profile = config.session_profile.candidate();
    let key = sophia_config::DesktopProfileActivationKey::new(profile.generation, profile.digest);
    *config.session_profile.slot_mut() =
        sophia_config::rollback_desktop_profile_candidate_slot(config.session_profile.slot(), key)
            .unwrap();
    assert_eq!(
        config.session_profile.slot().participant().phase(),
        sophia_config::DesktopProfileParticipantPhase::Idle,
    );
    assert!(config.session_profile.slot().active().is_none());
    assert!(config.session_profile.slot().candidate().is_none());
    assert!(config.reload_core_config(DIRECT_CORE.as_bytes()).is_err());
    assert_eq!(config.core_config_state.active(), &active);
    assert_eq!(config.applications, applications);
    assert_eq!(config.core_config_state.pending_restart(), pending.as_ref());
}

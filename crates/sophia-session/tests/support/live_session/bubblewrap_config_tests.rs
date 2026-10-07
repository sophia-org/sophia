//! The session's trusted Bubblewrap choice (t301): parsed once from the
//! session's own arguments and carried into every protection domain it
//! starts. A stand-in Bubblewrap that reports an old version proves which
//! executable a launch actually ran, without starting any role.
use super::session_config_tests::isolated_session_config;
use super::*;
use std::path::{Path, PathBuf};

struct StandIn {
    directory: PathBuf,
}

impl StandIn {
    fn new(label: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-bubblewrap-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let script = directory.join("bwrap");
        std::fs::write(&script, "#!/bin/sh\necho 'bubblewrap 0.0.1'\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { directory }
    }

    fn path(&self) -> PathBuf {
        self.directory.join("bwrap")
    }

    /// The refusal only the stand-in can cause.
    fn ran(&self, error: &dyn std::fmt::Display) -> bool {
        error.to_string().contains("bubblewrap 0.0.1")
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn the_session_bubblewrap_is_the_default_or_an_explicit_executable() {
    let config = isolated_session_config(&[]).unwrap();
    assert_eq!(
        config.bubblewrap,
        Path::new(sophia_runtime::DEFAULT_BUBBLEWRAP_PATH)
    );
    let stand_in = StandIn::new("parse");
    let plain = stand_in.directory.join("plain");
    std::fs::write(&plain, b"").unwrap();
    std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
    for (path, refusal) in [
        (PathBuf::from("bwrap"), "requires an absolute path"),
        (stand_in.directory.join("missing"), "cannot inspect"),
        (stand_in.directory.clone(), "is not an executable file"),
        (plain, "is not an executable file"),
    ] {
        let error = isolated_session_config(&[format!("--bubblewrap={}", path.display())])
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains(refusal), "{}: {error}", path.display());
    }
    let config =
        isolated_session_config(&[format!("--bubblewrap={}", stand_in.path().display())]).unwrap();
    assert_eq!(config.bubblewrap, stand_in.path());
}

#[test]
fn the_window_manager_domain_names_the_session_bubblewrap() {
    let stand_in = StandIn::new("wm");
    let config = isolated_session_config(&[
        "--wm-process=/usr/bin/true".to_owned(),
        "--wm-interface=sophia_wm_v1".to_owned(),
        format!("--bubblewrap={}", stand_in.path().display()),
    ])
    .unwrap();
    let spec = public_policy_launch_spec(
        &config,
        "/usr/bin/true",
        Path::new("/run/user/1000/sophia/policy/endpoint/wm.sock"),
        Path::new("/run/user/1000/sophia/policy/checkpoint/policy.checkpoint"),
        Path::new("/run/user/1000/sophia/policy/policy.profile.kdl"),
        false,
    )
    .unwrap();
    assert_eq!(
        spec.protection_domain.unwrap().bubblewrap_executable(),
        stand_in.path()
    );
}

#[test]
fn the_metadata_broker_and_lock_provider_launch_the_session_bubblewrap() {
    let stand_in = StandIn::new("roles");
    let error = LiveMetadataBroker::start(&stand_in.path()).err().unwrap();
    assert!(stand_in.ran(&error), "{error}");

    let wake = sophia_wake::Wake::new().unwrap();
    let directory = stand_in.directory.join("lock");
    // The service starts at once; the process is launched later, by a poll.
    let mut provider = lock_provider::LockProvider::start(
        &sophia_config::LockProviderConfig {
            executable: PathBuf::from("/usr/bin/true"),
            config: None,
            gpu: sophia_config::ShellGpuMode::Denied,
        },
        &directory,
        &stand_in.path(),
        None,
        crate::session_lock_object::session_lock_object(
            crate::session_lock::SessionLockPhase::Unlocked,
            None,
        ),
        crate::session_lock_object::session_lock_file_limits(None),
        Vec::new(),
        wake.notifier(),
    )
    .unwrap();
    let error = provider.launch(true).err().unwrap();
    assert!(stand_in.ran(&error), "{error}");
}

/// Every production protection domain in this crate names the session's
/// Bubblewrap. A new domain that forgets it would silently run the default.
/// The GPU content proof is a standalone proof command with no session, so it
/// keeps the default and is listed by name.
#[test]
fn every_session_protection_domain_names_the_session_bubblewrap() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut pending = vec![source.clone()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    let mut constructions = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for (offset, _) in text.match_indices("ProtectionDomainSpec::bubblewrap(") {
            let statement = &text[offset..];
            let statement = &statement[..statement.find(';').unwrap()];
            let relative = file.strip_prefix(&source).unwrap().display().to_string();
            constructions.push((relative, statement.contains(".bubblewrap_path(")));
        }
    }
    constructions.sort();
    assert_eq!(
        constructions,
        [
            ("live_session/lock_provider.rs".to_owned(), true),
            ("live_session/metadata_broker.rs".to_owned(), true),
            (
                "live_session/metadata_shell/component_launch.rs".to_owned(),
                true
            ),
            (
                "live_session/metadata_shell/gpu_content_proof.rs".to_owned(),
                false
            ),
            ("live_session/wm/public_policy.rs".to_owned(), true),
            (
                "live_session/wm/public_policy/output_service.rs".to_owned(),
                true
            ),
        ]
    );
    // And each owner is handed the session's choice, not the default: the
    // metadata broker and the legacy shell (run.rs), the shell components, the
    // lock provider, the WM and the output authority.
    for (file, forwards) in [
        ("live_session/run.rs", 2),
        ("live_session/component_lifecycle.rs", 1),
        ("live_session/lock_provider.rs", 1),
        ("live_session/wm/public_policy.rs", 1),
        ("live_session/wm/public_policy/output_service.rs", 1),
    ] {
        let text = std::fs::read_to_string(source.join(file)).unwrap();
        assert_eq!(
            text.matches("&config.bubblewrap").count(),
            forwards,
            "{file}"
        );
    }
}

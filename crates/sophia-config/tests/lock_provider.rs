//! t294: the operator selects at most one lock provider in the session
//! section. Its paths are absolute, its GPU access is denied unless chosen,
//! and anything else is refused with the whole profile.
use sophia_config::*;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn session(body: &str, label: &str) -> Result<DesktopSessionCandidate, String> {
    let root = std::env::temp_dir().join(format!("lock-provider-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join("desktop.kdl");
    std::fs::write(&path, format!("schema 1\nsession {{\n{body}\n}}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let result = load_prepared_desktop_profile(Some(&path), ConfigGeneration::from_raw(1))
        .map_err(|error| format!("{error:?}"))
        .and_then(|loaded| {
            prepare_desktop_session_candidate(
                &loaded.profile.candidates[&DesktopAuthority::Session],
            )
            .map_err(|error| format!("{error:?}"))
        });
    std::fs::remove_dir_all(root).unwrap();
    result
}

#[test]
fn a_lock_provider_is_selected_with_gpu_denied_by_default() {
    let candidate = session(
        r#"lock-provider {
    executable "/usr/libexec/kleis"
    config "/home/user/.config/kleis/config.kdl"
}"#,
        "default",
    )
    .unwrap();
    assert_eq!(
        candidate.components.lock_provider,
        Some(LockProviderConfig {
            executable: PathBuf::from("/usr/libexec/kleis"),
            config: Some(PathBuf::from("/home/user/.config/kleis/config.kdl")),
            gpu: ShellGpuMode::Denied,
        })
    );
    let direct = session(
        "lock-provider {\n executable \"/opt/locker\"\n gpu \"direct\"\n}",
        "direct",
    )
    .unwrap();
    let provider = direct.components.lock_provider.unwrap();
    assert_eq!(provider.gpu, ShellGpuMode::Direct);
    assert_eq!(provider.config, None);
    assert!(
        session("", "none")
            .unwrap()
            .components
            .lock_provider
            .is_none()
    );
}

#[test]
fn a_malformed_lock_provider_refuses_the_profile() {
    for (label, body) in [
        ("relative", "lock-provider {\n executable \"kleis\"\n}"),
        ("missing", "lock-provider {\n config \"/c.kdl\"\n}"),
        ("argument", "lock-provider \"/opt/kleis\""),
        (
            "gpu",
            "lock-provider {\n executable \"/opt/k\"\n gpu \"shared\"\n}",
        ),
        (
            "repeat",
            "lock-provider {\n executable \"/a\"\n executable \"/b\"\n}",
        ),
        (
            "unknown",
            "lock-provider {\n executable \"/a\"\n secret \"x\"\n}",
        ),
        (
            "relative-config",
            "lock-provider {\n executable \"/a\"\n config \"c.kdl\"\n}",
        ),
    ] {
        assert!(session(body, label).is_err(), "{label}");
    }
}

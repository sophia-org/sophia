//! The session-lock-provider QEMU guest's argv (tools/qemu_guest_init.sh),
//! parsed by Session as the guest would: the GTK input proof, the injected
//! lock with its authenticator, a desktop profile naming a lock provider, and
//! the generic test WM (tools/qemu_generic_wm.c) that publishes the output
//! snapshot a provider needs. Every flag below must appear in the guest
//! script; that script writes /etc, so its construction is not run here.
use super::session_config_tests::isolated_session_config;
use super::*;

#[test]
fn the_lock_provider_guest_argv_selects_wm_provider_and_injected_lock() {
    // The native-scanout opt-in is set in a child, as the guest sets it:
    // no process-global environment mutation and no device opens.
    if std::env::var_os("SOPHIA_LOCK_PROVIDER_ARGV_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "live_session::tests::qemu_lock_provider_argv_tests::the_lock_provider_guest_argv_selects_wm_provider_and_injected_lock",
                "--nocapture",
            ])
            .env("SOPHIA_LOCK_PROVIDER_ARGV_CHILD", "1")
            .env("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE", "1")
            .output()
            .unwrap();
        assert!(child.status.success(), "{child:?}");
        return;
    }
    let init = include_str!("../../../../../tools/qemu_guest_init.sh");
    for flag in [
        "--inject-session-lock",
        "--factotum-agent=/usr/bin/sophia-factotum",
        "--desktop-profile=/run/sophia-qemu-lock/desktop.kdl",
        "--wm-process=/usr/bin/sophia-qemu-generic-wm",
        "--wm-interface=sophia_wm_v1 --wm-transport=9p2000.L",
        "executable \"/usr/bin/sophia-qemu-lock-provider\"",
    ] {
        assert!(init.contains(flag), "guest script lacks {flag}");
    }
    // The resize proof bypasses a public WM, so only this scenario drops it.
    assert!(init.contains(
        "if [ \"$scenario\" = \"session-lock-provider\" ]; then\n        resize_proof=\"\"\n    fi"
    ));
    assert!(init.contains("${resize_proof:+\"$resize_proof\"} --exit-after-input-proof"));
    let dir = std::env::temp_dir().join(format!(
        "sophia-qemu-lock-provider-argv-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let profile = dir.join("desktop.kdl");
    std::fs::write(
        &profile,
        "schema 1\nsession {\n    lock-provider {\n        executable \"/usr/bin/sophia-qemu-lock-provider\"\n        config \"/run/sophia-qemu-lock/mode\"\n    }\n}\n",
    )
    .unwrap();
    std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o600)).unwrap();
    let args = [
        "--display=:181",
        "--native-scanout",
        "--max-runtime-ms=90000",
        "--namespace-profile=classic",
        "--software-client-rendering",
        "--client=zenity",
        "--client-arg=--entry",
        "--client-arg=--title",
        "--client-arg=Sophia GTK proof",
        "--client-arg=--text",
        "--client-arg=Type sophia, then click OK",
        "--expect-client-stdout=sophia\n",
        "--require-client-normal-exit",
        "--expect-physical-text=sophia",
        "--expect-physical-pointer",
        "--exit-after-input-proof",
        "--factotum-agent=/usr/bin/sophia-factotum",
        "--factotum-pam-helper=/usr/bin/sophia-factotum-pam",
        "--inject-session-lock",
        "--physical-sequence-timeout-ms=60000",
        &format!("--desktop-profile={}", profile.display()),
        "--wm-process=/usr/bin/true",
        "--wm-interface=sophia_wm_v1",
        "--wm-transport=9p2000.L",
    ]
    .map(str::to_owned);
    let config = isolated_session_config(&args);
    std::fs::remove_dir_all(&dir).unwrap();
    let config = config.unwrap();
    assert_eq!(config.wm_process.as_deref(), Some("/usr/bin/true"));
    assert!(config.inject_session_lock);
    assert!(!config.surface_resize_requested());
    assert!(config.factotum.is_some());
    assert_eq!(
        config.lock_provider.as_ref().map(|p| p.executable.clone()),
        Some(std::path::PathBuf::from(
            "/usr/bin/sophia-qemu-lock-provider"
        ))
    );
}

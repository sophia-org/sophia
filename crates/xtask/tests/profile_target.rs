use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn profile_validation_uses_only_the_selected_private_target() {
    let root = std::env::temp_dir().join(format!("sophia-profile-target-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let debug = root.join("debug");
    std::fs::create_dir(&debug).unwrap();
    let binary = debug.join("sophia");
    std::fs::write(
        &binary,
        "#!/bin/sh\n[ \"$1 $2 $3\" = 'session run --validate-session-args' ] || exit 3\nprintf 'sophia_live_session_args schema=1 status=accepted\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let check = || {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .args(["profile", "args", "--profile=standalone"])
            .env_remove("SOPHIA_BIN")
            .env("CARGO_TARGET_DIR", &root)
            .output()
            .unwrap()
    };
    let accepted = check();
    assert!(accepted.status.success(), "{accepted:?}");
    std::fs::remove_file(binary).unwrap();
    let missing = check();
    assert!(
        !missing.status.success(),
        "must not fall back to a different build"
    );
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no sophia binary found"));
    std::fs::remove_dir_all(root).unwrap();
}

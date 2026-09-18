//! Execute the actual shell launcher with supplied build/VT/session effects.
//! The real xtask profile/verifier binary runs. No device or live endpoint exists.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/bash\nset -euo pipefail\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

struct Fixture {
    directory: PathBuf,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("dock-launcher-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let root = directory.join("sophia-stack");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in [
            "tools/fixtures",
            "tools/probes/lom_workload",
            "target/release/examples",
        ] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        for name in [
            "tools/run_current_lom_panel_gate_tty4.sh",
            "tools/fixtures/native_launcher_core.kdl",
            "tools/fixtures/lom_panel_desktop.kdl",
            "tools/fixtures/lom_workload_budgets.json",
            "tools/probes/lom_workload/verify.py",
            "tools/probes/lom_workload/records.py",
            "tools/probes/lom_workload/memory.py",
        ] {
            fs::copy(repo.join(name), root.join(name)).unwrap();
        }
        fs::copy(
            env!("CARGO_BIN_EXE_xtask"),
            root.join("target/release/xtask"),
        )
        .unwrap();
        for name in ["lom", "provlita", "bemenu", "hagia", "bin"] {
            fs::create_dir(directory.join(name)).unwrap();
        }
        fs::write(directory.join("lom/config.kdl"), "fixture configuration").unwrap();
        fs::write(
            directory.join("provlita/config.kdl"),
            "fixture configuration",
        )
        .unwrap();
        fs::write(
            directory.join("wm.kdl"),
            "schema 1\nshortcut { bind \"Super+Space\" \"session:application-launcher\"; }\n",
        )
        .unwrap();
        script(&directory.join("bin/tty"), "echo /dev/tty4");
        script(
            &directory.join("bin/git"),
            r#"
case "$*" in
    *'status --short'*) ;;
    *'rev-parse HEAD'*) printf '%040d\n' 1 ;;
    *'verify-commit '*) ;;
    *'archive '*) tar -cf - --files-from /dev/null ;;
    *) exit 99 ;;
esac"#,
        );
        script(&directory.join("bin/cargo"), "echo build >> \"$TRACE\"");
        script(
            &directory.join("bin/nim"),
            r#"for arg in "$@"; do case "$arg" in -o:*) echo fixture > "${arg#-o:}" ;; esac; done"#,
        );
        // make receives -C DIRECTORY TARGET ...
        script(
            &directory.join("bin/make"),
            r#"echo fixture > "$2/bemenu-sophia""#,
        );
        script(
            &root.join("target/release/sophia"),
            r#"case "$2" in print-effective) cat "$SOPHIA_DESKTOP_PROFILE" ;; check) test -f "${3#--desktop-profile=}" ;; *) exit 99 ;; esac"#,
        );
        script(
            &root.join("target/release/examples/desktop_profile_probe"),
            r#"
[[ "$#" == 3 && "$3" == --require-launcher-binding ]]
cat "$1"
tail -n +2 "$2""#,
        );
        for app in ["lom", "provlita"] {
            let path = directory.join(format!("{app}-target/release/{app}"));
            script(&path, "exit 99");
        }
        script(
            &root.join("tools/lom_gpu_content_hardware_proof.sh"),
            r#"echo proof >> "$TRACE"; exit "${PROOF_STATUS:-0}""#,
        );
        script(
            &root.join("tools/run_sophia_session.sh"),
            r#"
echo session >> "$TRACE"
[[ "$#" == 2 && "$1" == --max-runtime-ms=90000 && "$2" == --wm-process=* ]]
[[ "$SOPHIA_SESSION_STARTUP" == none && "$SOPHIA_REQUIRE_LOCAL_VT" == true ]]
[[ "$SOPHIA_MANAGE_KEYD" == true && "$SOPHIA_SESSION_WATCHDOG_SECONDS" == 110 ]]
mkdir -p "$SOPHIA_DIAGNOSTIC_DIR"
printf '%s\n' 'sophia_tty_recovery schema=3 termios_restored=true done=true' 'sophia_tty_recovery_verification schema=1 keyd_restored=true' > "$SOPHIA_DIAGNOSTIC_DIR/recovery.log"
# Empty transcript must fail the real verifier after a simulated clean exit.
touch "$SOPHIA_DIAGNOSTIC_DIR/events.0.log"
exit "${SESSION_STATUS:-0}""#,
        );
        Self { directory, root }
    }
    fn run(&self, evidence: &str, proof: &str, session: &str) -> std::process::Output {
        Command::new("timeout")
            .args(["30", "bash"])
            .arg(self.root.join("tools/run_current_lom_panel_gate_tty4.sh"))
            .arg("dock")
            .env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.directory.join("bin").display()),
            )
            .env("HOME", &self.directory)
            .env("SOPHIA_LOM_NATIVE_GATE_ARM", "1")
            .env("SOPHIA_LOM_SOURCE", self.directory.join("lom"))
            .env("SOPHIA_LOM_TARGET_DIR", self.directory.join("lom-target"))
            .env("SOPHIA_LOM_CONFIG", self.directory.join("lom/config.kdl"))
            .env("SOPHIA_BEMENU_SOURCE", self.directory.join("bemenu"))
            .env("SOPHIA_HAGIA_ROOT", self.directory.join("hagia"))
            .env("SOPHIA_PROVLITA_SOURCE", self.directory.join("provlita"))
            .env(
                "SOPHIA_PROVLITA_TARGET_DIR",
                self.directory.join("provlita-target"),
            )
            .env(
                "SOPHIA_PROVLITA_CONFIG",
                self.directory.join("provlita/config.kdl"),
            )
            .env("SOPHIA_DESKTOP_PROFILE", self.directory.join("wm.kdl"))
            .env(
                "SOPHIA_LOM_NATIVE_EVIDENCE_DIR",
                self.directory.join(evidence),
            )
            .env("TRACE", self.directory.join("trace"))
            .env("PROOF_STATUS", proof)
            .env("SESSION_STATUS", session)
            .output()
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn dock_launcher_uses_three_component_profile_and_refuses_failed_or_missing_evidence() {
    let f = Fixture::new();
    let failed_proof = f.run("proof-failure", "1", "0");
    assert!(!failed_proof.status.success());
    let trace = fs::read_to_string(f.directory.join("trace")).unwrap();
    assert!(
        trace.contains("proof"),
        "{}",
        String::from_utf8_lossy(&failed_proof.stderr)
    );
    assert!(!trace.contains("session"));
    fs::remove_file(f.directory.join("trace")).unwrap();
    let failed_session = f.run("watchdog", "0", "124");
    assert!(!failed_session.status.success());
    assert_eq!(
        fs::read_to_string(f.directory.join("watchdog/native-outcome.txt")).unwrap(),
        "native_exit_status=124\n"
    );
    let empty = f.run("empty", "0", "0");
    assert!(!empty.status.success());
    assert!(
        String::from_utf8_lossy(&empty.stderr).contains("missing/repeated lifecycle evidence"),
        "{}",
        String::from_utf8_lossy(&empty.stderr)
    );
    let config = fs::read_to_string(f.directory.join("empty/desktop.kdl")).unwrap();
    assert!(config.contains("shell-component \"dock\" \"dock\""));
    assert!(config.contains("reservation \"bottom\" 64"));
    assert!(config.contains("bind \"Super+Space\""));
    let manifest = fs::read_to_string(f.directory.join("empty/identity.manifest")).unwrap();
    assert!(manifest.contains("provlita_binary_sha256="));
    assert!(manifest.contains("latency_acceptance=NOT_RUN"));
    assert!(
        !f.run("empty", "0", "0").status.success(),
        "must not overwrite evidence"
    );
}

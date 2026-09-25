use std::fs;
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "sophia-session-preflight-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::create_dir(root.join("config")).unwrap();
        let fixture = Self(root);
        fixture.profile("schema 1\npolicy { layout scroller; }\n");
        fixture.executable(
            "hagia client",
            r#"#!/bin/sh
set -eu
test "$1" = config
test "$2" = check
policy=${3#--config=}
test "$(stat -c %a "$policy")" = 600
test "$(stat -c %a "$(dirname "$policy")")" = 700
cp "$policy" "$SOPHIA_TEST_CAPTURE"
printf '%s' "$policy" > "$SOPHIA_TEST_POLICY_PATH"
exit "${SOPHIA_TEST_STATUS:-0}"
"#,
        );
        fixture
    }

    fn profile(&self, text: &str) {
        let path = self.0.join("desktop profile.kdl");
        fs::write(&path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn executable(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, source).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sophia"));
        command
            .args(["config", "check-session-profile"])
            .arg(format!(
                "--desktop-profile={}",
                self.0.join("desktop profile.kdl").display()
            ))
            .arg(format!(
                "--default-wm={}",
                self.0.join("hagia client").display()
            ))
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("TMPDIR", &self.0)
            .env("SOPHIA_TEST_CAPTURE", self.0.join("captured.kdl"))
            .env("SOPHIA_TEST_POLICY_PATH", self.0.join("policy-path"));
        command
    }

    fn assert_cleaned_policy(&self) {
        let path = fs::read_to_string(self.0.join("policy-path")).unwrap();
        assert!(!Path::new(&path).exists());
        assert!(!Path::new(&path).parent().unwrap().exists());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn assert_success(output: Output) {
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn preflight_sends_only_private_policy_and_removes_the_staged_fragment() {
    let fixture = Fixture::new();
    fixture.profile("schema 1\npolicy { layout scroller; }\nsession { application \"private\" { exec \"secret-program\" \"secret-argument\"; }; }\n");
    assert_success(fixture.command().output().unwrap());
    let policy = fs::read_to_string(fixture.0.join("captured.kdl")).unwrap();
    assert!(policy.contains("layout"));
    assert!(!policy.contains("secret"));
    assert!(!policy.contains("session"));
    fixture.assert_cleaned_policy();
}

#[test]
fn rejected_policy_fails_preflight_and_releases_its_private_file() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .env("SOPHIA_TEST_STATUS", "7")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Hagia policy validation failed"));
    fixture.assert_cleaned_policy();
}

#[test]
fn another_window_manager_keeps_its_own_policy_vocabulary() {
    let fixture = Fixture::new();
    let wm = fixture.executable("different wm", "#!/bin/sh\nexit 99\n");
    fixture.profile(&format!(
        "schema 1\nsession {{ window-manager {:?}; }}\npolicy {{ other-wm-value 3; }}\n",
        wm.to_str().unwrap()
    ));
    fs::remove_file(fixture.0.join("hagia client")).unwrap();
    assert_success(fixture.command().output().unwrap());
    assert!(!fixture.0.join("policy-path").exists());
}

#[test]
fn missing_selected_shell_or_window_manager_is_refused_before_hagia() {
    let fixture = Fixture::new();
    for component in ["window-manager", "shell-client"] {
        fixture.profile(&format!(
            "schema 1\nsession {{ {component} {:?}; }}\n",
            fixture.0.join("missing").to_str().unwrap()
        ));
        assert!(!fixture.command().output().unwrap().status.success());
        assert!(!fixture.0.join("policy-path").exists());
    }
}

#[test]
#[ignore = "t101 gap: preflight checks legacy shell artifacts but not component artifacts"]
fn missing_two_component_artifacts_must_not_pass_package_preflight() {
    let mut incorrectly_accepted = vec![];
    for deferred in [false, true] {
        for role in ["lom", "bemenu"] {
            for missing in [false, true] {
                let fixture = Fixture::new();
                for name in ["lom", "bemenu"] {
                    fixture.executable(name, "#!/bin/sh\nexit 99\n");
                }
                let victim = fixture.0.join(role);
                if missing {
                    fs::remove_file(victim).unwrap();
                } else {
                    fs::set_permissions(victim, fs::Permissions::from_mode(0o600)).unwrap();
                }
                // This asset is deliberately opaque; no UI grammar is owned
                // by the session preflight. Only its path exists here.
                fs::write(fixture.0.join("lom.kdl"), "opaque private asset\n").unwrap();
                let wm = if deferred {
                    let path = fixture.executable("different wm", "#!/bin/sh\nexit 99\n");
                    format!("window-manager {:?};", path.to_str().unwrap())
                } else {
                    String::new()
                };
                fixture.profile(&format!(
                    r#"schema 1
shell {{ enabled #true; content #true; content-input #true; panel 24; }}
session {{
    {wm}
    shell-component "panel" "bar" {{ executable "{0}/lom"; config "{0}/lom.kdl"; gpu "direct"; reservation "top" 24; }}
    shell-component "menu" "application-launcher" {{ executable "{0}/bemenu"; gpu "denied"; }}
}}
"#,
                    fixture.0.display()
                ));
                let output = fixture.command().output().unwrap();
                if output.status.success() {
                    incorrectly_accepted.push(format!(
                        "deferred={deferred} role={role} missing={missing}: {}",
                        String::from_utf8_lossy(&output.stdout)
                    ));
                }
            }
        }
    }
    assert!(
        incorrectly_accepted.is_empty(),
        "invalid component artifacts were accepted: {incorrectly_accepted:#?}"
    );
}

#[test]
fn selecting_a_hagia_alias_still_checks_hagias_policy() {
    let fixture = Fixture::new();
    let alias = fixture.0.join("hagia alias");
    std::os::unix::fs::symlink(fixture.0.join("hagia client"), &alias).unwrap();
    fixture.profile(&format!(
        "schema 1\nsession {{ window-manager {:?}; }}\n",
        alias.to_str().unwrap()
    ));
    assert!(
        !fixture
            .command()
            .env("SOPHIA_TEST_STATUS", "7")
            .output()
            .unwrap()
            .status
            .success()
    );
    fixture.assert_cleaned_policy();
}

#[test]
fn invalid_envelope_and_duplicate_options_cannot_reach_the_policy_client() {
    let fixture = Fixture::new();
    fixture.profile("not a desktop profile {");
    assert!(!fixture.command().output().unwrap().status.success());
    fixture.profile("schema 1\n");
    assert!(
        !fixture
            .command()
            .arg("--default-wm=/bin/true")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!fixture.0.join("policy-path").exists());
}

#[test]
fn a_stalled_policy_checker_is_killed_at_the_preflight_deadline() {
    let fixture = Fixture::new();
    fixture.executable(
        "hagia client",
        "#!/bin/sh\nprintf '%s' \"${3#--config=}\" > \"$SOPHIA_TEST_POLICY_PATH\"\nsleep 60\n",
    );
    let started = Instant::now();
    let output = fixture.command().output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeded ten seconds"));
    assert!(started.elapsed() < Duration::from_secs(20));
    fixture.assert_cleaned_policy();
}

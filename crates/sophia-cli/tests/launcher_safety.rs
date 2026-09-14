const SESSION_LAUNCHER: &str = include_str!("../../../tools/run_sophia_session.sh");
const TTY3_LAUNCHER: &str = include_str!("../../../tools/start_sophia_tty3.sh");
const DESKTOP_COMPARISON_GATE: &str = include_str!("../../../tools/desktop_comparison_tty3.sh");
const INSTALLED_SESSION: &str = include_str!("../../../tools/installed/sophia-session");
const INSTALLED_HAGIA: &str = include_str!("../../../tools/installed/sophia-hagia-session");
const INSTALLED_HAGIA_PROMOTION: &str =
    include_str!("../../../tools/installed/sophia-hagia-promotion-session");
const INSTALLED_RECOVERY: &str = include_str!("../../../tools/installed/sophia-recovery-proof");
const INSTALLED_TRUECOLOR: &str = include_str!("../../../tools/installed/sophia-truecolor-proof");
const INSTALLER: &str = include_str!("../../../tools/install_live_session.sh");
const ACTIVATOR: &str = include_str!("../../../tools/activate_live_session_release.sh");
const TTY_MODE_HELPER: &str = include_str!("../../../tools/sophia_tty_mode.py");
const LOM_PANEL_GATE: &str = include_str!("../../../tools/run_current_lom_panel_gate_tty4.sh");

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("sophia-{label}-{}-{nonce}", std::process::id()))
}

fn offset(needle: &str) -> usize {
    SESSION_LAUNCHER
        .find(needle)
        .unwrap_or_else(|| panic!("launcher is missing {needle:?}"))
}

#[test]
fn graphical_takeover_disables_console_rendering_and_input_echo_after_guard_arming() {
    let guard_ready = offset("echo \"Emergency input guard armed.\"");
    let graphics = offset("python3 \"$TTY_MODE_HELPER\" graphics");
    let keyboard_off = offset("python3 \"$TTY_MODE_HELPER\" keyboard-off");
    let raw = offset("stty raw -echo");
    let session = offset("setsid \"${session_launch[@]}\"");

    assert!(guard_ready < graphics);
    assert!(graphics < keyboard_off);
    assert!(keyboard_off < raw);
    assert!(raw < session);
}

#[test]
fn lom_gate_keeps_unsanitized_child_output_private_and_explicit() {
    assert!(SESSION_LAUNCHER.contains(
        "SOPHIA_UNTRUSTED_SESSION_OUTPUT_LOG must name untrusted-session-output.log in the diagnostic directory."
    ));
    assert!(SESSION_LAUNCHER.contains("chmod 600 \"$UNTRUSTED_OUTPUT_LOG\""));
    assert!(
        SESSION_LAUNCHER
            .contains("setsid \"${session_launch[@]}\" >\"$UNTRUSTED_OUTPUT_LOG\" 2>&1 &")
    );
    assert!(LOM_PANEL_GATE.contains(
        "SOPHIA_UNTRUSTED_SESSION_OUTPUT_LOG=\"$EVIDENCE_DIR/session/untrusted-session-output.log\""
    ));
}

#[test]
fn input_guard_arm_timeout_is_bounded_and_defaults_to_thirty_seconds() {
    assert!(SESSION_LAUNCHER.contains(
        "INPUT_GUARD_ARM_TIMEOUT_SECONDS=\"${SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS:-30}\""
    ));
    assert!(SESSION_LAUNCHER.contains("\"$INPUT_GUARD_ARM_TIMEOUT_SECONDS\" -gt 300"));
    assert!(SESSION_LAUNCHER.contains("guard_wait_tick < INPUT_GUARD_ARM_WAIT_TICKS"));
    assert!(SESSION_LAUNCHER.contains("within $INPUT_GUARD_ARM_TIMEOUT_SECONDS seconds"));
}

#[test]
fn graphical_takeover_saves_and_restores_exact_tty_state() {
    let save_termios = offset("tty_state=\"$(stty -g)\"");
    let save_kd = offset("kd_mode=\"$(python3 \"$TTY_MODE_HELPER\" get)\"");
    let save_keyboard = offset("keyboard_mode=\"$(python3 \"$TTY_MODE_HELPER\" get-keyboard)\"");
    let graphics = offset("python3 \"$TTY_MODE_HELPER\" graphics");

    assert!(save_termios < graphics);
    assert!(save_kd < graphics);
    assert!(save_keyboard < graphics);
    assert!(SESSION_LAUNCHER.contains("python3 \"$TTY_MODE_HELPER\" \"$kd_mode\""));
    assert!(SESSION_LAUNCHER.contains("stty \"$tty_state\""));
    assert!(SESSION_LAUNCHER.contains("python3 \"$TTY_MODE_HELPER\" \"keyboard-$keyboard_mode\""));
    assert!(
        SESSION_LAUNCHER
            .contains("restored_keyboard=\"$(python3 \"$TTY_MODE_HELPER\" get-keyboard")
    );
    assert!(SESSION_LAUNCHER.contains("sophia_tty_recovery_verification schema=1"));
    assert!(SESSION_LAUNCHER.contains("keyd did not become ready after restoration"));
}

#[test]
fn detached_graphical_owner_does_not_attempt_direct_vt_activation() {
    assert!(!SESSION_LAUNCHER.contains("SOPHIA_SESSION_TTY_FD"));
    assert!(!TTY_MODE_HELPER.contains("VT_ACTIVATE"));
    assert!(!TTY_MODE_HELPER.contains("activate-vt-"));
}

#[test]
fn verbose_trace_defaults_to_one_shot_without_overriding_an_explicit_mode() {
    assert!(SESSION_LAUNCHER.contains("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE-1"));
    assert!(!SESSION_LAUNCHER.contains("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE=continuous"));
    assert!(SESSION_LAUNCHER.contains("SOPHIA_SESSION_VERBOSE_TRACE:-false"));
}

#[test]
fn visual_proofs_opt_in_to_repeated_final_region_readback() {
    assert!(INSTALLED_TRUECOLOR.contains("SOPHIA_INSTALLED_ATTEMPT_MODE=truecolor"));
    assert!(INSTALLED_TRUECOLOR.contains("SOPHIA_TRUECOLOR_PROOF=true"));
    assert!(INSTALLED_TRUECOLOR.contains("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE=final-regions"));
    assert!(!SESSION_LAUNCHER.contains("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE=final-regions"));
    assert!(!SESSION_LAUNCHER.contains("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE=continuous"));
}

#[test]
fn launcher_trace_mode_precedence_is_preserved_when_verbose_and_proof_overlap() {
    let begin = offset("if [[ \"${SOPHIA_SESSION_VERBOSE_TRACE:-false}\" == true ]]");
    let end = offset("session_bus_launcher=()");
    let block = &SESSION_LAUNCHER[begin..end];
    for (verbose, proof, explicit, expected) in [
        ("true", "false", None, Some("1")),
        (
            "true",
            "false",
            Some("final-regions"),
            Some("final-regions"),
        ),
        ("true", "true", Some("continuous"), Some("continuous")),
        ("true", "true", None, Some("final-regions")),
        ("false", "false", None, None),
    ] {
        let mut command = std::process::Command::new("bash");
        command.args(["-eu", "-c", &format!("session_environment=(); {block}\nfor entry in \"${{session_environment[@]}}\"; do export \"$entry\"; done\nprintf '%s' \"${{SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE-unset}}\"")]);
        command
            .env("SOPHIA_SESSION_VERBOSE_TRACE", verbose)
            .env("FIREFOX_M10_RENDERING_PROOF", proof);
        command.env_remove("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE");
        if let Some(value) = explicit {
            command.env("SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE", value);
        }
        let output = command.output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected.unwrap_or("unset")
        );
    }
}

#[test]
fn firefox_m10_gate_uses_the_proven_isolated_native_x_configuration() {
    assert!(SESSION_LAUNCHER.contains("firefox_m10_profile_dir=\"\""));
    assert!(SESSION_LAUNCHER.contains("$firefox_m10_profile_dir/user.js"));
    assert!(SESSION_LAUNCHER.contains("browser.tabs.remote.autostart"));
    assert!(SESSION_LAUNCHER.contains("fission.autostart"));
    assert!(SESSION_LAUNCHER.contains("--session-app-arg=browser=--profile"));
    assert!(SESSION_LAUNCHER.contains("--session-app-arg=browser=$firefox_m10_profile_dir"));
    assert!(SESSION_LAUNCHER.contains("GDK_BACKEND=x11"));
    assert!(SESSION_LAUNCHER.contains("MOZ_ENABLE_WAYLAND=0"));
    assert!(SESSION_LAUNCHER.contains("MOZ_FORCE_DISABLE_E10S=1"));
    assert!(SESSION_LAUNCHER.contains("MOZ_USE_XINPUT2=1"));
}

#[test]
fn firefox_m10_profiles_are_bounded_by_the_session_lifecycle() {
    let prior_wrapper_check = offset("if [[ -s \"$PID_FILE\" ]]");
    let graphical_session_check = offset("if (( ${#active_sessions[@]} > 0 )); then");
    let child_shutdown = offset(
        "[[ -z \"$session_pid\" ]] || terminate_bounded \"-$session_pid\" \"$SESSION_LABEL\"",
    );
    let current_cleanup = offset("if [[ -n \"$firefox_m10_probe_dir\" ]]; then");
    let trap = offset("trap cleanup EXIT");
    let stale_cleanup =
        offset("find \"$STATE_DIR\" -mindepth 1 -maxdepth 1 -type d -name 'firefox-m10.*'");
    let profile_create = offset("firefox_m10_probe_dir=\"$(mktemp -d");

    assert!(prior_wrapper_check < graphical_session_check);
    assert!(graphical_session_check < stale_cleanup);
    assert!(child_shutdown < current_cleanup);
    assert!(trap < stale_cleanup);
    assert!(stale_cleanup < profile_create);
    assert!(SESSION_LAUNCHER.contains("rm -rf -- \"$firefox_m10_probe_dir\""));
    assert!(SESSION_LAUNCHER.contains("firefox_m10_probe_dir=\"\""));
}

#[test]
fn tty3_gate_restores_input_before_activating_the_ready_greetd_vt() {
    let restore_origin = TTY3_LAUNCHER.find("if ! restore_origin_tty").unwrap();
    let restore_manager_tty = TTY3_LAUNCHER.find("if ! restore_greetd_tty").unwrap();
    let restore_manager = TTY3_LAUNCHER
        .find("sudo -n sv up \"$display_manager\"")
        .unwrap();
    let greeter_ready = TTY3_LAUNCHER.find("ps -C tuigreet -o tty=").unwrap();
    let verify_manager_tty = TTY3_LAUNCHER
        .find("elif ! verify_greetd_tty_ready")
        .unwrap();
    let reactivate_tty = TTY3_LAUNCHER
        .find("sudo -n chvt \"$activation_vt\"")
        .unwrap();

    assert!(TTY3_LAUNCHER.contains("origin_tty=\"$(tty)\""));
    assert!(TTY3_LAUNCHER.contains("origin_vt=\"${origin_tty#/dev/tty}\""));
    assert!(
        TTY3_LAUNCHER
            .contains("origin_keyboard_mode=\"$(python3 \"$TTY_MODE_HELPER\" get-keyboard)\"")
    );
    assert!(TTY3_LAUNCHER.contains("display_manager_keyboard_mode=\"$("));
    assert!(TTY3_LAUNCHER.contains("verify_greetd_tty_prestart"));
    assert!(TTY3_LAUNCHER.contains("establish_safe_greetd_tty"));
    assert!(TTY3_LAUNCHER.contains("sudo -n stty sane -F \"$display_manager_tty\""));
    assert!(TTY3_LAUNCHER.contains("phase=exact_prestart"));
    assert!(TTY3_LAUNCHER.contains("phase=safe_prestart"));
    assert!(TTY3_LAUNCHER.contains("phase=live_ready"));
    assert!(TTY3_LAUNCHER.contains("keyboard_mode\" =~ ^[0-3]$"));
    assert!(TTY3_LAUNCHER.contains("stable_samples\" -ge 3"));
    assert!(TTY3_LAUNCHER.contains("manager_restore=%s"));
    assert!(TTY3_LAUNCHER.contains("manager_keyboard=%s"));
    assert!(restore_origin < restore_manager_tty);
    assert!(restore_manager_tty < restore_manager);
    assert!(restore_manager < greeter_ready);
    assert!(greeter_ready < verify_manager_tty);
    assert!(verify_manager_tty < reactivate_tty);
    assert!(TTY3_LAUNCHER.contains("active_vt=\"$(fgconsole 2>/dev/null || true)\""));
    assert!(TTY3_LAUNCHER.contains("sudo -n sv down \"$display_manager\" 2>/dev/null || true"));
    assert!(TTY3_LAUNCHER.contains("sudo -n -v || exit"));
    assert!(TTY3_LAUNCHER.contains("stop_sudo_keepalive"));
    assert!(TTY3_LAUNCHER.contains("sophia_tty_handoff schema=1"));
}

#[test]
fn desktop_comparison_gate_is_terminal_free_local_and_failure_safe() {
    assert!(DESKTOP_COMPARISON_GATE.contains("export SOPHIA_SESSION_STARTUP=none"));
    assert!(DESKTOP_COMPARISON_GATE.contains("export SOPHIA_PROFILE_CHECK_XTASK=\"$xtask\""));
    assert!(DESKTOP_COMPARISON_GATE.contains("trap cleanup_sophia_session EXIT"));
    assert!(DESKTOP_COMPARISON_GATE.contains("exec {operator_tty_fd}<&0"));
    assert!(DESKTOP_COMPARISON_GATE.contains(") <&\"$operator_tty_fd\" &"));
    assert!(DESKTOP_COMPARISON_GATE.contains("operator_tty_fd_tty=$(tty <&\"$operator_tty_fd\")"));
    assert!(!DESKTOP_COMPARISON_GATE.contains("operator_tty_fd}<\"$operator_tty\""));
    assert!(DESKTOP_COMPARISON_GATE.contains("gate-last.log"));
    assert!(DESKTOP_COMPARISON_GATE.contains("run_xtask_logged()"));
    assert!(DESKTOP_COMPARISON_GATE.contains("2>&1 | tee -a \"$diagnostic_log\""));
    assert!(
        DESKTOP_COMPARISON_GATE
            .contains("run_xtask_logged conformance desktop-comparison capture \"$run\"")
    );
    assert!(
        DESKTOP_COMPARISON_GATE
            .contains("run_xtask_logged conformance desktop-comparison finalize \"$run\"")
    );
    assert!(DESKTOP_COMPARISON_GATE.contains(
        "run_xtask_logged conformance desktop-comparison attest \"$run\" \"$supervisor\" || return $?"
    ));
    assert!(
        DESKTOP_COMPARISON_GATE.contains(
            "run_xtask_logged conformance desktop-comparison qualify \"$run\" || return $?"
        )
    );
    assert!(DESKTOP_COMPARISON_GATE.contains("desktop-comparison attest"));
    assert!(DESKTOP_COMPARISON_GATE.contains("desktop-comparison qualify"));
    assert!(DESKTOP_COMPARISON_GATE.contains("desktop-comparison finalize"));
    assert!(DESKTOP_COMPARISON_GATE.contains("xmonad-$(uname -m)-linux"));
    assert!(DESKTOP_COMPARISON_GATE.contains("desktop-comparison cursor-theme"));
    assert!(DESKTOP_COMPARISON_GATE.contains("export XCURSOR_THEME=sophia-x11-core"));
    assert!(DESKTOP_COMPARISON_GATE.contains("xsetroot -cursor_name left_ptr"));
    assert!(DESKTOP_COMPARISON_GATE.contains("install -m 600 \"$source\""));
    assert!(DESKTOP_COMPARISON_GATE.contains("comparison core configuration is not owner-only"));
    assert!(
        DESKTOP_COMPARISON_GATE.contains("export SOPHIA_CORE_CONFIG=\"$comparison_core_config\"")
    );
    assert!(DESKTOP_COMPARISON_GATE.contains("Sophia did not attest the prepared cursor asset"));
    assert!(DESKTOP_COMPARISON_GATE.contains("internal_mode=false"));
    assert!(DESKTOP_COMPARISON_GATE.contains("cleanup exceeded 30 seconds"));
    assert!(DESKTOP_COMPARISON_GATE.contains("^SOPHIA-1 connected primary"));
    assert!(DESKTOP_COMPARISON_GATE.contains("xrandr-last.log"));
    assert!(DESKTOP_COMPARISON_GATE.contains("topology=$(xrandr --query 2>&1)"));
    assert!(DESKTOP_COMPARISON_GATE.contains("status=protocol_error"));
    assert!(DESKTOP_COMPARISON_GATE.contains("for _ in {1..50}"));
    assert!(DESKTOP_COMPARISON_GATE.contains("trap cleanup_niri EXIT"));
    assert!(DESKTOP_COMPARISON_GATE.contains("trap cleanup_xmonad EXIT"));
    assert!(!DESKTOP_COMPARISON_GATE.contains("/tmp/crtc"));
    assert!(!DESKTOP_COMPARISON_GATE.to_ascii_lowercase().contains("ssh"));
    assert!(SESSION_LAUNCHER.contains("SESSION_STARTUP"));
    assert!(SESSION_LAUNCHER.contains("--config=$SOPHIA_CORE_CONFIG"));
    assert!(SESSION_LAUNCHER.contains("sophia_append_session_terminal_registration_args"));
}

#[test]
fn tty_profile_check_is_independent_of_the_callers_working_directory() {
    assert!(TTY3_LAUNCHER.contains("SOPHIA_PROFILE_CHECK_XTASK"));
    assert!(TTY3_LAUNCHER.contains("--manifest-path \"$ROOT_DIR/Cargo.toml\""));
    assert!(TTY3_LAUNCHER.contains("--package xtask -- profile check"));
    assert!(!TTY3_LAUNCHER.contains("cargo xtask profile check"));
}

#[test]
fn desktop_comparison_gate_records_a_non_tty_admission_failure() {
    let root = unique_temp_dir("comparison-no-tty");
    let runtime = root.join("runtime");
    let state = root.join("state");
    std::fs::create_dir_all(&runtime).unwrap();
    let gate = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/desktop_comparison_tty3.sh");
    let output = std::process::Command::new("bash")
        .arg(gate)
        .arg(root.join("unused-run"))
        .env("SOPHIA_DESKTOP_COMPARISON_XTASK", "/bin/true")
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("XDG_STATE_HOME", &state)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("standard input is not a terminal")
    );
    let diagnostic =
        std::fs::read_to_string(state.join("sophia/desktop-comparison/gate-last.log")).unwrap();
    assert!(diagnostic.contains("status=entered stage=tty-admission"));
    assert!(diagnostic.contains("status=failed stage=tty-admission"));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn installed_session_uses_only_versioned_release_artifacts() {
    assert!(INSTALLED_SESSION.contains("SOPHIA_BUILD_SESSION=false"));
    assert!(INSTALLED_SESSION.contains("SOPHIA_MANAGE_KEYD=false"));
    assert!(INSTALLED_SESSION.contains("$RELEASE_DIR/target/release/sophia"));
    assert!(!INSTALLED_SESSION.contains("cargo "));
    assert!(!INSTALLED_SESSION.contains("sudo "));
}

#[test]
fn installed_hagia_separates_personal_and_packaged_promotion_profiles() {
    assert!(INSTALLED_HAGIA.contains("$config_home/hagia/config.kdl"));
    assert!(INSTALLED_HAGIA.contains("/etc/hagia/config.kdl"));
    assert!(INSTALLED_HAGIA.contains("packaged-fallback"));
    assert!(INSTALLED_HAGIA_PROMOTION.contains("packaged-promotion"));
    assert!(INSTALLED_HAGIA_PROMOTION.contains("unset SOPHIA_DESKTOP_PROFILE"));
    assert!(SESSION_LAUNCHER.contains("--desktop-profile=$desktop_profile"));
    assert!(SESSION_LAUNCHER.contains("Ctrl+Alt+Delete to log out"));
}

#[test]
fn desktop_startup_does_not_require_a_focused_app_but_proofs_do() {
    let start = offset("# A user-composed desktop may start only panels");
    let end = start
        + SESSION_LAUNCHER[start..]
            .find("if [[ \"$SESSION_PROFILE\" == standalone ]]; then")
            .unwrap();
    let block = &SESSION_LAUNCHER[start..end];
    for (profile, startup, firefox, truecolor, expected) in [
        ("hagia", "terminal", "false", "false", false),
        ("hagia", "none", "false", "false", false),
        ("hagia", "terminal", "true", "false", true),
        ("hagia", "terminal", "false", "true", true),
        ("standalone", "terminal", "false", "false", true),
        ("native", "terminal", "false", "false", true),
        ("kitty", "terminal", "false", "false", true),
    ] {
        // Execute only argument assembly; never enter the live launcher.
        let output = std::process::Command::new("bash")
            .args([
                "-eu",
                "-c",
                &format!("session_args=(); {block}\nprintf '%s' \"${{session_args[*]}}\""),
            ])
            .env("SESSION_PROFILE", profile)
            .env("SESSION_STARTUP", startup)
            .env("FIREFOX_M10_ANY_PROOF", firefox)
            .env("TRUECOLOR_PROOF", truecolor)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("--startup-ready-timeout-ms=8000"),
            expected,
            "profile={profile} startup={startup} firefox={firefox} truecolor={truecolor}"
        );
    }
}

#[test]
fn installed_watchdog_is_fixed_and_opt_in() {
    assert!(INSTALLED_RECOVERY.contains("SOPHIA_SESSION_WATCHDOG_SECONDS=45"));
    assert!(INSTALLED_RECOVERY.contains("$RELEASE_DIR/bin/sophia-hagia-session"));
    assert!(!INSTALLED_SESSION.contains("export SOPHIA_SESSION_WATCHDOG_SECONDS="));
}

#[test]
fn installer_preserves_a_rollback_pointer_before_activation() {
    let verify = ACTIVATOR.find("sha256sum -c SHA256SUMS").unwrap();
    let preserve = ACTIVATOR
        .find("mv -Tf \"$previous_temp\" \"$PREFIX/previous\"")
        .unwrap();
    let activate = ACTIVATOR
        .find("mv -Tf \"$current_temp\" \"$PREFIX/current\"")
        .unwrap();
    assert!(verify < preserve);
    assert!(preserve < activate);
    assert!(INSTALLER.contains("sha256sum -c SHA256SUMS"));
    assert!(INSTALLER.contains("activate_live_session_release.sh"));
}

#[test]
fn installer_verifies_root_owned_staging_before_immutable_promotion() {
    let copy = INSTALLER.find("cp -a \"$artifact\" \"$staging\"").unwrap();
    let ownership = INSTALLER.find("chown -R 0:0 -- \"$staging\"").unwrap();
    let staged_ledger = ownership
        + INSTALLER[ownership..]
            .find("sha256sum -c SHA256SUMS")
            .unwrap();
    let verify = INSTALLER
        .find("\"$staging/tools/verify_packaged_policy.sh\" \"$staging\"")
        .unwrap();
    let promote = INSTALLER.find("mv \"$staging\" \"$target\"").unwrap();

    assert!(copy < ownership);
    assert!(ownership < staged_ledger);
    assert!(staged_ledger < verify);
    assert!(verify < promote);
    assert!(!INSTALLER.contains("\"$artifact/tools/verify_packaged_policy.sh\""));
}

/// The policy client is owner-only, and the release remains the fallback.
///
/// This path names the process that will be handed the WM socket, so the
/// checks around it are the point rather than decoration. A binary someone
/// else can write is a binary someone else can make the window manager.
#[test]
fn a_user_policy_client_is_refused_unless_its_owner_alone_can_write_it() {
    let override_block = INSTALLED_SESSION
        .split_once("sophia_user_wm=")
        .expect("installed session launcher never looks for a user policy client")
        .1;
    let export = override_block
        .find("export SOPHIA_HAGIA_BIN")
        .expect("the developer override never exports the policy client");
    let decision = &override_block[..export];

    assert!(
        decision.contains("-O \"$sophia_user_wm\""),
        "a policy client owned by another user must be refused"
    );
    assert!(
        decision.contains("*w*w*"),
        "a group- or world-writable policy client must be refused"
    );
    assert!(
        decision.contains("exit 1"),
        "an unsafe policy client must stop the session rather than fall back to \
         the release, which would silently run something else"
    );
    assert!(
        decision.contains("$RELEASE_DIR/target/release/hagia"),
        "the release binary must remain the default when no override exists"
    );

    // The user copy is chosen only when it is there, so an install that never
    // placed one still starts on the packaged binary.
    let opt_in = decision
        .find("elif [[ -x \"$sophia_user_wm\" ]]")
        .expect("the user policy client is not chosen by the file existing");
    assert!(
        decision[..opt_in].contains("-n \"${SOPHIA_HAGIA_BIN:-}\""),
        "an explicit SOPHIA_HAGIA_BIN must still win, which is how gates point \
         at their fixtures"
    );
}

/// Reloading must never write into the installed release.
#[test]
fn the_reload_tool_leaves_the_installed_release_alone() {
    const RELOAD: &str = include_str!("../../../tools/reload_policy_client.sh");
    assert!(
        !RELOAD.contains("/opt/sophia"),
        "the reload tool must not touch the checksummed release"
    );
    assert!(
        !RELOAD.contains("sudo"),
        "reloading a policy client is not a privileged operation"
    );
    assert!(
        RELOAD.contains("rolling back"),
        "a reload that does not come back must restore what it replaced"
    );
    // The running client is identified before the new binary lands. Installing
    // unlinks the old inode, after which /proc/PID/exe reads as "(deleted)",
    // so a check made afterwards would fail against its own install.
    let install = RELOAD
        .find("mv -f \"$staged\"")
        .expect("the reload tool never installs the binary");
    let identify = RELOAD
        .find("running_exe=")
        .expect("the reload tool never identifies the running client");
    assert!(
        identify < install,
        "the running client must be identified before its binary is replaced"
    );
    assert!(
        RELOAD.contains("% (deleted)"),
        "a client whose binary was already replaced is still that client"
    );
    assert!(
        RELOAD.contains("status=(layout_committed|focus_committed)"),
        "a started process is not a working one; the reload must wait for the \
         session to accept a projection from the replacement"
    );
}

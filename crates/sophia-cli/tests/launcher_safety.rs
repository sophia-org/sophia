const SESSION_LAUNCHER: &str = include_str!("../../../tools/run_sophia_session.sh");
const TTY_MODE_HELPER: &str = include_str!("../../../tools/sophia_tty_mode.py");

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
fn session_launcher_keeps_unsanitized_child_output_private_and_explicit() {
    assert!(SESSION_LAUNCHER.contains(
        "SOPHIA_UNTRUSTED_SESSION_OUTPUT_LOG must name untrusted-session-output.log in the diagnostic directory."
    ));
    assert!(SESSION_LAUNCHER.contains("chmod 600 \"$UNTRUSTED_OUTPUT_LOG\""));
    assert!(
        SESSION_LAUNCHER
            .contains("setsid \"${session_launch[@]}\" >\"$UNTRUSTED_OUTPUT_LOG\" 2>&1 &")
    );
}

#[test]
fn input_guard_wait_uses_the_validated_bound() {
    assert!(offset("prepare-controls") < offset("mkdir -p \"$STATE_DIR\""));
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
fn external_host_preflight_precedes_inputs_guard_and_service_changes() {
    let check = offset("\"$SOPHIA_BIN\" session check-host \"--tty=$tty_name\"");
    assert!(offset("lifecycle_phase entering preflight") < check);
    assert!(offset("if [[ ! -t 0 ]]") < check);
    assert!(offset("if [[ \"$REQUIRE_LOCAL_VT\" == true") < check);
    assert!(check < offset("input_source_args="));
    assert!(check < offset("sudo sv down keyd"));
    assert!(check < offset("Emergency input guard armed."));
    assert!(!SESSION_LAUNCHER.contains("--allow-active"));
    assert!(!SESSION_LAUNCHER.contains("live_named_processes"));
}

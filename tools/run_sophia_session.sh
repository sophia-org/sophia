#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/tools/lib/session_lifecycle.sh"
source "$ROOT_DIR/tools/lib/session_terminal.sh"
SOPHIA_BIN="${SOPHIA_BIN:-$ROOT_DIR/target/release/sophia}"
SOPHIA_HAGIA_BIN="${SOPHIA_HAGIA_BIN:-$(command -v hagia 2>/dev/null || true)}"
TTY_MODE_HELPER="${SOPHIA_TTY_MODE_HELPER:-$ROOT_DIR/tools/sophia_tty_mode.py}"
BUILD_SESSION="${SOPHIA_BUILD_SESSION:-true}"
MANAGE_KEYD="${SOPHIA_MANAGE_KEYD:-true}"
INSTALLED_SESSION="${SOPHIA_INSTALLED_SESSION:-false}"
INSTALLED_VERSION="${SOPHIA_INSTALLED_VERSION:-unknown}"
INSTALLED_COMMIT="${SOPHIA_INSTALLED_COMMIT:-unknown}"
[[ "$INSTALLED_VERSION" =~ ^[0-9A-Za-z._-]+$ ]] || INSTALLED_VERSION=unknown
[[ "$INSTALLED_COMMIT" =~ ^[0-9A-Za-z._-]+$ ]] || INSTALLED_COMMIT=unknown
REQUIRE_RUNTIME_DIR="${SOPHIA_REQUIRE_RUNTIME_DIR:-false}"
REQUIRE_LOCAL_VT="${SOPHIA_REQUIRE_LOCAL_VT:-false}"
DISPLAY_NAME="${SOPHIA_LIVE_SESSION_DISPLAY:-:77}"
SESSION_PROFILE="${SOPHIA_TTY_PROFILE:-}"
SESSION_STARTUP="${SOPHIA_SESSION_STARTUP:-terminal}"
SESSION_WATCHDOG_SECONDS="${SOPHIA_SESSION_WATCHDOG_SECONDS:-}"
INPUT_GUARD_ARM_TIMEOUT_SECONDS="${SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS:-30}"
INPUT_GUARD_ARMING="${SOPHIA_INPUT_GUARD_ARMING:-manual}"
SESSION_HANDOFF="${SOPHIA_SESSION_HANDOFF:-display_manager}"
TRUECOLOR_PROOF="${SOPHIA_TRUECOLOR_PROOF:-false}"
FIREFOX_M10_PROOF=false
FIREFOX_M10_RENDERING_PROOF=false
FIREFOX_M10_DIALOG_PROOF=false
FIREFOX_M10_PRIMARY_PROOF=false
FIREFOX_M10_SELECTION_PROOF=false
FIREFOX_M10_LIFECYCLE_PROOF=false
for argument in "$@"; do
    case "$argument" in
        --firefox-m10-proof) FIREFOX_M10_PROOF=true ;;
        --firefox-m10-rendering-proof) FIREFOX_M10_RENDERING_PROOF=true ;;
        --firefox-m10-dialog-proof) FIREFOX_M10_DIALOG_PROOF=true ;;
        --firefox-m10-primary-proof) FIREFOX_M10_PRIMARY_PROOF=true ;;
        --firefox-m10-selection-proof) FIREFOX_M10_SELECTION_PROOF=true ;;
        --firefox-m10-lifecycle-proof) FIREFOX_M10_LIFECYCLE_PROOF=true ;;
    esac
done
FIREFOX_M10_ANY_PROOF=false
if [[ "$FIREFOX_M10_PROOF" == true
    || "$FIREFOX_M10_RENDERING_PROOF" == true
    || "$FIREFOX_M10_DIALOG_PROOF" == true
    || "$FIREFOX_M10_PRIMARY_PROOF" == true
    || "$FIREFOX_M10_SELECTION_PROOF" == true
    || "$FIREFOX_M10_LIFECYCLE_PROOF" == true ]]; then
    FIREFOX_M10_ANY_PROOF=true
fi
if [[ "$SESSION_PROFILE" != standalone
    && "$SESSION_PROFILE" != native
    && "$SESSION_PROFILE" != hagia
    && "$SESSION_PROFILE" != kitty ]]; then
    echo "SOPHIA_TTY_PROFILE must be standalone, native, hagia, or kitty." >&2
    exit 1
fi
if [[ "$SESSION_STARTUP" != terminal && "$SESSION_STARTUP" != none ]]; then
    echo "SOPHIA_SESSION_STARTUP must be terminal or none." >&2
    exit 1
fi
if [[ "$SESSION_STARTUP" == none && "$SESSION_PROFILE" != hagia ]]; then
    echo "A terminal-free normal session is supported only by the Hagia profile." >&2
    exit 1
fi
if [[ -n "$SESSION_WATCHDOG_SECONDS"
    && ! "$SESSION_WATCHDOG_SECONDS" =~ ^[1-9][0-9]*$ ]]; then
    echo "SOPHIA_SESSION_WATCHDOG_SECONDS must be a positive integer when set." >&2
    exit 1
fi
if [[ ! "$INPUT_GUARD_ARM_TIMEOUT_SECONDS" =~ ^[1-9][0-9]*$
    || "$INPUT_GUARD_ARM_TIMEOUT_SECONDS" -gt 300 ]]; then
    echo "SOPHIA_INPUT_GUARD_ARM_TIMEOUT_SECONDS must be an integer from 1 through 300." >&2
    exit 1
fi
INPUT_GUARD_ARM_WAIT_TICKS=$((INPUT_GUARD_ARM_TIMEOUT_SECONDS * 20))
if [[ "$INPUT_GUARD_ARMING" != manual && "$INPUT_GUARD_ARMING" != automatic ]]; then
    echo "SOPHIA_INPUT_GUARD_ARMING must be manual or automatic." >&2
    exit 1
fi
if [[ "$SESSION_HANDOFF" != display_manager && "$SESSION_HANDOFF" != cycle_runner ]]; then
    echo "SOPHIA_SESSION_HANDOFF must be display_manager or cycle_runner." >&2
    exit 1
fi
if [[ "$TRUECOLOR_PROOF" != true && "$TRUECOLOR_PROOF" != false ]]; then
    echo "SOPHIA_TRUECOLOR_PROOF must be true or false." >&2
    exit 1
fi
if [[ "$TRUECOLOR_PROOF" == true && "$SESSION_PROFILE" != hagia ]]; then
    echo "The TrueColor proof requires the Hagia session profile." >&2
    exit 1
fi
normal_application_defaults=false
if sophia_session_uses_application_defaults \
    "$SESSION_PROFILE" "$FIREFOX_M10_ANY_PROOF" "$TRUECOLOR_PROOF"; then
    normal_application_defaults=true
fi
SESSION_LABEL="Sophia $SESSION_PROFILE session"
runtime_root="${XDG_RUNTIME_DIR:-/tmp}"
tty_name="$(tty 2>/dev/null || true)"
STATE_DIR="$runtime_root/sophia-${SESSION_PROFILE}-session-${UID}"
PID_FILE="$STATE_DIR/wrapper.pid"
GUARD_ARMED_FILE="$STATE_DIR/input-guard.armed"
GUARD_TRIGGERED_FILE="$STATE_DIR/input-guard.triggered"
WATCHDOG_TRIGGERED_FILE="$STATE_DIR/session-watchdog.triggered"

mkdir -p "$STATE_DIR"
chmod 700 "$STATE_DIR"
firefox_m10_probe_dir=""
firefox_m10_profile_dir=""
if [[ -s "$PID_FILE" ]]; then
    previous_pid="$(<"$PID_FILE")"
    if [[ "$previous_pid" =~ ^[0-9]+$ ]] && kill -0 "$previous_pid" 2>/dev/null; then
        echo "A $SESSION_LABEL is already running (wrapper PID $previous_pid)." >&2
        echo "Stop it with: tools/stop_sophia_${SESSION_PROFILE}_session.sh" >&2
        exit 1
    fi
    rm -f "$PID_FILE"
fi

LOG_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/sophia/${SESSION_PROFILE}-session"
LOG_DIR="${SOPHIA_DIAGNOSTIC_DIR:-$LOG_DIR}"
GUARD_LOG="$LOG_DIR/input-guard.log"
RECOVERY_LOG="$LOG_DIR/recovery.log"
SESSION_LOG="$LOG_DIR/session.log"
LIFECYCLE_LOG="$LOG_DIR/lifecycle.log"
UNTRUSTED_OUTPUT_LOG="${SOPHIA_UNTRUSTED_SESSION_OUTPUT_LOG:-}"
if [[ -n "$UNTRUSTED_OUTPUT_LOG"
    && "$UNTRUSTED_OUTPUT_LOG" != "$LOG_DIR/untrusted-session-output.log" ]]; then
    echo "SOPHIA_UNTRUSTED_SESSION_OUTPUT_LOG must name untrusted-session-output.log in the diagnostic directory." >&2
    exit 1
fi
mkdir -p "$LOG_DIR"
chmod 700 "$LOG_DIR"
sophia_session_rotate_log "$LIFECYCLE_LOG"
sophia_session_rotate_log "$GUARD_LOG"
sophia_session_rotate_log "$RECOVERY_LOG"
sophia_session_rotate_log "$SESSION_LOG"
if [[ -n "$UNTRUSTED_OUTPUT_LOG" ]]; then
    : >"$UNTRUSTED_OUTPUT_LOG"
    chmod 600 "$UNTRUSTED_OUTPUT_LOG"
fi
lifecycle_phase() {
    printf 'sophia_session_lifecycle schema=1 status=%s phase=%s installed=%s build=%s manual_service=%s runtime=%s vt=%s\n' \
        "$1" "$2" "$INSTALLED_SESSION" "$BUILD_SESSION" "$MANAGE_KEYD" \
        "$([[ "$runtime_root" == /tmp ]] && echo temporary || echo owner)" \
        "$([[ "$tty_name" =~ ^/dev/tty[0-9]+$ ]] && echo local || echo other)" \
        >>"$LIFECYCLE_LOG"
}
lifecycle_current_phase=preflight
lifecycle_diagnostic_written=false
record_lifecycle_failure() {
    local phase="$1" status="$2"
    if [[ "$lifecycle_diagnostic_written" == false && "$status" != 0 ]]; then
        sophia_session_record_failure \
            "$LIFECYCLE_LOG" "$phase" "$INSTALLED_SESSION" \
            "$INSTALLED_VERSION" "$INSTALLED_COMMIT" "$status"
        lifecycle_diagnostic_written=true
    fi
}
record_early_lifecycle_failure() {
    local status=$?
    record_lifecycle_failure "$lifecycle_current_phase" "$status"
    return "$status"
}
lifecycle_phase entering preflight
trap record_early_lifecycle_failure EXIT
if [[ ! -t 0 ]]; then
    echo "Run this interactively from a dedicated local TTY." >&2
    exit 1
fi
if [[ "$REQUIRE_RUNTIME_DIR" == true ]]; then
    [[ -n "${XDG_RUNTIME_DIR:-}" && -d "$XDG_RUNTIME_DIR" ]] || {
        echo "Installed Sophia requires an existing XDG_RUNTIME_DIR." >&2
        exit 1
    }
    [[ "$XDG_RUNTIME_DIR" == /* && "$(stat -c %u "$XDG_RUNTIME_DIR")" == "$UID" ]] || {
        echo "Installed Sophia requires an absolute, user-owned XDG_RUNTIME_DIR." >&2
        exit 1
    }
fi
if [[ "$REQUIRE_LOCAL_VT" == true && ! "$tty_name" =~ ^/dev/tty[0-9]+$ ]]; then
    echo "Installed Sophia requires a local Linux VT; observed: $tty_name" >&2
    exit 1
fi
if [[ "$INSTALLED_SESSION" == true
    && ( "$BUILD_SESSION" != false || "$MANAGE_KEYD" != false ) ]]; then
    echo "Installed Sophia forbids source builds and manual service control." >&2
    exit 1
fi

live_named_processes() {
    local name pid state
    for name in "$@"; do
        while read -r pid; do
            [[ -n "$pid" ]] || continue
            state="$(ps -o stat= -p "$pid" 2>/dev/null || true)"
            [[ "$state" == Z* ]] || printf '%s:%s\n' "$name" "$pid"
        done < <(pgrep -x "$name" 2>/dev/null || true)
    done
}
active_sessions=()
for process in river niri sway Hyprland kwin_wayland Xorg; do
    while read -r active; do
        [[ -n "$active" ]] && active_sessions+=("$active")
    done < <(live_named_processes "$process")
done
if (( ${#active_sessions[@]} > 0 )); then
    echo "Refusing to take over a TTY while a graphical session is active." >&2
    echo "Still active (process:pid): ${active_sessions[*]}" >&2
    exit 1
fi

input_seat="${SOPHIA_OPERATOR_INPUT_SEAT:-seat0}"
input_devices="${SOPHIA_OPERATOR_INPUT_DEVICES:-}"
input_source_args=()
if [[ -n "$input_devices" ]]; then
    input_source_args+=("--input-devices=$input_devices")
else
    input_source_args+=("--input-seat=$input_seat")
fi

cd "$ROOT_DIR"
if [[ "$BUILD_SESSION" == true ]]; then
    cargo build --offline --release -p sophia-cli --features native-session
    if [[ "$SESSION_PROFILE" == native || "$SESSION_PROFILE" == standalone ]]; then
        cargo build --offline --release -p sophia-wm-demo
    fi
    tools/atomic_scanout_preflight.sh
fi
[[ -x "$SOPHIA_BIN" ]] || {
    echo "Sophia session binary is not executable: $SOPHIA_BIN" >&2
    exit 1
}
hagia_browser_bin=""
if [[ "$SESSION_PROFILE" == hagia ]]; then
    if [[ "$FIREFOX_M10_ANY_PROOF" == true ]]; then
        hagia_browser_bin="${SOPHIA_FIREFOX_BIN:-$(command -v firefox || true)}"
    else
        hagia_browser_bin="${SOPHIA_HAGIA_BROWSER_BIN:-$(command -v helium || command -v firefox || true)}"
    fi
    if [[ "$normal_application_defaults" == true ]]; then
        if [[ -n "$hagia_browser_bin" && ! -x "$hagia_browser_bin" ]]; then
            echo "The default browser is not executable: $hagia_browser_bin" >&2
            exit 1
        fi
    elif [[ -z "$hagia_browser_bin" || ! -x "$hagia_browser_bin" ]]; then
        echo "The Hagia profile requires Helium, Firefox, or SOPHIA_HAGIA_BROWSER_BIN." >&2
        exit 1
    fi
fi
lifecycle_phase complete preflight

keyd_was_running=false
tty_state=""
kd_mode=""
keyboard_mode=""
guard_pid=""
watchdog_pid=""
session_pid=""
cleanup_done=false
emergency_session_shutdown=not_requested
emergency_session_exit_status=none
terminate_bounded() {
    local target="$1" label="$2"
    if ! kill -0 -- "$target" 2>/dev/null; then
        return 0
    fi
    kill -TERM -- "$target" 2>/dev/null || true
    for _ in {1..40}; do
        if ! kill -0 -- "$target" 2>/dev/null; then
            wait "${target#-}" 2>/dev/null || true
            return 0
        fi
        sleep 0.05
    done
    echo "WARNING: $label did not stop after TERM; sending KILL." >&2
    kill -KILL -- "$target" 2>/dev/null || true
    wait "${target#-}" 2>/dev/null || true
}
cleanup() {
    local status=$?
    if [[ "$cleanup_done" == true ]]; then
        return "$status"
    fi
    cleanup_done=true
    local emergency=false handoff_failed=false operator_emergency=false watchdog_failure=false
    [[ ! -s "$GUARD_TRIGGERED_FILE" ]] || operator_emergency=true
    [[ ! -s "$WATCHDOG_TRIGGERED_FILE" ]] || watchdog_failure=true
    if [[ "$operator_emergency" == true || "$watchdog_failure" == true ]]; then
        emergency=true
    fi
    [[ -z "$watchdog_pid" ]] || terminate_bounded "$watchdog_pid" "Sophia session watchdog"
    watchdog_pid=""
    [[ -z "$session_pid" ]] || terminate_bounded "-$session_pid" "$SESSION_LABEL"
    session_pid=""
    [[ -z "$guard_pid" ]] || terminate_bounded "$guard_pid" "Sophia input guard"
    guard_pid=""
    if [[ -n "$firefox_m10_probe_dir" ]]; then
        rm -rf -- "$firefox_m10_probe_dir"
        firefox_m10_probe_dir=""
        firefox_m10_profile_dir=""
    fi
    rm -f "$PID_FILE"
    if [[ -n "$kd_mode" ]] && ! python3 "$TTY_MODE_HELPER" "$kd_mode" 2>/dev/null; then
        status=1
        handoff_failed=true
    fi
    if [[ -n "$keyboard_mode" ]] \
        && ! python3 "$TTY_MODE_HELPER" "keyboard-$keyboard_mode" 2>/dev/null; then
        status=1
        handoff_failed=true
    fi
    if [[ -n "$tty_state" ]] && ! stty "$tty_state" 2>/dev/null; then
        status=1
        handoff_failed=true
    fi
    if [[ "$keyd_was_running" == true ]]; then
        echo
        echo "Restoring keyd..."
        if ! sudo sv up keyd; then
            echo "WARNING: keyd could not be restored; run: sudo sv up keyd" >&2
            status=1
            handoff_failed=true
        else
            for _ in {1..200}; do
                pgrep -x keyd >/dev/null 2>&1 && break
                sleep 0.05
            done
            if ! pgrep -x keyd >/dev/null 2>&1; then
                echo "WARNING: keyd did not become ready after restoration." >&2
                status=1
                handoff_failed=true
            fi
        fi
    fi
    rm -f "$GUARD_ARMED_FILE" "$GUARD_TRIGGERED_FILE" "$WATCHDOG_TRIGGERED_FILE"
    if [[ -n "$kd_mode" && -n "$tty_state" ]]; then
        local restored_kd restored_keyboard restored_termios keyd_restored
        restored_kd="$(python3 "$TTY_MODE_HELPER" get 2>/dev/null || echo unavailable)"
        restored_keyboard="$(python3 "$TTY_MODE_HELPER" get-keyboard 2>/dev/null || echo unavailable)"
        restored_termios="$(stty -g 2>/dev/null || echo unavailable)"
        # keyd_restored is a conclusion: true means there was nothing to put
        # back or it is back, and on a machine without keyd it reads true on
        # every run. keyd_seen is the observation it rests on.
        keyd_restored=true
        if [[ "$keyd_was_running" == true ]] && ! pgrep -x keyd >/dev/null 2>&1; then
            keyd_restored=false
        fi
        printf 'sophia_tty_recovery schema=3 profile=%s kd_mode_before=%s kd_mode_after=%s termios_restored=%s emergency=%s session_shutdown=%s session_exit_status=%s\n' \
            "$SESSION_PROFILE" \
            "$kd_mode" "$restored_kd" \
            "$([[ "$restored_termios" == "$tty_state" ]] && echo true || echo false)" \
            "$emergency" \
            "$emergency_session_shutdown" \
            "$emergency_session_exit_status" >>"$RECOVERY_LOG"
        printf 'sophia_tty_recovery_verification schema=1 profile=%s keyboard_mode_before=%s keyboard_mode_after=%s keyd_seen=%s keyd_restored=%s\n' \
            "$SESSION_PROFILE" "$keyboard_mode" "$restored_keyboard" "$keyd_was_running" "$keyd_restored" \
            >>"$RECOVERY_LOG"
        if [[ "$restored_kd" != "$kd_mode" \
            || "$restored_keyboard" != "$keyboard_mode" \
            || "$restored_termios" != "$tty_state" \
            || "$keyd_restored" != true ]]; then
            status=1
            handoff_failed=true
        fi
    fi
    if [[ "$handoff_failed" == true ]]; then
        record_lifecycle_failure handoff "$status"
    elif [[ "$status" != 0 \
        && ( "$operator_emergency" == false || "$watchdog_failure" == true ) ]]; then
        record_lifecycle_failure "$lifecycle_current_phase" "$status"
    fi
    printf 'sophia_session_lifecycle schema=1 status=returned phase=handoff installed=%s exit_status=%s emergency=%s handoff=%s\n' \
        "$INSTALLED_SESSION" "$status" "$emergency" "$SESSION_HANDOFF" \
        >>"$LIFECYCLE_LOG"
    return "$status"
}
stop_from_signal() {
    local status="$1"
    exit "$status"
}
trap cleanup EXIT
trap 'stop_from_signal 130' INT
trap 'stop_from_signal 143' TERM
printf '%s\n' "$$" >"$PID_FILE"

if [[ "$FIREFOX_M10_ANY_PROOF" == true ]]; then
    # Proof profiles can exceed 100 MiB. They are session resources, not
    # retained evidence, so reclaim prior profiles before allocating this one.
    find "$STATE_DIR" -mindepth 1 -maxdepth 1 -type d -name 'firefox-m10.*' \
        -exec rm -rf -- {} +
    firefox_m10_probe_dir="$(mktemp -d "$STATE_DIR/firefox-m10.XXXXXX")"
    firefox_m10_profile_dir="$firefox_m10_probe_dir/firefox-profile"
    mkdir -p "$firefox_m10_profile_dir"
    chmod 700 "$firefox_m10_profile_dir"
    printf '%s\n' \
        'user_pref("browser.tabs.remote.autostart", false);' \
        'user_pref("browser.tabs.remote.autostart.2", false);' \
        'user_pref("fission.autostart", false);' \
        'user_pref("middlemouse.paste", true);' \
        'user_pref("middlemouse.contentLoadURL", false);' \
        >"$firefox_m10_profile_dir/user.js"
    chmod 600 "$firefox_m10_profile_dir/user.js"
fi

tty_state="$(stty -g)"
kd_mode="$(python3 "$TTY_MODE_HELPER" get)"
keyboard_mode="$(python3 "$TTY_MODE_HELPER" get-keyboard)"

if [[ "$MANAGE_KEYD" == true ]] && pgrep -x keyd >/dev/null 2>&1; then
    echo "Temporarily stopping keyd so Sophia can own the keyboard..."
    sudo -v
    sudo sv down keyd
    keyd_was_running=true
fi

rm -f "$GUARD_ARMED_FILE" "$GUARD_TRIGGERED_FILE" "$WATCHDOG_TRIGGERED_FILE"
lifecycle_current_phase=input_guard
lifecycle_phase entering input_guard
"$SOPHIA_BIN" session input-guard \
    "${input_source_args[@]}" \
    --arming="$INPUT_GUARD_ARMING" \
    --armed-file="$GUARD_ARMED_FILE" \
    --triggered-file="$GUARD_TRIGGERED_FILE" \
    --owner-pid="$$" >>"$GUARD_LOG" 2>&1 &
guard_pid=$!
if [[ "$INPUT_GUARD_ARMING" == manual ]]; then
    echo "Safety check: press and release Ctrl-Alt-Backspace once to arm recovery."
    echo "During Sophia, press Ctrl-Alt-Backspace again for emergency recovery."
else
    echo "Waiting for the emergency input guard to open keyboard input."
fi
for ((guard_wait_tick = 0; guard_wait_tick < INPUT_GUARD_ARM_WAIT_TICKS; guard_wait_tick++)); do
    [[ ! -s "$GUARD_ARMED_FILE" ]] || break
    kill -0 "$guard_pid" 2>/dev/null || {
        echo "Input guard exited before arming; see $GUARD_LOG" >&2
        exit 1
    }
    sleep 0.05
done
[[ -s "$GUARD_ARMED_FILE" ]] || {
    echo "Input guard was not armed within $INPUT_GUARD_ARM_TIMEOUT_SECONDS seconds; refusing graphics takeover." >&2
    exit 1
}
echo "Emergency input guard armed."
lifecycle_phase complete input_guard

if [[ "$SESSION_PROFILE" == standalone ]]; then
    echo "Starting Sophia's standalone single-application proof on $DISPLAY_NAME."
    echo "No terminal, window manager, or status bar will run."
    echo "There are no shortcuts: they need a policy client and none runs here."
    echo "Quit the application to end the session; Ctrl+Alt+Backspace is the"
    echo "emergency path and is recorded as one."
elif [[ "$SESSION_PROFILE" == native ]]; then
    echo "Starting Sophia's session-lifecycle proof on $DISPLAY_NAME."
    echo "No window manager runs; Hagia is Sophia's native WM."
    echo "There are no shortcuts: they need a policy client and none runs here."
    echo "Exit the terminal to end the session; Ctrl+Alt+Backspace is the"
    echo "emergency path and is recorded as one."
elif [[ "$SESSION_PROFILE" == hagia ]]; then
    echo "Starting Sophia with Hagia's native policy on $DISPLAY_NAME."
    echo "Use Super+Enter for Kitty or Ctrl+Alt+Delete to log out."
else
    echo "Starting the supported Kitty-only Sophia input session on $DISPLAY_NAME."
    echo "A policy client and Super+Enter are intentionally disabled for this input gate."
    echo "Exit Kitty normally to return to tty3."
fi
echo "Press Ctrl-Alt-Backspace for local emergency recovery."
echo "The outside control plane may also run tools/stop_sophia_${SESSION_PROFILE}_session.sh."
terminal_bin=""
standalone_bin=""
standalone_workload=""
glxgears_duration=""
glxgears_width=""
glxgears_height=""
xterm_duration=""
xterm_width=""
xterm_height=""
xterm_lines=""
xterm_interval_msec=""
if [[ "$SESSION_PROFILE" == standalone ]]; then
    standalone_workload="${SOPHIA_STANDALONE_WORKLOAD:-vkcube}"
    case "$standalone_workload" in
        glxgears)
            standalone_default_bin="$(command -v glxgears || true)"
            standalone_requirement=glxgears
            glxgears_duration="${SOPHIA_GLXGEARS_DURATION_SECONDS:-20}"
            glxgears_width="${SOPHIA_GLXGEARS_WIDTH:-500}"
            glxgears_height="${SOPHIA_GLXGEARS_HEIGHT:-500}"
            [[ "$glxgears_duration" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_GLXGEARS_DURATION_SECONDS must be a positive integer." >&2
                exit 1
            }
            [[ "$glxgears_width" =~ ^[1-9][0-9]*$
                && "$glxgears_height" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_GLXGEARS_WIDTH and SOPHIA_GLXGEARS_HEIGHT must be positive integers." >&2
                exit 1
            }
            ;;
        kitty)
            # The client this stack is known to hand DMA-BUFs. vkcube
            # presents through the software path here -- 389 Presents,
            # every one a CPU layer -- while Kitty produced DMA-BUF
            # content in every promoted Hagia archive. Direct scanout
            # needs a client buffer, so the probe uses the one that
            # provides one.
            standalone_default_bin="$(command -v kitty || true)"
            standalone_requirement=kitty
            ;;
        vkcube)
            standalone_default_bin="$(command -v vkcube || true)"
            standalone_requirement=vkcube
            ;;
        xterm)
            standalone_default_bin="$(command -v xterm || true)"
            standalone_requirement=xterm
            xterm_duration="${SOPHIA_XTERM_DURATION_SECONDS:-20}"
            xterm_width="${SOPHIA_XTERM_WIDTH:-500}"
            xterm_height="${SOPHIA_XTERM_HEIGHT:-500}"
            xterm_lines="${SOPHIA_XTERM_LINES:-1}"
            xterm_interval_msec="${SOPHIA_XTERM_INTERVAL_MSEC:-16}"
            [[ "$xterm_duration" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_XTERM_DURATION_SECONDS must be a positive integer." >&2
                exit 1
            }
            [[ "$xterm_width" =~ ^[1-9][0-9]*$
                && "$xterm_height" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_XTERM_WIDTH and SOPHIA_XTERM_HEIGHT must be positive integers." >&2
                exit 1
            }
            [[ "$xterm_lines" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_XTERM_LINES must be a positive integer." >&2
                exit 1
            }
            [[ "$xterm_interval_msec" =~ ^[1-9][0-9]*$
                && "$xterm_interval_msec" -le 1000 ]] || {
                echo "SOPHIA_XTERM_INTERVAL_MSEC must be an integer from 1 through 1000." >&2
                exit 1
            }
            ;;
        *)
            echo "SOPHIA_STANDALONE_WORKLOAD must be glxgears, kitty, vkcube, or xterm." >&2
            exit 1
            ;;
    esac
    standalone_bin="${SOPHIA_STANDALONE_APP_BIN:-$standalone_default_bin}"
    if [[ -z "$standalone_bin" || ! -x "$standalone_bin" ]]; then
        echo "The standalone $standalone_workload proof requires $standalone_requirement; set SOPHIA_STANDALONE_APP_BIN to override it." >&2
        exit 1
    fi
else
    terminal_bin="${SOPHIA_TERMINAL_BIN:-$(command -v kitty || true)}"
    terminal_kind=""
    if [[ "$normal_application_defaults" == true ]]; then
        if [[ -n "$terminal_bin" && ! -x "$terminal_bin" ]]; then
            echo "The default terminal is not executable: $terminal_bin" >&2
            exit 1
        fi
    elif [[ -z "$terminal_bin" || ! -x "$terminal_bin" ]]; then
        echo "The graphical session requires Kitty or xterm; set SOPHIA_TERMINAL_BIN if it is installed elsewhere." >&2
        exit 1
    fi
    if [[ "$normal_application_defaults" != true ]]; then
        terminal_kind="$(
            sophia_resolve_session_terminal_kind \
                "$terminal_bin" "${SOPHIA_TERMINAL_KIND:-}"
        )"
    fi
    if [[ "$FIREFOX_M10_ANY_PROOF" == true && "$terminal_kind" != kitty ]]; then
        echo "The Firefox proof profiles require the Kitty terminal adapter." >&2
        exit 1
    fi
    if [[ "$TRUECOLOR_PROOF" == true && "$terminal_kind" != kitty ]]; then
        echo "The TrueColor proof requires the Kitty terminal adapter." >&2
        exit 1
    fi
fi
# Prepare private proof inputs before asking the installed binary for the
# argument vector. Preparation never enters session run or acquires devices.
if [[ "$SESSION_PROFILE" == standalone ]]; then
    if [[ "${SOPHIA_ENABLE_DIRECT_SCANOUT:-0}" == 1 ]]; then
        install -m 600 "$ROOT_DIR/tools/fixtures/direct_scanout_core.kdl" \
            "$STATE_DIR/standalone-core.kdl"
        install -m 600 "$ROOT_DIR/tools/fixtures/direct_scanout_desktop.kdl" \
            "$STATE_DIR/standalone-desktop.kdl"
        if [[ "$standalone_workload" == vkcube ]]; then
            : "${SOPHIA_STANDALONE_FRAME_COUNT:=600}"
            : "${SOPHIA_STANDALONE_WIDTH:=2560}"
            : "${SOPHIA_STANDALONE_HEIGHT:=1440}"
        fi
    fi
    if [[ "$standalone_workload" == kitty ]]; then
        # Ask Kitty's own parser before graphics takeover. These are the same
        # overrides whose complete vector is checked against Rust in tests.
        kitty_override_check=(
            'linux_display_server x11' 'background_opacity 1'
            'remember_window_size no'
            "initial_window_width ${SOPHIA_STANDALONE_WIDTH:-2560}"
            "initial_window_height ${SOPHIA_STANDALONE_HEIGHT:-1440}"
            'confirm_os_window_close 0'
        )
        if ! timeout --kill-after=2s 10s "$standalone_bin" +runpy 'import sys
from kitty.config import parse_config
for spec in sys.argv[1:]:
    parse_config([spec])
' "${kitty_override_check[@]}" >/dev/null 2>"$STATE_DIR/kitty-override-check.log"; then
            echo "Kitty refused one of the probe overrides or exceeded its deadline." >&2
            cat "$STATE_DIR/kitty-override-check.log" >&2
            exit 1
        fi
    fi
    standalone_width="${SOPHIA_STANDALONE_WIDTH:-500}"
    standalone_height="${SOPHIA_STANDALONE_HEIGHT:-500}"
    standalone_present_mode="${SOPHIA_STANDALONE_PRESENT_MODE:-2}"
fi
prepared_arguments="$STATE_DIR/session-arguments.bin"
if ! "$SOPHIA_BIN" session prepare-arguments \
    "--profile=$SESSION_PROFILE" "--root=$ROOT_DIR" "--state-dir=$STATE_DIR" \
    "--binary=$SOPHIA_BIN" "--terminal=$terminal_bin" \
    "--terminal-kind=${terminal_kind:-}" "--browser=$hagia_browser_bin" \
    "--standalone=$standalone_bin" "--wm=$SOPHIA_HAGIA_BIN" \
    "--firefox-profile=$firefox_m10_profile_dir" -- "$@" >"$prepared_arguments"; then
    echo "The installed binary refused session argument preparation." >&2
    exit 1
fi
chmod 600 "$prepared_arguments"
mapfile -d '' -t prepared_vector <"$prepared_arguments"
rm -f "$prepared_arguments"
if [[ "${prepared_vector[0]:-}" != 'sophia_session_arguments schema=1 status=prepared' ]]; then
    echo "This binary does not support session argument preparation; rebuild it." >&2
    exit 1
fi
session_args=("${prepared_vector[@]:1}")
prepared_environment="$STATE_DIR/session-environment.bin"
if ! "$SOPHIA_BIN" session prepare-environment \
    "--tty=$tty_name" "--firefox-probe=$firefox_m10_probe_dir" -- "$@" \
    >"$prepared_environment"; then
    echo "The installed binary refused session environment preparation." >&2
    exit 1
fi
chmod 600 "$prepared_environment"
mapfile -d '' -t prepared_vector <"$prepared_environment"
rm -f "$prepared_environment"
if [[ "${prepared_vector[0]:-}" != 'sophia_session_environment schema=1 status=prepared' ]]; then
    echo "This binary does not support session environment preparation; rebuild it." >&2
    exit 1
fi
session_bus_mode="${prepared_vector[1]:-}"
session_environment=("${prepared_vector[@]:2}")
session_bus_launcher=()
case "$session_bus_mode" in
    session_scoped) session_bus_launcher=(dbus-run-session --) ;;
    inherited|isolated|unavailable) ;;
    *) echo "Invalid prepared session bus mode." >&2; exit 1 ;;
esac
# A run that quietly chooses a different bus than the operator expected
# measures, and debugs, the wrong thing -- the same reason the flag check
# above refuses a dropped flag.
printf 'sophia_session_bus schema=1 mode=%s\n' "$session_bus_mode" >>"$SESSION_LOG"

session_command=(
    env
    -u WAYLAND_DISPLAY
    -u WAYLAND_SOCKET
    "${session_environment[@]}"
    "$SOPHIA_BIN"
    "${session_args[@]}"
)
# Validation asks the binary whether it would accept these arguments; it opens
# no display and needs no bus, so it runs unwrapped rather than starting a
# daemon to answer a question about argument parsing.
session_launch=(
    ${session_bus_launcher[@]+"${session_bus_launcher[@]}"}
    "${session_command[@]}"
)

# Ask the session whether it would accept this exact command.
#
# The command itself, not a reconstruction of it: the first attempt rebuilt the
# vector without the environment the session runs under and was refused for
# missing a variable that was always going to be there. Validating anything but
# what actually runs answers a question nobody asked.
#
# The accepted record must appear, not merely a zero exit. A binary from before
# the flag existed ignores it and starts a real session instead -- which here,
# with the display manager already down, means taking DRM at the validation
# step. A validation that might not be one is worse than none.
if ! "${session_command[@]}" --validate-session-args \
    >"$STATE_DIR/session-args-check.log" 2>"$STATE_DIR/session-args-check.err"; then
    echo "The assembled session arguments would be refused:" >&2
    cat "$STATE_DIR/session-args-check.err" >&2
    exit 1
fi
if ! grep -q '^sophia_live_session_args schema=1 status=accepted' \
    "$STATE_DIR/session-args-check.log"; then
    echo "This sophia binary does not support --validate-session-args; rebuild it." >&2
    exit 1
fi
if [[ "$SESSION_PROFILE" == standalone
    && "$standalone_workload" == vkcube
    && -n "${SOPHIA_STANDALONE_FRAME_COUNT:-}" ]]; then
    printf 'sophia_rendering_benchmark schema=1 workload=vkcube-xcb requested_frames=%s surface_width=%s surface_height=%s vulkan_present_mode=%s\n' \
        "$SOPHIA_STANDALONE_FRAME_COUNT" "$standalone_width" "$standalone_height" \
        "$standalone_present_mode" >>"$SESSION_LOG"
elif [[ "$SESSION_PROFILE" == standalone
    && "$standalone_workload" == glxgears ]]; then
    printf 'sophia_glxgears_benchmark schema=1 duration_seconds=%s surface_width=%s surface_height=%s swap_interval=1\n' \
        "$glxgears_duration" "$glxgears_width" "$glxgears_height" >>"$SESSION_LOG"
elif [[ "$SESSION_PROFILE" == standalone
    && "$standalone_workload" == xterm ]]; then
    printf 'sophia_terminal_benchmark schema=2 workload=xterm-cpu duration_seconds=%s surface_width=%s surface_height=%s lines_per_iteration=%s interval_msec=%s\n' \
        "$xterm_duration" "$xterm_width" "$xterm_height" \
        "$xterm_lines" "$xterm_interval_msec" >>"$SESSION_LOG"
fi
# Preparation can outlive guard readiness. Never take over after recovery was
# requested or after the independent reader failed while we were preparing.
if [[ -s "$GUARD_TRIGGERED_FILE" ]]; then
    echo "Emergency recovery requested before graphics takeover."
    exit 130
fi
if ! kill -0 "$guard_pid" 2>/dev/null; then
    echo "Input guard exited before graphics takeover; see $GUARD_LOG" >&2
    exit 1
fi
lifecycle_current_phase=graphics_takeover
python3 "$TTY_MODE_HELPER" graphics
python3 "$TTY_MODE_HELPER" keyboard-off
stty raw -echo
lifecycle_phase entering graphics_takeover
if [[ -n "$UNTRUSTED_OUTPUT_LOG" ]]; then
    # This opt-in file is outside the structured recorder and may contain
    # arbitrary child text. Native investigation gates keep it private and
    # never treat it as sanitized acceptance evidence.
    setsid "${session_launch[@]}" >"$UNTRUSTED_OUTPUT_LOG" 2>&1 &
elif [[ -n "${SOPHIA_DIAGNOSTIC_DIR:-}" || "${SOPHIA_DIAGNOSTICS_DISABLED:-false}" == true ]]; then
    # Only Sophia's approved evidence callback enters daily history. Arbitrary
    # application stdout/stderr must not become a metadata-disclosure channel.
    setsid "${session_launch[@]}" >/dev/null 2>&1 &
else
    setsid "${session_launch[@]}" > >(tee -a "$SESSION_LOG") 2>&1 &
fi
session_pid=$!
if [[ -n "$SESSION_WATCHDOG_SECONDS" ]]; then
    (
        sleep "$SESSION_WATCHDOG_SECONDS"
        if kill -0 "$session_pid" 2>/dev/null; then
            printf 'sophia_session_watchdog schema=1 result=deadline_exceeded deadline_seconds=%s session_pid=%s action=terminate_process_group\n' \
                "$SESSION_WATCHDOG_SECONDS" "$session_pid" >>"$SESSION_LOG"
            printf 'deadline_exceeded\n' >"$WATCHDOG_TRIGGERED_FILE"
            kill -TERM -- "-$session_pid" 2>/dev/null || true
            sleep 2
            kill -KILL -- "-$session_pid" 2>/dev/null || true
        fi
    ) &
    watchdog_pid=$!
    echo "Independent session watchdog armed for ${SESSION_WATCHDOG_SECONDS} seconds."
fi
lifecycle_phase complete graphics_takeover
lifecycle_current_phase=session
lifecycle_phase entering session
set +e
wait_targets=("$session_pid" "$guard_pid")
[[ -z "$watchdog_pid" ]] || wait_targets+=("$watchdog_pid")
wait -n "${wait_targets[@]}"
status=$?
set -e
if [[ -s "$WATCHDOG_TRIGGERED_FILE" ]]; then
    echo "Session deadline exceeded; automatic recovery requested." >&2
    emergency_session_shutdown=watchdog_term
    exit 124
fi
if [[ -s "$GUARD_TRIGGERED_FILE" ]]; then
    echo "Emergency recovery requested."
    emergency_session_shutdown=fallback_term
    for _ in {1..100}; do
        session_state="$(ps -o stat= -p "$session_pid" 2>/dev/null || true)"
        if [[ -z "$session_state" || "$session_state" == Z* ]]; then
            set +e
            wait "$session_pid"
            emergency_session_exit_status=$?
            set -e
            session_pid=""
            emergency_session_shutdown=graceful
            break
        fi
        sleep 0.05
    done
    exit 130
fi
if ! kill -0 "$session_pid" 2>/dev/null; then
    set +e
    wait "$session_pid"
    status=$?
    set -e
    session_pid=""
else
    echo "Input guard exited unexpectedly; see $GUARD_LOG" >&2
    status=1
fi
exit "$status"

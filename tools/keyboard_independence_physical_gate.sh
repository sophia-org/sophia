#!/usr/bin/env bash
# Two physical keyboards on one seat, attended: a key held on one is not
# released by the other, an unplug releases exactly what the unplugged
# keyboard held, a replug is a new identity, and the emergency chord cannot be
# assembled across the two. The evidence is the session's own device records
# and two runs of the input guard; the verifier reads all three logs.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
kitty_bin="${SOPHIA_TERMINAL_BIN:-$(command -v kitty || true)}"
sophia_bin="${SOPHIA_BIN:-$ROOT_DIR/target/release/sophia}"
seat="${SOPHIA_KEYBOARD_INDEPENDENCE_SEAT:-}"
keyboard_a="${SOPHIA_KEYBOARD_A:-}"
keyboard_b="${SOPHIA_KEYBOARD_B:-}"
display="${SOPHIA_KEYBOARD_INDEPENDENCE_DISPLAY:-:296}"
runtime_msec="${SOPHIA_KEYBOARD_INDEPENDENCE_RUNTIME_MSEC:-660000}"
sequence_timeout_msec="${SOPHIA_KEYBOARD_INDEPENDENCE_SEQUENCE_TIMEOUT_MSEC:-600000}"
evidence_dir="${SOPHIA_KEYBOARD_INDEPENDENCE_EVIDENCE_DIR:-/tmp/sophia-keyboard-independence-physical}"
proof_text="${SOPHIA_KEYBOARD_INDEPENDENCE_TEXT:-twokeyboards}"
guide="${SOPHIA_KEYBOARD_INDEPENDENCE_GUIDE:-$ROOT_DIR/tools/fixtures/keyboard_independence_guide.sh}"
source_commit="${SOPHIA_KEYBOARD_INDEPENDENCE_SOURCE_COMMIT:-}"
recorded_sophia_sha256="${SOPHIA_KEYBOARD_INDEPENDENCE_SOPHIA_SHA256:-}"
guard_wait_ticks=600

refuse() {
    echo "$*" >&2
    exit 2
}

list_keyboards() {
    echo "available keyboard paths:" >&2
    find /dev/input/by-id /dev/input/by-path -maxdepth 1 -type l -name '*-event-kbd' -print 2>/dev/null >&2 || true
}

if [[ ! "$proof_text" =~ ^[a-z]{1,24}$ ]]; then
    refuse "SOPHIA_KEYBOARD_INDEPENDENCE_TEXT must contain 1-24 lowercase ASCII letters"
fi
if [[ "${SOPHIA_KEYBOARD_INDEPENDENCE_ARM:-0}" != "1" ]]; then
    refuse "set SOPHIA_KEYBOARD_INDEPENDENCE_ARM=1 to acknowledge exclusive DRM/input use"
fi
if [[ -z "$seat" ]]; then
    refuse "set SOPHIA_KEYBOARD_INDEPENDENCE_SEAT to the libinput seat (normally seat0)"
fi
if [[ -z "$keyboard_a" || -z "$keyboard_b" ]]; then
    list_keyboards
    refuse "set SOPHIA_KEYBOARD_A (the keyboard you will unplug) and SOPHIA_KEYBOARD_B to absolute ...-event-kbd paths"
fi
for keyboard in "$keyboard_a" "$keyboard_b"; do
    if [[ "$keyboard" != /* || ! -e "$keyboard" ]]; then
        list_keyboards
        refuse "keyboard path must be absolute and present: $keyboard"
    fi
    if [[ ! -r "$keyboard" || ! -w "$keyboard" ]]; then
        refuse "keyboard node is not readable and writable by this user: $keyboard"
    fi
done
if [[ "$(readlink -f "$keyboard_a")" == "$(readlink -f "$keyboard_b")" ]]; then
    refuse "SOPHIA_KEYBOARD_A and SOPHIA_KEYBOARD_B resolve to the same device"
fi
if [[ -z "$kitty_bin" || ! -x "$kitty_bin" ]]; then
    refuse "set SOPHIA_TERMINAL_BIN to real Kitty"
fi
if [[ ! -x "$guide" ]]; then
    refuse "set SOPHIA_KEYBOARD_INDEPENDENCE_GUIDE to the executable proof guide"
fi
if [[ ! "$source_commit" =~ ^[0-9a-f]{40}$ || ! "$recorded_sophia_sha256" =~ ^[0-9a-f]{64}$ ]]; then
    refuse "run tools/run_keyboard_independence_gate_tty4.sh to bind the signed commit and the binary identity"
fi
if [[ ! "$runtime_msec" =~ ^[0-9]+$ ]] || (( runtime_msec < 30000 )); then
    refuse "SOPHIA_KEYBOARD_INDEPENDENCE_RUNTIME_MSEC must be at least 30000"
fi
if [[ ! "$sequence_timeout_msec" =~ ^[0-9]+$ ]] \
    || (( sequence_timeout_msec < 1000 || sequence_timeout_msec > 600000 )); then
    refuse "SOPHIA_KEYBOARD_INDEPENDENCE_SEQUENCE_TIMEOUT_MSEC must be 1000-600000"
fi
# A key remapper merges every keyboard into one virtual device, which is the
# exact thing this gate exists to tell apart.
if pgrep -x keyd >/dev/null 2>&1; then
    refuse "keyd is running; stop it (sudo sv down keyd) so the seat shows two real keyboards"
fi
# The trusted listener binds under the runtime directory, so a tty login
# without one fails inside the session. Fail here instead, before DRM.
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
if [[ ! -d "$XDG_RUNTIME_DIR" || ! -O "$XDG_RUNTIME_DIR" ]]; then
    refuse "XDG_RUNTIME_DIR must name a directory this user owns: $XDG_RUNTIME_DIR"
fi

verify_bound_identity() {
    if [[ -n "$(git -C "$ROOT_DIR" status --short)" \
        || "$(git -C "$ROOT_DIR" rev-parse HEAD)" != "$source_commit" ]]; then
        echo "Sophia source identity changed during the physical proof." >&2
        exit 1
    fi
    git -C "$ROOT_DIR" verify-commit "$source_commit" >/dev/null 2>&1 || {
        echo "Sophia physical-proof commit does not have a valid signature." >&2
        exit 1
    }
    sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
    if [[ "$sophia_sha256" != "$recorded_sophia_sha256" ]]; then
        echo "Sophia does not match its bound physical-proof identity." >&2
        exit 1
    fi
}

verify_bound_identity

rm -rf -- "$evidence_dir"
install -d -m 700 "$evidence_dir"
state_dir="$evidence_dir/state"
install -d -m 700 "$state_dir"
seat_log="$evidence_dir/guard_seat.log"
pinned_log="$evidence_dir/guard_pinned.log"
session_log="$evidence_dir/session.log"

wait_for_file() {
    local file="$1" ticks="$2"
    while (( ticks > 0 )); do
        [[ ! -s "$file" ]] || return 0
        ticks=$((ticks - 1))
        sleep 0.1
    done
    return 1
}

wait_for_line() {
    local file="$1" pattern="$2" ticks="$3"
    while (( ticks > 0 )); do
        ! grep -Eq "$pattern" "$file" 2>/dev/null || return 0
        ticks=$((ticks - 1))
        sleep 0.1
    done
    return 1
}

# One guard run: the split chord must not arm, the whole chord on A arms,
# the whole chord on A again triggers and ends the guard.
guard_phase() {
    local phase="$1" log="$2" split_instruction="$3"
    shift 3
    local armed="$state_dir/$phase.armed" triggered="$state_dir/$phase.triggered" guard_pid
    rm -f "$armed" "$triggered"
    : >"$log"
    "$sophia_bin" session input-guard "$@" \
        --arming=manual \
        "--armed-file=$armed" \
        "--triggered-file=$triggered" \
        "--owner-pid=$$" >>"$log" 2>&1 &
    guard_pid=$!
    if ! wait_for_line "$log" '^sophia_session_input_guard schema=2 status=ready ' 100; then
        kill "$guard_pid" 2>/dev/null || true
        echo "the $phase guard never opened its keyboards; see $log" >&2
        exit 1
    fi
    echo
    echo "Keyboard independence, $phase guard"
    echo "  1. $split_instruction"
    echo "     Then release everything and wait three seconds. Nothing must happen."
    sleep 3
    if [[ -s "$armed" ]]; then
        kill "$guard_pid" 2>/dev/null || true
        echo "the $phase guard armed from a chord split across two keyboards" >&2
        exit 1
    fi
    printf 'sophia_keyboard_independence_guard schema=1 status=split_chord_ignored phase=%s\n' "$phase" >>"$log"
    echo "  2. Press and release Ctrl+Alt+Backspace on keyboard A alone."
    if ! wait_for_file "$armed" "$guard_wait_ticks"; then
        kill "$guard_pid" 2>/dev/null || true
        echo "the $phase guard did not arm from keyboard A" >&2
        exit 1
    fi
    echo "  3. Press Ctrl+Alt+Backspace on keyboard A once more."
    if ! wait_for_file "$triggered" "$guard_wait_ticks"; then
        kill "$guard_pid" 2>/dev/null || true
        echo "the $phase guard did not trigger from keyboard A" >&2
        exit 1
    fi
    wait "$guard_pid" || {
        echo "the $phase guard exited with a failure; see $log" >&2
        exit 1
    }
}

echo "Keyboard independence physical gate"
echo "This takes exclusive seat input, then exclusive DRM/KMS. Evidence: $evidence_dir"
echo "Keyboard A is the one you will unplug and replug. Keyboard B stays."
echo "Press ONLY the keys each step names."

guard_phase seat "$seat_log" \
    "Hold Ctrl+Alt on keyboard A and press Backspace on keyboard B." \
    "--input-seat=$seat"
guard_phase pinned "$pinned_log" \
    "Press and release Ctrl+Alt+Backspace on keyboard B." \
    "--input-devices=$keyboard_a"

echo
echo "Both guards passed. Starting the session; follow the guide on screen."

SOPHIA_LIVE_SESSION_DISPLAY="$display" \
SOPHIA_LIVE_SESSION_RUNTIME_MSEC="$runtime_msec" \
SOPHIA_LIVE_SESSION_PERSISTENT_EVIDENCE="$session_log" \
SOPHIA_LIVE_SESSION_VERIFY_MODE=caller \
SOPHIA_KEYBOARD_INDEPENDENCE_TEXT="$proof_text" \
    "$ROOT_DIR/tools/live_session_persistent_hardware_proof.sh" \
    --no-config \
    --session-mode=normal \
    "--session-app=terminal=$kitty_bin" \
    --session-start=terminal \
    --session-action-app=terminal=terminal \
    --session-app-arg=terminal=--config \
    --session-app-arg=terminal=NONE \
    --session-app-arg=terminal=--override \
    --session-app-arg=terminal=linux_display_server=x11 \
    --session-app-arg=terminal=--override \
    --session-app-arg=terminal=remember_window_size=no \
    "--session-app-arg=terminal=$guide" \
    "--input-seat=$seat" \
    "--expect-physical-text=$proof_text" \
    "--physical-sequence-timeout-ms=$sequence_timeout_msec" \
    --exit-after-input-proof

verify_bound_identity
printf 'sophia_keyboard_independence_identity schema=1 status=bound sophia_commit=%s sophia_sha256=%s\n' \
    "$source_commit" "$sophia_sha256" | tee -a "$session_log"

"$ROOT_DIR/tools/verify_keyboard_independence_physical.sh" "$evidence_dir" "$proof_text"
SOPHIA_KEYBOARD_INDEPENDENCE_SOPHIA_BIN="$sophia_bin" \
    "$ROOT_DIR/tools/archive_keyboard_independence_physical_run.sh" "$evidence_dir" "$proof_text"
echo "Keyboard independence physical gate passed"

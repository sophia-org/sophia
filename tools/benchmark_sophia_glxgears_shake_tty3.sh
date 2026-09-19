#!/usr/bin/env bash
# The glxgears benchmark under a synthetic pointer shake.
#
# The benchmark's rule -- cadence under pointer motion holds at least 55 FPS
# with a p95 of at most 25 ms -- has only ever been exercised by a hand on the
# mouse, which is neither repeatable nor present on an unattended run. This
# creates a virtual mouse before the session opens its seat, so udev
# enumerates it beside the physical devices, and drives it at a fixed report
# rate from the moment the client has focus until the bounded run ends.
#
# Run from tty3 exactly like tools/benchmark_sophia_glxgears_tty3.sh; every
# variable that script honours passes through. Keep hands off the mouse.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

SHAKE_HZ="${SOPHIA_GLXGEARS_SHAKE_HZ:-1000}"
SHAKE_AMPLITUDE="${SOPHIA_GLXGEARS_SHAKE_AMPLITUDE:-8}"
DURATION_SECONDS="${SOPHIA_GLXGEARS_DURATION_SECONDS:-20}"
# The shake outlives the bounded client so an early trigger cannot leave an
# unshaken tail in the mean; the session's end stops it.
SHAKE_MARGIN_SECONDS=15
# The client's own GL_RENDERER line reaches session.log (its stdout is teed
# there); the session's focus_applied record does not -- it is diverted to
# the reduced per-session evidence log. GL_RENDERER means the client has a
# context and is about to draw, which is when the shake should begin.
TRIGGER_RECORD='GL_RENDERER'
STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
LOG_DIR="${SOPHIA_DIAGNOSTIC_DIR:-$STATE_HOME/sophia/standalone-session}"
SESSION_LOG="$LOG_DIR/session.log"
SHAKE_LOG="$LOG_DIR/shake.log"
INJECTOR_EVIDENCE="$LOG_DIR/shake-injector.log"

fail() {
    echo "Sophia glxgears shake benchmark failed: $*" >&2
    exit 1
}

[[ "$SHAKE_HZ" =~ ^[1-9][0-9]*$ ]] ||
    fail "SOPHIA_GLXGEARS_SHAKE_HZ must be a positive integer"
[[ "$SHAKE_AMPLITUDE" =~ ^[1-9][0-9]*$ ]] ||
    fail "SOPHIA_GLXGEARS_SHAKE_AMPLITUDE must be a positive integer"
[[ "$DURATION_SECONDS" =~ ^[1-9][0-9]*$ ]] ||
    fail "SOPHIA_GLXGEARS_DURATION_SECONDS must be a positive integer"
[[ -w /dev/uinput ]] ||
    fail "/dev/uinput is not writable (run tools/setup_sophia_uinput.sh, then start a fresh login or run newgrp input)"

SHAKE_SECONDS=$((DURATION_SECONDS + SHAKE_MARGIN_SECONDS))
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/sophia-glxgears-shake.XXXXXX")"
READY_FILE="$SCRATCH/device"
TRIGGER_FILE="$SCRATCH/shake"
RESULT_FILE="$SCRATCH/shaken-at-usec"
INJECTOR_LOG="$SCRATCH/injector.log"
TRIGGER_LOG="$SCRATCH/trigger.log"
INJECTOR_PID=""
WATCHER_PID=""

field() {
    local line="$1" key="$2" token
    for token in $line; do
        if [[ "$token" == "$key="* ]]; then
            printf '%s\n' "${token#*=}"
            return 0
        fi
    done
    printf 'none\n'
}

# The evidence goes to a file beside the session log: a TTY run is read from
# disk afterwards, not from the console.
summarize() {
    local benchmark_status="$1" injected trigger client cadence status
    injected="$(
        grep -E '^sophia_uinput schema=1 status=injected mode=shake ' \
            "$INJECTOR_LOG" 2>/dev/null | tail -n 1 || true
    )"
    trigger="$(cat "$TRIGGER_LOG" 2>/dev/null || echo none)"
    client="$(
        grep -E '^sophia_glxgears_client schema=1 status=complete ' \
            "$SESSION_LOG" 2>/dev/null | tail -n 1 || true
    )"
    cadence="$(
        grep -E '^sophia_live_present_cadence schema=1 status=complete ' \
            "$SESSION_LOG" 2>/dev/null | tail -n 1 || true
    )"
    status=fail
    if ((benchmark_status == 0)) && [[ -n "$injected" && "$trigger" == renderer ]]; then
        status=pass
    fi
    mkdir -p "$LOG_DIR"
    cp -f "$INJECTOR_LOG" "$INJECTOR_EVIDENCE" 2>/dev/null || true
    printf 'sophia_glxgears_shake schema=1 status=%s benchmark_status=%s trigger=%s rate_hz=%s amplitude=%s seconds=%s events=%s achieved_hz=%s client_mean_fps=%s present_fps=%s p95_frame_msec=%s\n' \
        "$status" "$benchmark_status" "$trigger" "$SHAKE_HZ" "$SHAKE_AMPLITUDE" \
        "$SHAKE_SECONDS" "$(field "$injected" events)" \
        "$(field "$injected" achieved_hz)" "$(field "$client" mean_fps)" \
        "$(field "$cadence" mean_fps)" "$(field "$cadence" p95_frame_msec)" |
        tee -a "$SHAKE_LOG"
}

cleanup() {
    local status=$?
    [[ -z "$WATCHER_PID" ]] || kill "$WATCHER_PID" 2>/dev/null || true
    if [[ -n "$INJECTOR_PID" ]]; then
        kill "$INJECTOR_PID" 2>/dev/null || true
        wait "$INJECTOR_PID" 2>/dev/null || true
    fi
    summarize "$status"
    rm -rf "$SCRATCH"
    exit "$status"
}
trap cleanup EXIT

# The runner rotates session.log before the session starts, so the previous
# run's records must not stand in for this one's: wait for a new inode, then
# for the client to hold focus.
watch_for_trigger() {
    local previous_inode="$1" current deadline
    deadline=$((SECONDS + 900))
    while :; do
        current="$(stat -c %i "$SESSION_LOG" 2>/dev/null || echo none)"
        [[ "$current" == none || "$current" == "$previous_inode" ]] || break
        if ((SECONDS >= deadline)); then
            echo no_session_log >"$TRIGGER_LOG"
            return 0
        fi
        sleep 0.1
    done
    deadline=$((SECONDS + 180))
    while ! grep -Fq "$TRIGGER_RECORD" "$SESSION_LOG" 2>/dev/null; do
        if ((SECONDS >= deadline)); then
            echo deadline >"$TRIGGER_LOG"
            : >"$TRIGGER_FILE"
            return 0
        fi
        sleep 0.01
    done
    echo renderer >"$TRIGGER_LOG"
    : >"$TRIGGER_FILE"
}

tools/probes/uinput_text_injector.py \
    "--shake-hz=$SHAKE_HZ" "--shake-seconds=$SHAKE_SECONDS" \
    "--shake-amplitude=$SHAKE_AMPLITUDE" --self-test

previous_inode="$(stat -c %i "$SESSION_LOG" 2>/dev/null || echo none)"
tools/probes/uinput_text_injector.py \
    "--shake-hz=$SHAKE_HZ" "--shake-seconds=$SHAKE_SECONDS" \
    "--shake-amplitude=$SHAKE_AMPLITUDE" \
    --ready-file="$READY_FILE" \
    --trigger-file="$TRIGGER_FILE" \
    --result-file="$RESULT_FILE" \
    --timeout-seconds=900 \
    >"$INJECTOR_LOG" 2>&1 &
INJECTOR_PID=$!
ready_deadline=$((SECONDS + 5))
while [[ ! -s "$READY_FILE" && $SECONDS -lt $ready_deadline ]]; do
    kill -0 "$INJECTOR_PID" 2>/dev/null ||
        fail "the virtual mouse exited before it became ready; see $INJECTOR_LOG"
    sleep 0.01
done
[[ -s "$READY_FILE" ]] || fail "the virtual mouse did not become ready"
input_device="$(<"$READY_FILE")"
[[ "$input_device" == /dev/input/event* && -e "$input_device" ]] ||
    fail "the virtual mouse published an invalid input device: $input_device"

watch_for_trigger "$previous_inode" &
WATCHER_PID=$!

printf '%s\n' \
    "Virtual mouse ready at $input_device." \
    "It shakes at ${SHAKE_HZ} Hz, ${SHAKE_AMPLITUDE} px, once the client starts rendering; keep hands off the mouse." \
    "The shake summary lands in $SHAKE_LOG."
tools/benchmark_sophia_glxgears_tty3.sh "$@"

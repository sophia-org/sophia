#!/usr/bin/env bash
set -euo pipefail

# The native Hagia session gate. It proves the bounded product workflow --
# three terminal launches, a visible focus-next, one close, and a normal logout
# -- across Sophia's own WM and shell protocols. A passing run promotes the
# three native frame slots.
#
# Unlike the switcher gate, this one runs the session through the ordinary
# `hagia` runner profile rather than launching `sophia session run` itself.
# That runner already owns TTY mode save/restore, keyd, the Ctrl-Alt-Backspace
# input guard, and the `sophia_tty_recovery` record. Exact TTY recovery is one
# of this gate's exit criteria, so the gate uses the component that produces it
# instead of standing up a second session lifecycle beside it.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/lib/proof_checkout.sh
source "$ROOT_DIR/tools/lib/proof_checkout.sh"
# shellcheck source=tools/lib/reference_capture.sh
source "$ROOT_DIR/tools/lib/reference_capture.sh"
hagia_bin="${SOPHIA_HAGIA_BIN:-$(command -v hagia || true)}"
hagia_shell_bin="${SOPHIA_HAGIA_SHELL_BIN:-$(command -v narthex || true)}"
kitty_bin="${SOPHIA_TERMINAL_BIN:-$(command -v kitty || true)}"
seat="${SOPHIA_HAGIA_NATIVE_SEAT:-}"
display="${SOPHIA_HAGIA_NATIVE_DISPLAY:-:292}"
sequence_timeout_msec="${SOPHIA_HAGIA_NATIVE_SEQUENCE_TIMEOUT_MSEC:-600000}"
# A startup budget, not a session lifetime. With an input proof requested the
# global runtime deadline deliberately does not end the session
# (`global_runtime_deadline_ends_session`); it bounds the wait for the first
# focused terminal frame the proof types into, and narrower stage deadlines own
# everything after. The session still ends by the operator's normal logout, so
# this value covers startup rather than the whole workflow.
startup_budget_msec="${SOPHIA_HAGIA_NATIVE_STARTUP_BUDGET_MSEC:-660000}"
# The native workflow proof by default. SOPHIA_HAGIA_NATIVE_CAPTURE=reference
# runs the same session to retain a t018 tab reference capture instead: it
# claims only bound identity and profile, exit 0, TTY recovery and retained
# evidence, never the native workflow or any tab observation.
capture_mode="${SOPHIA_HAGIA_NATIVE_CAPTURE:-}"
case "$capture_mode" in
    ''|reference) ;;
    *)
        echo "SOPHIA_HAGIA_NATIVE_CAPTURE must be unset or reference" >&2
        exit 2
        ;;
esac
if [[ "$capture_mode" == reference ]]; then
    evidence="${SOPHIA_HAGIA_NATIVE_EVIDENCE:-/tmp/sophia-hagia-reference-capture.log}"
    default_guide="$ROOT_DIR/tools/fixtures/hagia_reference_capture_guide.sh"
else
    evidence="${SOPHIA_HAGIA_NATIVE_EVIDENCE:-/tmp/sophia-hagia-native-session.log}"
    default_guide="$ROOT_DIR/tools/fixtures/hagia_native_session_guide.sh"
fi
proof_text="${SOPHIA_HAGIA_NATIVE_TEXT:-hagianativeproof}"
guide="${SOPHIA_HAGIA_NATIVE_GUIDE:-$default_guide}"
hagia_root="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
narthex_root="${SOPHIA_NARTHEX_ROOT:-$ROOT_DIR/../narthex}"
desktop_profile="${SOPHIA_DESKTOP_PROFILE:-}"
source_commit="${SOPHIA_HAGIA_NATIVE_SOURCE_COMMIT:-}"
hagia_commit="${SOPHIA_HAGIA_NATIVE_HAGIA_COMMIT:-}"
recorded_sophia_sha256="${SOPHIA_HAGIA_NATIVE_SOPHIA_SHA256:-}"
recorded_hagia_sha256="${SOPHIA_HAGIA_NATIVE_HAGIA_SHA256:-}"
recorded_narthex_sha256="${SOPHIA_HAGIA_NATIVE_NARTHEX_SHA256:-}"
narthex_commit="${SOPHIA_HAGIA_NATIVE_NARTHEX_COMMIT:-}"
recorded_profile_sha256="${SOPHIA_DESKTOP_PROFILE_SHA256:-}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
session_log="$state_home/sophia/hagia-session/session.log"
recovery_log="$state_home/sophia/hagia-session/recovery.log"

if [[ ! "$proof_text" =~ ^[a-z]{1,24}$ ]]; then
    echo "SOPHIA_HAGIA_NATIVE_TEXT must contain 1-24 lowercase ASCII letters" >&2
    exit 2
fi
if [[ "${SOPHIA_HAGIA_NATIVE_ARM:-0}" != "1" ]]; then
    echo "set SOPHIA_HAGIA_NATIVE_ARM=1 to acknowledge exclusive DRM/input use" >&2
    exit 2
fi
if [[ -z "$seat" ]]; then
    echo "set SOPHIA_HAGIA_NATIVE_SEAT to the libinput seat (normally seat0)" >&2
    exit 2
fi
if [[ -z "$hagia_bin" || ! -x "$hagia_bin" ]]; then
    echo "set SOPHIA_HAGIA_BIN to a built Hagia executable" >&2
    exit 2
fi
if [[ -z "$hagia_shell_bin" || ! -x "$hagia_shell_bin" ]]; then
    echo "set SOPHIA_HAGIA_SHELL_BIN to a built Narthex executable" >&2
    exit 2
fi
if [[ -z "$kitty_bin" || ! -x "$kitty_bin" ]]; then
    echo "set SOPHIA_TERMINAL_BIN to real Kitty" >&2
    exit 2
fi
if [[ ! -x "$guide" ]]; then
    echo "set SOPHIA_HAGIA_NATIVE_GUIDE to the executable proof guide" >&2
    exit 2
fi
# The profile is passed explicitly rather than left to discovery. A session
# started with --no-config runs the compiled profile while an exported digest
# still names a file, and the resulting identity line describes a profile that
# did not run.
if [[ "$desktop_profile" != /* || ! -f "$desktop_profile" ]]; then
    echo "set SOPHIA_DESKTOP_PROFILE to the absolute Hagia profile this run should load" >&2
    exit 2
fi
if [[ ! "$source_commit" =~ ^[0-9a-f]{40}$ \
    || ! "$hagia_commit" =~ ^[0-9a-f]{40}$ \
    || ! "$recorded_sophia_sha256" =~ ^[0-9a-f]{64}$ \
    || ! "$recorded_hagia_sha256" =~ ^[0-9a-f]{64}$ \
    || ! "$recorded_narthex_sha256" =~ ^[0-9a-f]{64}$ \
    || ! "$narthex_commit" =~ ^[0-9a-f]{40}$ \
    || ! "$recorded_profile_sha256" =~ ^[0-9a-f]{64}$ ]]; then
    echo "run tools/run_current_hagia_native_gate_tty4.sh to bind all signed commits, all three binary identities, and the profile digest" >&2
    exit 2
fi
if [[ "$(sha256sum "$desktop_profile" | awk '{ print $1 }')" != "$recorded_profile_sha256" ]]; then
    echo "The desktop profile does not match its bound digest." >&2
    exit 2
fi
if ! proof_checkout_root "$hagia_root"; then
    echo "Hagia checkout not found at $hagia_root" >&2
    exit 2
fi
if ! proof_checkout_root "$narthex_root"; then
    echo "Narthex checkout not found at $narthex_root" >&2
    exit 2
fi
if [[ ! "$sequence_timeout_msec" =~ ^[0-9]+$ ]] \
    || (( sequence_timeout_msec < 1000 || sequence_timeout_msec > 600000 )); then
    echo "SOPHIA_HAGIA_NATIVE_SEQUENCE_TIMEOUT_MSEC must be 1000-600000" >&2
    exit 2
fi
if [[ ! "$startup_budget_msec" =~ ^[0-9]+$ ]] || (( startup_budget_msec < 30000 )); then
    echo "SOPHIA_HAGIA_NATIVE_STARTUP_BUDGET_MSEC must be at least 30000" >&2
    exit 2
fi
# The runner's Hagia profile registers a browser application even when the
# workflow never launches one. Refusing here names the missing dependency
# before the display manager is stopped rather than after.
browser_bin="${SOPHIA_HAGIA_BROWSER_BIN:-$(command -v helium || command -v firefox || true)}"
if [[ -z "$browser_bin" || ! -x "$browser_bin" ]]; then
    echo "The Hagia profile requires Helium, Firefox, or SOPHIA_HAGIA_BROWSER_BIN." >&2
    exit 2
fi

verify_bound_identity() {
    if [[ -n "$(git -C "$ROOT_DIR" status --short)" \
        || -n "$(git -C "$hagia_root" status --short)" \
        || -n "$(git -C "$narthex_root" status --short)" \
        || "$(git -C "$ROOT_DIR" rev-parse HEAD)" != "$source_commit" \
        || "$(git -C "$hagia_root" rev-parse HEAD)" != "$hagia_commit" \
        || "$(git -C "$narthex_root" rev-parse HEAD)" != "$narthex_commit" ]]; then
        echo "Sophia, Hagia, or Narthex source identity changed during the physical proof." >&2
        exit 1
    fi
    git -C "$ROOT_DIR" verify-commit "$source_commit" >/dev/null 2>&1 || {
        echo "Sophia physical-proof commit does not have a valid signature." >&2
        exit 1
    }
    git -C "$hagia_root" verify-commit "$hagia_commit" >/dev/null 2>&1 || {
        echo "Hagia physical-proof commit does not have a valid signature." >&2
        exit 1
    }
    git -C "$narthex_root" verify-commit "$narthex_commit" >/dev/null 2>&1 || {
        echo "Narthex physical-proof commit does not have a valid signature." >&2
        exit 1
    }
    sophia_sha256="$(sha256sum "$ROOT_DIR/target/release/sophia" | awk '{ print $1 }')"
    hagia_sha256="$(sha256sum "$hagia_bin" | awk '{ print $1 }')"
    narthex_sha256="$(sha256sum "$hagia_shell_bin" | awk '{ print $1 }')"
    if [[ "$sophia_sha256" != "$recorded_sophia_sha256" \
        || "$hagia_sha256" != "$recorded_hagia_sha256" \
        || "$narthex_sha256" != "$recorded_narthex_sha256" ]]; then
        echo "Sophia, Hagia, or Hagia Shell does not match its bound physical-proof identity." >&2
        exit 1
    fi
}

verify_bound_identity

if [[ "$capture_mode" == reference ]]; then
    echo "Hagia t018 reference capture (not a native or tab acceptance)"
    echo "This takes exclusive DRM/KMS and seat input. Evidence: $evidence"
    echo "After the startup terminal appears, follow the on-screen guide:"
    echo "  1. Type '$proof_text' and press Enter."
    echo "  2. Work through the tab matrix the guide lists, recording what you see."
    echo "  3. Press Ctrl+Alt+Delete once for a normal logout."
    echo "Do not use Ctrl+Alt+Backspace during the capture."
else
    echo "Hagia native session gate"
    echo "This takes exclusive DRM/KMS and seat input. Evidence: $evidence"
    echo "After the startup terminal appears, follow the on-screen guide:"
    echo "  1. Type '$proof_text' and press Enter."
    echo "  2. Press Super+Return three times, waiting for each new terminal."
    echo "  3. Press Super+J once and confirm focus visibly moves."
    echo "  4. Press Super+q once to close the focused terminal."
    echo "  5. Press Ctrl+Alt+Delete once for a normal logout."
    echo "Do not use Ctrl+Alt+Backspace during the normal proof."
fi

# The runner skips its own preflight when it is not building, and this gate
# deliberately does not let it build: a rebuild between binding the digests and
# running the session would invalidate the identity the archive rests on. The
# preflight is a debug-profile check that does not touch target/release/sophia,
# so it runs here instead of being lost.
"$ROOT_DIR/tools/atomic_scanout_preflight.sh"

# The startup terminal runs the guide; every terminal the workflow launches must
# not. The two cannot be separate applications: with a physical text proof
# requested, a normal session requires the terminal action to name the single
# startup application, and refuses the proof otherwise. So one application runs
# the guide in every terminal, and the guide itself stands down in all but the
# first -- it claims this file, and an instance that cannot claim it becomes an
# ordinary shell.
guide_claim_dir="$(mktemp -d "${TMPDIR:-/tmp}/sophia-hagia-native-guide.XXXXXX")"
trap 'rm -rf -- "$guide_claim_dir"' EXIT HUP INT TERM
guide_claim="$guide_claim_dir/startup.claim"

# The runner rotates its session and recovery logs once per run. A capture
# binds each to this invocation by requiring exactly that one rotation, so an
# earlier or later session's files can never stand in; prior files are kept.
if [[ "$capture_mode" == reference ]]; then
    reference_capture_root_allowed \
        "${SOPHIA_HAGIA_REFERENCE_RUN_ROOT:-$state_home/sophia/reference/hagia-tab-captures}" \
        "$state_home/sophia/promotion" \
        "${SOPHIA_HAGIA_NATIVE_RUN_ROOT:-$state_home/sophia/promotion/hagia-native-runs}" || {
        echo "The reference run root may not be, contain or sit inside promotion storage." >&2
        exit 2
    }
    recovery_before="$(reference_capture_log_state "$recovery_log")"
    session_before="$(reference_capture_log_state "$session_log")"
fi

status=0
# Shared renderer workers stay opt-in and this run is their promotion: the
# verifier requires schema-10 evidence whose thread count is below the head
# count, with no misrouted result and no output passed over more than once per
# sibling. Buffer-age damage is no longer set here because it is the default;
# the verifier still requires a frame to have rendered partially, so a session
# that lost the path fails rather than passing quietly.
#
# Nothing may interrupt the assignments below. A comment between them would
# continue the backslash into itself and comment out the session launch, which
# `bash -n` accepts without complaint.
SOPHIA_ENABLE_SHARED_RENDERER_WORKER=1 \
SOPHIA_TTY_PROFILE=hagia \
SOPHIA_HAGIA_BIN="$hagia_bin" \
SOPHIA_HAGIA_SHELL_BIN="$hagia_shell_bin" \
SOPHIA_TERMINAL_BIN="$kitty_bin" \
SOPHIA_DESKTOP_PROFILE="$desktop_profile" \
SOPHIA_DESKTOP_PROFILE_SHA256="$recorded_profile_sha256" \
SOPHIA_LIVE_SESSION_DISPLAY="$display" \
SOPHIA_LIVE_SESSION_PERSISTENT_EVIDENCE="$session_log" \
SOPHIA_OPERATOR_INPUT_SEAT="$seat" \
SOPHIA_HAGIA_NATIVE_TEXT="$proof_text" \
SOPHIA_HAGIA_NATIVE_GUIDE_CLAIM="$guide_claim" \
SOPHIA_BUILD_SESSION=false \
    "$ROOT_DIR/tools/start_sophia_tty3.sh" \
    "--shell-process=$hagia_shell_bin" \
    "--session-app-arg=terminal=$guide" \
    "--expect-physical-text=$proof_text" \
    "--physical-sequence-timeout-ms=$sequence_timeout_msec" \
    "--max-runtime-ms=$startup_budget_msec" || status=$?

if (( status != 0 )); then
    echo "The native session did not return cleanly (exit $status); evidence is not archived." >&2
    exit "$status"
fi

[[ -s "$session_log" ]] || {
    echo "The native session produced no evidence: $session_log" >&2
    exit 1
}

verify_bound_identity

# One evidence artifact carries the whole run. The session log, the runner's TTY
# restoration record, and the bound identity live in three places while the
# session is running; the archive keeps one file, so they are joined here rather
# than left for the verifier to correlate across the filesystem.
install -m 600 "$session_log" "$evidence"
if [[ "$capture_mode" == reference ]]; then
    if ! reference_capture_rotated_once "$recovery_log" "$recovery_before" \
        || ! reference_capture_rotated_once "$session_log" "$session_before"; then
        echo "The runner logs were not rotated exactly once by this invocation." >&2
        exit 1
    fi
    # Every recovery record of this invocation's fresh log is carried; the
    # shared validation below requires exactly one, of the exact passing form.
    grep -E '^sophia_tty_recovery ' "$recovery_log" >>"$evidence" || true
else
    recovery_record="$(grep -E '^sophia_tty_recovery schema=3 profile=hagia ' "$recovery_log" | tail -n 1 || true)"
    [[ -n "$recovery_record" ]] || {
        echo "The runner recorded no TTY recovery for this session: $recovery_log" >&2
        exit 1
    }
    printf '%s\n' "$recovery_record" >>"$evidence"
fi
printf 'sophia_hagia_native_identity schema=2 status=bound sophia_commit=%s hagia_commit=%s narthex_commit=%s sophia_sha256=%s hagia_sha256=%s narthex_sha256=%s desktop_profile_sha256=%s\n' \
    "$source_commit" "$hagia_commit" "$narthex_commit" "$sophia_sha256" "$hagia_sha256" \
    "$narthex_sha256" "$recorded_profile_sha256" >>"$evidence"

# A run made by current code records which hardware cursor path it took. The
# verifier cannot require that, because it also reads archives written before
# the record existed and absence there means "older", not "lost". Here absence
# can only mean lost: this gate just built and ran the binary that emits it.
grep -qE '^sophia_live_cursor_path schema=2 status=selected requested=(atomic_plane|legacy_ioctl) path=(atomic_plane|legacy_ioctl)$' \
    "$evidence" || {
    echo "The session recorded no hardware cursor path, which current code always emits" >&2
    exit 1
}

if [[ "$capture_mode" == reference ]]; then
    # The native workflow verifier is never run on a capture: its counts
    # describe a different workflow, and a capture neither passes nor fails it.
    reference_capture_validate_session "$evidence" "$proof_text" || {
        echo "The reference capture is refused; no capture record was written." >&2
        exit 1
    }
    reference_capture_record "$evidence" >>"$evidence"
    SOPHIA_HAGIA_BIN="$hagia_bin" \
    SOPHIA_HAGIA_SHELL_BIN="$hagia_shell_bin" \
    SOPHIA_HAGIA_ROOT="$hagia_root" \
    SOPHIA_NARTHEX_ROOT="$narthex_root" \
        "$ROOT_DIR/tools/archive_hagia_native_session_run.sh" --kind=reference \
        "$evidence" "$proof_text"
    echo "Hagia reference capture retained: native_acceptance=false tab_observations=unverified"
    exit 0
fi
SOPHIA_HAGIA_NATIVE_GUIDE="$guide" \
    "$ROOT_DIR/tools/verify_hagia_native_session.sh" "$evidence" "$proof_text"
SOPHIA_HAGIA_BIN="$hagia_bin" \
SOPHIA_HAGIA_SHELL_BIN="$hagia_shell_bin" \
SOPHIA_HAGIA_ROOT="$hagia_root" \
SOPHIA_HAGIA_NATIVE_GUIDE="$guide" \
    "$ROOT_DIR/tools/archive_hagia_native_session_run.sh" "$evidence" "$proof_text"
echo "Hagia native session gate passed"

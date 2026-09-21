#!/usr/bin/env bash
set -euo pipefail

STATE_HOME="${XDG_STATE_HOME:-${HOME}/.local/state}"
LOG_DIR="${SOPHIA_STANDALONE_LOG_DIR:-$STATE_HOME/sophia/standalone-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"
WAIT_SECONDS="${SOPHIA_VERIFY_WAIT_SECONDS:-5}"

fail() {
    echo "standalone vkcube verification failed: $*" >&2
    exit 1
}

field() {
    local line="$1" key="$2" token
    for token in $line; do
        if [[ "$token" == "$key="* ]]; then
            printf '%s\n' "${token#*=}"
            return 0
        fi
    done
    return 1
}

[[ -s "$SESSION_LOG" ]] || fail "missing session log: $SESSION_LOG"

deadline=$((SECONDS + WAIT_SECONDS))
while ! grep -Eq '^sophia_live_session_cleanup schema=1 status=clean ' "$SESSION_LOG" ||
    ! grep -Eq '^sophia_live_session schema=(15|16|18) status=bounded_complete ' "$SESSION_LOG"; do
    ((SECONDS < deadline)) || fail "session log is incomplete"
    sleep 0.1
done

if grep -Eqi \
    '(^Error:|panicked at|admission_group_(invalid|overflowed)|mismatched.transaction|status=(failed|degraded)([[:space:]]|$))' \
    "$SESSION_LOG"; then
    fail "session contains an error, invalid admission group, or degraded status"
fi

# This profile runs no window manager: `sophia-wm-demo` lost its serving mode
# in 83596bfc, and a single-application proof has nothing to arrange anyway.
# The two checks that stood here asserted a reference policy started and
# committed a layout, which only a WM can produce. What a WM-less session can
# show in their place is that none ran and that exactly one client did.
# Schema-agnostic: these fixtures are schema-15 evidence and must keep
# verifying, the same way the promoted archives do.
grep -Eq '^sophia_live_session schema=[0-9]+ .*(^| )wm_policy=disabled( |$)' "$SESSION_LOG" ||
    fail "a window manager ran; this profile has none to run"
grep -Eq '^sophia_session_app schema=1 status=started id=standalone source=startup$' \
    "$SESSION_LOG" || fail "standalone startup application did not launch"
grep -Eq '^sophia_live_session_startup schema=2 status=ready ' "$SESSION_LOG" ||
    fail "standalone startup did not become ready"

mapfile -t candidates < <(
    grep -E \
        '^sophia_live_visual_candidate schema=1 status=selected transaction=[0-9]+ surface=[0-9]+ width=[0-9]+ height=[0-9]+ evidence=PresentedBuffer$' \
        "$SESSION_LOG"
)
((${#candidates[@]} == 1)) ||
    fail "expected exactly one presented-frame candidate, observed ${#candidates[@]}"

candidate="${candidates[0]}"
transaction="$(field "$candidate" transaction)" || fail "candidate lacks a transaction"
surface="$(field "$candidate" surface)" || fail "candidate lacks a surface"
width="$(field "$candidate" width)" || fail "visual candidate lacks width"
height="$(field "$candidate" height)" || fail "visual candidate lacks height"
((width > 0 && height > 0)) || fail "visual candidate extent is empty"

completion="$(
    grep -E '^sophia_live_session schema=(15|16|18) status=bounded_complete ' "$SESSION_LOG" |
        tail -n 1
)"
mapfile -t armed < <(
    grep -E \
        "^sophia_live_visual_admission schema=1 status=armed transaction=${transaction} surface=${surface}$" \
        "$SESSION_LOG"
)
if ((${#armed[@]} == 1)); then
    grep -Eq \
        "^sophia_live_visual_admission schema=1 status=presented transaction=${transaction} surface=${surface}$" \
        "$SESSION_LOG" || fail "DMA-BUF visual admission never completed"
    grep -Eq \
        "^sophia_live_session_present schema=2 status=retired transaction=${transaction} surface=${surface} " \
        "$SESSION_LOG" || fail "the selected DMA-BUF Present has no matching page-flip retirement"
    grep -Eq \
        '^sophia_live_session_startup schema=2 status=content_ready source=stable_present_scanout nonzero_rgb_pixels=[1-9][0-9]*$' \
        "$SESSION_LOG" || fail "DMA-BUF Present never produced visible nonzero pixels"
elif ((${#armed[@]} == 0)); then
    grep -Eq \
        "^sophia_live_visual_admission schema=1 status=committed transaction=${transaction} surface=${surface} source=cpu_snapshot$" \
        "$SESSION_LOG" || fail "software Present was not committed from an immutable CPU snapshot"
    grep -Eq \
        '^sophia_live_session_startup schema=1 status=content_ready source=cpu_visual_detail$' \
        "$SESSION_LOG" || fail "software Present never produced CPU visual detail"
    for metric in cpu_nonzero_pixel_bytes cpu_nonzero_frames native_submissions \
        native_retirements native_nonzero_exports present_idle \
        present_idle_fence_triggers; do
        value="$(field "$completion" "$metric")" ||
            fail "completion does not contain $metric"
        ((value > 0)) || fail "software Present has no positive $metric evidence"
    done
    presented="$(field "$completion" present_complete_copy)" ||
        fail "completion does not contain a Present Copy count"
    ((presented > 0)) || fail "software Present has no positive completion evidence"
    authority_transactions="$(field "$completion" authority_transactions)" ||
        fail "completion does not contain authority_transactions"
    ((authority_transactions >= 3)) ||
        fail "software Present did not advance through at least three animated frames"
else
    fail "expected at most one retirement-gated admission, observed ${#armed[@]}"
fi

# Not a logout shortcut. Shortcuts are resolved against a policy client's
# configuration (`wm/public_policy.rs:2136-2145`), so a session without a
# window manager registers none at all -- there is no Super+Shift+Q here to
# press. The clean exit is the client exiting and `--exit-when-startup-exits`
# ending the session with it, which is what these records show.
grep -Eq '^sophia_live_session_health schema=1 status=clean ' "$SESSION_LOG" ||
    fail "the session did not finish healthy"
grep -Eq '^sophia_live_session_cleanup schema=1 status=clean ' "$SESSION_LOG" ||
    fail "the session did not release its resources cleanly"
grep -Eq '^sophia_live_session schema=[0-9]+ status=bounded_complete ' "$SESSION_LOG" ||
    fail "the session did not end when its application did"
grep -Eq '^sophia_live_session_health schema=1 status=clean .*wm_degraded=false$' \
    "$SESSION_LOG" || fail "session health did not drain"
grep -Eq '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$SESSION_LOG" || fail "unexpected X11 protocol errors were recorded"
grep -Eq '^sophia_live_session_cleanup schema=1 status=clean ' "$SESSION_LOG" ||
    fail "session cleanup was not clean"

for assignment in \
    native_presentation=enabled \
    wm_policy=disabled \
    wm_restarts=0 \
    wm_degraded=false \
    native_submit_failures=0 \
    native_retire_failures=0 \
    native_in_flight=false \
    native_cleanup_pending=false \
    present_live_sources=0 \
    present_live_fences=0 \
    present_live_transactions=0; do
    [[ " $completion " == *" $assignment "* ]] ||
        fail "completion does not contain $assignment"
done

echo "standalone vkcube verification passed: surface=$surface transaction=$transaction extent=${width}x${height}"

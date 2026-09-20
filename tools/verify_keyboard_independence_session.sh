#!/usr/bin/env bash
# Reads an ordinary session's record directory for the keyboard-independence
# facts: two hardware keyboards typed on, one unplugged while the seat kept
# routing, and its return announced under a new identity. No gate, no guide:
# the operator uses the desktop, unplugs and replugs the second keyboard once,
# logs out, and runs this on the session that just ended.
set -euo pipefail

sessions="${SOPHIA_SESSIONS_ROOT:-${XDG_STATE_HOME:-$HOME/.local/state}/sophia/sessions}"
session="${1:-}"
if [[ -z "$session" ]]; then
    session="$(for dir in "$sessions"/*/; do [[ -s "$dir/manifest" && -s "$dir/outcome" ]] && printf '%s\n' "${dir%/}"; done | sort | tail -n 1)"
fi
[[ -n "$session" && -d "$session" ]] || {
    echo "usage: verify_keyboard_independence_session.sh [SESSION_DIR]; no finished session under $sessions" >&2
    exit 2
}

fail() {
    echo "keyboard independence, session $(basename "$session"): $*" >&2
    exit 1
}

manifest_value() {
    sed -n "s/^$1=//p" "$session/manifest" | head -n 1
}

for name in manifest outcome health lifecycle.log; do
    [[ -s "$session/$name" ]] || fail "missing $name"
done
release_commit="$(manifest_value release_commit)"
binary_sha256="$(manifest_value sophia_binary_sha256)"
profile="$(manifest_value profile)"
[[ "$release_commit" =~ ^[0-9a-f]{40}$ && "$binary_sha256" =~ ^[0-9a-f]{64}$ ]] \
    || fail "manifest does not name a release commit and binary digest"
grep -qx 'status=exited' "$session/outcome" && grep -qx 'exit_status=0' "$session/outcome" \
    || fail "the session did not exit cleanly: $(tr '\n' ' ' <"$session/outcome")"
grep -Eq '^sophia_session_lifecycle schema=1 status=returned phase=handoff installed=true exit_status=0 emergency=false ' "$session/lifecycle.log" \
    || fail "the lifecycle log does not show an ordinary, installed, non-emergency return"
grep -qx 'storage_errors=0' "$session/health" || fail "the record store reported storage errors"
discarded="$(sed -n 's/^discarded=//p' "$session/health")"

records="$(mktemp)"
trap 'rm -f -- "$records"' EXIT
# Segments are numbered; a long session rotates the early ones away, which
# is why the deliberate check is a short session. Records are the fourth
# tab-separated column.
for segment in $(ls "$session"/events.*.log 2>/dev/null | sed -E 's/.*events\.([0-9]+)\.log/\1 &/' | sort -n | cut -d' ' -f2); do
    cut -f4- "$segment"
done | sed -E 's/^.*(sophia_(live_[^ ]+|session_[^ ]+) schema=)/\1/' >"$records"
[[ -s "$records" ]] || fail "no event records were retained"

device='^sophia_live_session_input_device schema=1 '
keyboard_added="${device}status=added device=([0-9]+) keyboard=true pointer=(true|false) touch=(true|false) virtual=false source=udev\$"
device_of() { sed -E 's/.*device=([0-9]+).*/\1/'; }

mapfile -t removals < <(grep -En "${device}status=removed device=[0-9]+ released=[0-9]+$" "$records" || true)
(( ${#removals[@]} >= 1 )) || fail "no device was removed during the session${discarded:+ (records discarded: $discarded)}"
# The unplug that counts is the removal of a hardware keyboard somebody had
# typed on. Every other removal in the run (its media interface, a mouse) is
# allowed and must not have released anything it did not hold.
unplug=""
for removal in "${removals[@]}"; do
    line="${removal%%:*}"
    candidate="$(device_of <<<"$removal")"
    if head -n "$((line - 1))" "$records" | grep -Eq "${device}status=key_observed device=$candidate\$" \
        && head -n "$((line - 1))" "$records" | grep -Eq "${device}status=added device=$candidate keyboard=true .* virtual=false "; then
        unplug="$removal"
        break
    fi
done
[[ -n "$unplug" ]] || fail "no removed device was a hardware keyboard that had been typed on"
unplug_line="${unplug%%:*}"
removed="$(device_of <<<"$unplug")"
for removal in "${removals[@]}"; do
    released="$(sed -E 's/.*released=([0-9]+)$/\1/' <<<"$removal")"
    if (( released > 0 )); then
        grep -Eq "^sophia_live_session_keys schema=1 status=released reason=device_removed device=$(device_of <<<"$removal") count=$released\$" "$records" \
            || fail "a removal released $released keys without the flush that says so"
    fi
done
released="$(sed -E 's/.*released=([0-9]+)$/\1/' <<<"$unplug")"

before() { head -n "$((unplug_line - 1))" "$records"; }
after() { tail -n "+$((unplug_line + 1))" "$records"; }
mapfile -t keyboards_before < <(before | grep -Eo "${keyboard_added}" | device_of | sort -u)
mapfile -t keyed_before < <(before | grep -Eo "${device}status=key_observed device=[0-9]+\$" | device_of | sort -u)
(( ${#keyed_before[@]} >= 2 )) || fail "keys were observed from fewer than two devices before the unplug"
other=""
for candidate in "${keyed_before[@]}"; do
    [[ "$candidate" != "$removed" ]] && printf '%s\n' "${keyboards_before[@]}" | grep -qx "$candidate" && other="$candidate" && break
done
[[ -n "$other" ]] || fail "no second hardware keyboard was typed on before the unplug"

returned=""
returned_line=""
while read -r line_and_record; do
    candidate="$(device_of <<<"$line_and_record")"
    if ! printf '%s\n' "${keyboards_before[@]}" | grep -qx "$candidate" && [[ "$candidate" != "$removed" ]]; then
        returned="$candidate"
        returned_line="${line_and_record%%:*}"
        break
    fi
done < <(after | grep -En "${keyboard_added}" || true)
[[ -n "$returned" ]] || fail "no hardware keyboard was announced under a new identity after the unplug"
after | tail -n "+$((returned_line + 1))" | grep -Eq "${device}status=key_observed device=$returned\$" \
    || fail "no key was observed from the returned keyboard $returned"
# The seat kept routing between the unplug and the return: a routing pass
# with keys in it can only have come from a keyboard that stayed.
after | head -n "$((returned_line - 1))" | grep -Eq '^sophia_live_session_input_routing schema=1 key_observed_count=[1-9][0-9]* key_routed_count=[1-9]' \
    || fail "no keys were routed between the unplug and the return, so the remaining keyboard was not shown to keep working"
grep -Eq "${device}status=summary fallbacks=0\$" "$records" \
    || fail "the seat ran on class-identity fallbacks, or its summary was not retained"

cat <<SUMMARY
keyboard independence accepted from session $(basename "$session")
  profile=$profile release_commit=$release_commit binary_sha256=$binary_sha256
  keyboards typed on before the unplug: ${keyed_before[*]} (hardware: ${keyboards_before[*]})
  unplugged: device $removed released=$released (the kernel releases a USB keyboard's keys itself; the session releases what it did not)
  kept routing: device $other typed between the unplug and the return
  returned: device $returned, a new identity, typed on after its announcement
  records discarded by the recorder: ${discarded:-0}
SUMMARY

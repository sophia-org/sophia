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
# Lines [from, to] of the records, to the end when to is 0. One awk, so no
# reader is left holding a pipe: under pipefail, head piped into grep -q is a
# match reported as a failure, which is how a correct session read as one.
slice() { awk -v a="$1" -v b="$2" 'NR >= a && (b == 0 || NR <= b)' "$records"; }
seen() { (( $(slice "$2" "$3" | grep -Ec -- "$1" || true) > 0 )); }

mapfile -t removals < <(grep -En "${device}status=removed device=[0-9]+ released=[0-9]+$" "$records" || true)
(( ${#removals[@]} >= 1 )) || fail "no device was removed during the session${discarded:+ (records discarded: $discarded)}"
# The unplug that counts is the removal of a hardware keyboard somebody had
# typed on. Every other removal in the run (its media interface, a mouse) is
# allowed and must not have released anything it did not hold.
unplug=""
for removal in "${removals[@]}"; do
    line="${removal%%:*}"
    candidate="$(device_of <<<"$removal")"
    if seen "${device}status=key_observed device=$candidate\$" 1 "$((line - 1))" \
        && seen "${device}status=added device=$candidate keyboard=true .* virtual=false " 1 "$((line - 1))"; then
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
        seen "^sophia_live_session_keys schema=1 status=released reason=device_removed device=$(device_of <<<"$removal") count=$released\$" 1 0 \
            || fail "a removal released $released keys without the flush that says so"
    fi
done
released="$(sed -E 's/.*released=([0-9]+)$/\1/' <<<"$unplug")"

before() { slice 1 "$((unplug_line - 1))"; }
after() { slice "$((unplug_line + 1))" 0; }
mapfile -t keyboards_before < <(before | grep -Eo "${keyboard_added}" | device_of | sort -u)
# The other keyboard may be typed on before the unplug or while the first is
# gone; the second is the stronger witness that the seat kept working.
mapfile -t keyed < <(grep -Eo "${device}status=key_observed device=[0-9]+\$" "$records" | device_of | sort -u)
other=""
for candidate in "${keyed[@]}"; do
    [[ "$candidate" != "$removed" ]] && printf '%s\n' "${keyboards_before[@]}" | grep -qx "$candidate" && other="$candidate" && break
done
[[ -n "$other" ]] || fail "no second hardware keyboard present before the unplug was ever typed on"

# A returning keyboard is several kernel devices again; the one that counts
# is announced under an identity never seen before and then typed on.
returned=""
returned_line=""
announced_after=0
while read -r line_and_record; do
    candidate="$(device_of <<<"$line_and_record")"
    if ! printf '%s\n' "${keyboards_before[@]}" | grep -qx "$candidate" && [[ "$candidate" != "$removed" ]]; then
        announced_after=$((announced_after + 1))
        line="${line_and_record%%:*}"
        if seen "${device}status=key_observed device=$candidate\$" "$((line + 1))" 0; then
            returned="$candidate"
            returned_line="$line"
            break
        fi
    fi
done < <(grep -En "${keyboard_added}" "$records" | awk -F: -v limit="$unplug_line" '$1 > limit' || true)
(( announced_after > 0 )) || fail "no hardware keyboard was announced under a new identity after the unplug"
[[ -n "$returned" ]] || fail "no key was observed from any keyboard announced after the unplug"
# The seat kept routing between the unplug and the return: a routing pass
# with keys in it can only have come from a keyboard that stayed.
seen '^sophia_live_session_input_routing schema=1 key_observed_count=[1-9][0-9]* key_routed_count=[1-9]' "$((unplug_line + 1))" "$((returned_line - 1))" \
    || fail "no keys were routed between the unplug and the return, so the remaining keyboard was not shown to keep working"
seen "${device}status=summary fallbacks=0\$" 1 0 \
    || fail "the seat ran on class-identity fallbacks, or its summary was not retained"

cat <<SUMMARY
keyboard independence accepted from session $(basename "$session")
  profile=$profile release_commit=$release_commit binary_sha256=$binary_sha256
  keyboards typed on: ${keyed[*]} (hardware present before the unplug: ${keyboards_before[*]})
  unplugged: device $removed released=$released (the kernel releases a USB keyboard's keys itself; the session releases what it did not)
  kept routing: device $other typed between the unplug and the return
  returned: device $returned, a new identity, typed on after its announcement
  records discarded by the recorder: ${discarded:-0}
SUMMARY

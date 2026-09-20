#!/usr/bin/env bash
# Reads an attended keyboard-independence run: the session's device records
# and the two guard logs. Every claim the gate makes is a line here.
set -euo pipefail

evidence_dir="${1:?usage: verify_keyboard_independence_physical.sh EVIDENCE_DIR [PROOF_TEXT]}"
proof_text="${2:-twokeyboards}"
(( $# <= 2 )) || {
    echo "usage: verify_keyboard_independence_physical.sh EVIDENCE_DIR [PROOF_TEXT]" >&2
    exit 2
}
[[ "$proof_text" =~ ^[a-z]{1,24}$ ]] || {
    echo "keyboard independence proof text must contain 1-24 lowercase ASCII letters" >&2
    exit 2
}

fail() {
    echo "keyboard independence verification failed: $*" >&2
    exit 1
}

for name in session.log guard_seat.log guard_pinned.log; do
    [[ -s "$evidence_dir/$name" ]] || fail "missing or empty $name in $evidence_dir"
done

normalized="$(mktemp -d)"
trap 'rm -rf -- "$normalized"' EXIT
# Production tracing prefixes schema payloads with timestamp, target, and ANSI
# state. Normalize only Sophia schema records; client output is left alone.
for name in session.log guard_seat.log guard_pinned.log; do
    sed -E 's/^.*(sophia_(live_[^ ]+|session_[^ ]+|keyboard_independence[^ ]*|x11_[^ ]+) schema=)/\1/' \
        "$evidence_dir/$name" >"$normalized/$name"
done
session="$normalized/session.log"

device_record='^sophia_live_session_input_device schema=1 '
if grep -Eqi '(^Error:|panicked at)' "$session"; then
    fail "session evidence contains an error or a panic"
fi
[[ "$(grep -Ec '^sophia_keyboard_independence_identity schema=1 status=bound sophia_commit=[0-9a-f]{40} sophia_sha256=[0-9a-f]{64}$' "$session")" == 1 ]] \
    || fail "exactly one bound source identity is required"

poller_ready="$(grep -E '^sophia_live_session_input_pipeline schema=4 status=poller_ready source=udev ' "$session" || true)"
[[ -n "$poller_ready" ]] || fail "the session did not open its seat through udev"
seat_keyboards="$(sed -n 's/.* keyboards=\([0-9]*\).*/\1/p' <<<"$poller_ready" | head -n 1)"
[[ "$seat_keyboards" =~ ^[0-9]+$ ]] && (( seat_keyboards >= 2 )) \
    || fail "the seat opened with fewer than two keyboards: ${seat_keyboards:-none}"

# One keyboard is several kernel devices (a media interface beside the main
# one), so one unplug is several removals. Exactly one of them held the key;
# every other removal in the run must have released nothing.
mapfile -t removal_lines < <(grep -En "${device_record}status=removed device=[0-9]+ released=[0-9]+$" "$session" || true)
(( ${#removal_lines[@]} >= 1 )) || fail "no device removal was recorded"
mapfile -t held_removals < <(printf '%s\n' "${removal_lines[@]}" | grep -E ' released=1$' || true)
(( ${#held_removals[@]} == 1 )) || fail "expected exactly one removal releasing the one held key, found ${#held_removals[@]}"
if printf '%s\n' "${removal_lines[@]}" | grep -Eq ' released=([2-9]|[1-9][0-9]+)$'; then
    fail "a removal released more than the one key the unplugged keyboard held"
fi
removal_line="${held_removals[0]%%:*}"
removed="$(sed -n 's/.* device=\([0-9]*\) released=.*/\1/p' <<<"${held_removals[0]}")"
grep -Eq "^sophia_live_session_keys schema=1 status=released reason=device_removed device=$removed count=1$" "$session" \
    || fail "the removal's release was not recorded as a device_removed flush of one key"
if [[ "$(grep -Ec '^sophia_live_session_keys schema=1 status=released reason=device_removed ' "$session")" != 1 ]]; then
    fail "more than one device_removed flush was recorded"
fi

before() { awk -v limit="$removal_line" 'NR < limit' "$session"; }
after() { awk -v limit="$removal_line" 'NR > limit' "$session"; }

keyboard_added='status=added device=([0-9]+) keyboard=true pointer=(true|false) touch=(true|false) virtual=false source=udev$'
mapfile -t keyboards_before < <(before | grep -Eo "${device_record}${keyboard_added}" | sed -E 's/.*device=([0-9]+) .*/\1/')
(( ${#keyboards_before[@]} >= 2 )) || fail "fewer than two hardware keyboards were announced before the removal"
printf '%s\n' "${keyboards_before[@]}" | grep -qx "$removed" \
    || fail "the removed device $removed was not announced as a hardware keyboard"
if before | grep -Eq "${device_record}status=added device=$removed .* virtual=true "; then
    fail "the removed device was announced as virtual"
fi

mapfile -t keyed_before < <(before | grep -Eo "${device_record}status=key_observed device=[0-9]+$" | sed -E 's/.*device=//' | sort -u)
(( ${#keyed_before[@]} >= 2 )) || fail "keys were observed from fewer than two devices before the removal"
printf '%s\n' "${keyed_before[@]}" | grep -qx "$removed" || fail "no key was observed from the removed device before it left"

returned="$(after | grep -Eo "${device_record}${keyboard_added}" | sed -E 's/.*device=([0-9]+) .*/\1/' | head -n 1 || true)"
[[ -n "$returned" ]] || fail "no hardware keyboard was announced after the removal"
[[ "$returned" != "$removed" ]] || fail "the replugged keyboard was given the identity the removed one had"
if printf '%s\n' "${keyboards_before[@]}" | grep -qx "$returned"; then
    fail "the replugged keyboard's identity $returned had been announced before the removal"
fi
returned_line="$(after | grep -En "${device_record}status=added device=$returned " | head -n 1 | cut -d: -f1)"
after | awk -v limit="$returned_line" -v pattern="${device_record}status=key_observed device=$returned\$" \
    'NR > limit && $0 ~ pattern { found = 1 } END { exit found ? 0 : 1 }' \
    || fail "no key was observed from the replugged keyboard $returned after its announcement"

grep -Eq "^sophia_live_session_input schema=1 status=ready source=physical text=$proof_text$" "$session" \
    || fail "physical input readiness for $proof_text is missing"
grep -Eq "^sophia_live_session_input schema=2 status=complete source=physical text=$proof_text expected_events=[1-9][0-9]* matched_events=[1-9][0-9]* pixel_change=true$" "$session" \
    || fail "exact physical text completion for $proof_text is missing"
grep -Eq '^sophia_live_session schema=16 status=bounded_complete .* physical_input=enabled( |$)' "$session" \
    || fail "bounded session completion with physical input enabled is missing"
grep -Eq '^sophia_live_session_health schema=1 status=clean protocol_errors=0 ' "$session" \
    || fail "clean session health is missing"
grep -Eq '^sophia_live_session_cleanup schema=1 status=clean ' "$session" \
    || fail "clean process cleanup is missing"
grep -Eq "${device_record}status=summary fallbacks=0$" "$session" \
    || fail "the seat ran on class-identity fallbacks, or never summarised"

verify_guard() {
    local phase="$1" log="$normalized/guard_$1.log" source="$2" keyboards_rule="$3"
    local ready armed triggered ignored keyboards
    ready="$(grep -En "^sophia_session_input_guard schema=2 status=ready source=$source " "$log" || true)"
    [[ "$(grep -c . <<<"$ready")" == 1 && -n "$ready" ]] || fail "the $phase guard must report ready exactly once from $source"
    keyboards="$(sed -n 's/.* keyboards=\([0-9]*\).*/\1/p' <<<"$ready")"
    [[ "$keyboards" =~ ^[0-9]+$ ]] && eval "(( keyboards $keyboards_rule ))" \
        || fail "the $phase guard opened ${keyboards:-no} keyboards, expected $keyboards_rule"
    ignored="$(grep -En "^sophia_keyboard_independence_guard schema=1 status=split_chord_ignored phase=$phase$" "$log" || true)"
    [[ "$(grep -c . <<<"$ignored")" == 1 && -n "$ignored" ]] || fail "the $phase guard's split-chord witness is missing"
    armed="$(grep -En '^sophia_session_input_guard schema=1 status=armed$' "$log" || true)"
    [[ "$(grep -c . <<<"$armed")" == 1 && -n "$armed" ]] || fail "the $phase guard must arm exactly once"
    triggered="$(grep -En '^sophia_session_input_guard schema=1 status=triggered$' "$log" || true)"
    [[ "$(grep -c . <<<"$triggered")" == 1 && -n "$triggered" ]] || fail "the $phase guard must trigger exactly once"
    (( ${ready%%:*} < ${ignored%%:*} && ${ignored%%:*} < ${armed%%:*} && ${armed%%:*} < ${triggered%%:*} )) \
        || fail "the $phase guard's records are out of order: ready, split chord ignored, armed, triggered"
}

verify_guard seat udev ">= 2"
verify_guard pinned paths "== 1"

echo "keyboard independence physical evidence passed: removed=$removed returned=$returned"

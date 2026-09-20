#!/usr/bin/env bash
# The keyboard-independence verifier against a passing fixture and against
# every mutation that would turn a failed run into a passing one.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$ROOT_DIR/tools/fixtures/keyboard_independence_physical_pass"
verifier="$ROOT_DIR/tools/verify_keyboard_independence_physical.sh"
gate="$ROOT_DIR/tools/keyboard_independence_physical_gate.sh"
guide="$ROOT_DIR/tools/fixtures/keyboard_independence_guide.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

"$verifier" "$fixture" twokeyboards >/dev/null
if "$verifier" "$fixture" otherphrase >/dev/null 2>&1; then
    echo "keyboard independence verifier accepted a run for a different phrase" >&2
    exit 1
fi

reject_mutation() {
    local file="$1" expression="$2" description="$3"
    rm -rf -- "$work/rejected"
    cp -r "$fixture" "$work/rejected"
    sed -i "$expression" "$work/rejected/$file"
    if "$verifier" "$work/rejected" twokeyboards >/dev/null 2>&1; then
        echo "keyboard independence verifier accepted $description" >&2
        exit 1
    fi
}

reject_mutation session.log '/status=removed device=256/d' 'a run where only the media interface was removed'
reject_mutation session.log '/status=removed device=/d' 'a run with no removal'
reject_mutation session.log 's/status=removed device=261 released=0/status=removed device=261 released=1/' 'a second interface that released a key without the flush that says so'
reject_mutation session.log '/reason=device_removed device=256 count=1/d' 'a removal that released a key without its release flush'
reject_mutation session.log 's/status=added device=260 /status=added device=256 /; s/key_observed device=260/key_observed device=256/' 'a replug under the removed identity'
reject_mutation session.log 's/status=added device=260 /status=added device=258 /; s/key_observed device=260/key_observed device=258/' 'a replug under an identity announced before the removal'
reject_mutation session.log '/key_observed device=260/d' 'a replugged keyboard nobody typed on'
reject_mutation session.log '/key_observed device=258/d' 'a run where only the unplugged keyboard was typed on'
reject_mutation session.log 's/status=added device=256 keyboard=true pointer=false touch=false virtual=false/status=added device=256 keyboard=true pointer=false touch=false virtual=true/' 'an unplugged keyboard that was virtual'
reject_mutation session.log 's/status=added device=260 keyboard=true pointer=false touch=false virtual=false/status=added device=260 keyboard=true pointer=false touch=false virtual=true/; s/status=added device=262 keyboard=true pointer=true touch=false virtual=false/status=added device=262 keyboard=true pointer=true touch=false virtual=true/' 'a replugged keyboard that was virtual'
reject_mutation session.log 's/status=summary fallbacks=0/status=summary fallbacks=3/' 'a seat that ran on class-identity fallbacks'
reject_mutation session.log '/status=complete source=physical/d' 'a run without the physical text completion'
reject_mutation session.log 's/physical_input=enabled/physical_input=disabled/' 'a completion without physical input'
reject_mutation session.log '/sophia_keyboard_independence_identity/d' 'a run with no bound source identity'
reject_mutation session.log 's/keyboards=2 pointers=1 touch=0 tap_capable/keyboards=1 pointers=1 touch=0 tap_capable/' 'a seat opened with one keyboard'
reject_mutation guard_seat.log '/status=split_chord_ignored/d' 'a seat guard without the split-chord witness'
reject_mutation guard_seat.log 's/^sophia_session_input_guard schema=1 status=armed$/sophia_session_input_guard schema=1 status=armed\nsophia_session_input_guard schema=1 status=armed/' 'a seat guard that armed twice'
reject_mutation guard_seat.log '1s/^/sophia_session_input_guard schema=1 status=armed\n/' 'a seat guard armed before it was ready'
reject_mutation guard_pinned.log 's/source=paths seat=explicit devices=1 keyboards=1/source=paths seat=explicit devices=2 keyboards=2/' 'a pinned guard that opened two keyboards'
reject_mutation guard_pinned.log '/status=triggered/d' 'a pinned guard that never triggered'

# The guide and the gate must ask for the steps the verifier requires, in the
# order it requires them.
mapfile -t steps < <(grep -oE "status=(key_observed|removed|added)[^'\"]*" "$guide" | sed -E 's/ .*//' | uniq)
expected_steps=(status=key_observed status=removed status=added status=key_observed)
[[ "${steps[*]}" == "${expected_steps[*]}" ]] || {
    echo "keyboard independence guide waits on ${steps[*]}, the verifier requires ${expected_steps[*]}" >&2
    exit 1
}
grep -Fq 'UNPLUG keyboard A' "$guide" && grep -Fq 'Plug keyboard A back in' "$guide" || {
    echo "keyboard independence guide omitted the unplug or replug step" >&2
    exit 1
}
grep -Fq 'guard_phase seat' "$gate" && grep -Fq 'guard_phase pinned' "$gate" \
    && grep -Fq 'status=split_chord_ignored phase=%s' "$gate" || {
    echo "keyboard independence gate omitted a guard phase or its split-chord witness" >&2
    exit 1
}
grep -Fq -- '--expect-physical-text=$proof_text' "$gate" && grep -Fq -- '--input-seat=$seat' "$gate" || {
    echo "keyboard independence gate does not run the session on the seat with the physical text proof" >&2
    exit 1
}
grep -Fq 'verify_keyboard_independence_physical.sh' "$gate" \
    && grep -Fq 'archive_keyboard_independence_physical_run.sh' "$gate" || {
    echo "keyboard independence gate does not verify and archive its run" >&2
    exit 1
}
grep -Fq 'pgrep -x keyd' "$gate" || {
    echo "keyboard independence gate does not refuse a running key remapper" >&2
    exit 1
}
echo "keyboard independence verifier check passed"

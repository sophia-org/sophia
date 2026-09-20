#!/bin/sh
# Runs inside the session's terminal. Each step advances only when the
# session records the fact the step is for; the last step types the proof.
set -eu

evidence="${SOPHIA_LIVE_SESSION_PERSISTENT_EVIDENCE:-/tmp/sophia-keyboard-independence-physical/session.log}"
proof_text="${SOPHIA_KEYBOARD_INDEPENDENCE_TEXT:-twokeyboards}"
proof_result="${SOPHIA_INPUT_PROOF_RESULT:-}"

case "$proof_text" in
    *[!a-z]*|'')
        echo "invalid keyboard independence proof text" >&2
        exit 2
        ;;
esac
if [ -z "$proof_result" ]; then
    echo "Sophia did not provide the physical proof result path" >&2
    exit 2
fi

abort() {
    printf '\033[2J\033[H'
    echo 'Physical proof aborted: the session never produced' >&2
    echo "  $1" >&2
    echo 'Log out with Ctrl+Alt+Delete and inspect the session log.' >&2
    exit 2
}

# An operator step is bounded by minutes, because a person is unplugging a
# cable; a session step by seconds, because nobody is.
wait_for_count() {
    pattern="$1"
    expected="$2"
    attempts="${3:-6000}"
    while [ "$(grep -Ec "$pattern" "$evidence" 2>/dev/null || true)" -lt "$expected" ]; do
        attempts=$((attempts - 1))
        if [ "$attempts" -le 0 ]; then
            abort "$pattern (x$expected)"
        fi
        sleep 0.1
    done
}

# The line number of the first match, so a later step can ask for a record
# that appears after it.
line_of() {
    grep -En "$1" "$evidence" 2>/dev/null | head -n 1 | cut -d: -f1
}

wait_for_line_after() {
    pattern="$1"
    after="$2"
    attempts="${3:-6000}"
    while ! awk -v limit="$after" -v pattern="$pattern" \
        'NR > limit && $0 ~ pattern { found = 1 } END { exit found ? 0 : 1 }' "$evidence" 2>/dev/null; do
        attempts=$((attempts - 1))
        if [ "$attempts" -le 0 ]; then
            abort "$pattern after line $after"
        fi
        sleep 0.1
    done
}

show_step() {
    printf '\033[2J\033[H'
    printf '%s\n\n%s\n\n%s\n%s\n' \
        'KEYBOARD INDEPENDENCE PROOF' "$1" \
        'Press ONLY the keys named. Any other key before the final phrase fails the proof.' \
        'This screen advances only after Sophia records the step.'
}

wait_for_count '^sophia_live_session_input schema=1 status=ready source=physical ' 1 1200

show_step 'Press and release LEFT SHIFT on keyboard B, the keyboard that stays.'
wait_for_count '^sophia_live_session_input_device schema=1 status=key_observed device=[0-9]+$' 1

show_step 'Press and HOLD LEFT SHIFT on keyboard A. Keep holding it until told to let go.'
wait_for_count '^sophia_live_session_input_device schema=1 status=key_observed device=[0-9]+$' 2

show_step 'STILL HOLDING keyboard A: press and release LEFT SHIFT on keyboard B once.
Wait three seconds. Keyboard A must still be held.'
sleep 3

show_step 'STILL HOLDING the shift on keyboard A: UNPLUG keyboard A now.
Sophia must release the one key it held, and nothing else.'
wait_for_count '^sophia_live_session_input_device schema=1 status=removed device=[0-9]+ released=1$' 1
removed_line="$(line_of '^sophia_live_session_input_device schema=1 status=removed device=[0-9]+ released=1$')"

show_step 'Plug keyboard A back in and wait. Sophia must announce it under a new identity.'
wait_for_line_after '^sophia_live_session_input_device schema=1 status=added device=[0-9]+ keyboard=true .* virtual=false ' "$removed_line"
returned="$(awk -v limit="$removed_line" \
    'NR > limit && /^sophia_live_session_input_device schema=1 status=added device=[0-9]+ keyboard=true .* virtual=false / { sub(/.*device=/, ""); sub(/ .*/, ""); print; exit }' \
    "$evidence")"

show_step "Press and release LEFT SHIFT on the replugged keyboard A."
wait_for_count "^sophia_live_session_input_device schema=1 status=key_observed device=$returned\$" 1

printf '\033[2J\033[H'
printf '%s\n\n%s\n\n' \
    'ALL DEVICE STEPS RECORDED' \
    "On keyboard B, type $proof_text and press Enter. This immediately ends the session."

while IFS= read -r line; do
    if [ "$line" = "$proof_text" ]; then
        break
    fi
    printf '%s\n' "Type the exact final phrase: $proof_text"
done

umask 077
printf '%s' "$proof_text" >"$proof_result"
printf '%s\n' 'Proof phrase accepted. Waiting for Sophia to complete the session.'
while :; do
    sleep 1
done

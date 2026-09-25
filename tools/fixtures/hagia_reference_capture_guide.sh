#!/bin/sh
set -eu

# The t018 reference-capture guide. It proves the physical input path exactly
# as the native guide does, then shows the tab matrix and leaves the operator
# at a shell. It performs no matrix action and waits for none: every matrix
# item is an operator observation, retained unverified. The capture claims only
# bound identity and profile, exit 0, TTY recovery and retained evidence.
#
# The keys are those of tools/fixtures/t018_tab_reference.kdl.

evidence="${SOPHIA_LIVE_SESSION_PERSISTENT_EVIDENCE:-}"
proof_text="${SOPHIA_HAGIA_NATIVE_TEXT:-hagianativeproof}"
proof_result="${SOPHIA_INPUT_PROOF_RESULT:-}"
guide_claim="${SOPHIA_HAGIA_NATIVE_GUIDE_CLAIM:-}"

# Only the startup terminal is the guide; later terminals are ordinary shells
# (the native guide explains why every terminal runs this script).
if [ -n "$guide_claim" ] && ! (set -C; : >"$guide_claim") 2>/dev/null; then
    exec "${SHELL:-/bin/sh}"
fi

case "$proof_text" in
    *[!a-z]*|'')
        echo "invalid Hagia proof text" >&2
        exit 2
        ;;
esac
if [ -z "$proof_result" ] || [ -z "$evidence" ]; then
    echo "Sophia did not provide the proof result or session evidence path" >&2
    exit 2
fi

printf '\033[2J\033[H'
printf '%s\n\n%s\n\n' \
    'HAGIA t018 REFERENCE CAPTURE (not an acceptance)' \
    "Type $proof_text and press Enter to prove the physical input path."
while IFS= read -r line; do
    if [ "$line" = "$proof_text" ]; then
        break
    fi
    printf '%s\n' "Type the exact phrase: $proof_text"
done
umask 077
printf '%s' "$proof_text" >"$proof_result"

attempts=600
until grep -Eq "^sophia_live_session_input schema=2 status=complete source=physical text=$proof_text expected_events=[1-9][0-9]* matched_events=[1-9][0-9]* pixel_change=true$" "$evidence" 2>/dev/null; do
    attempts=$((attempts - 1))
    if [ "$attempts" -le 0 ]; then
        echo 'The session never recorded the completed input proof.' >&2
        echo 'Log out with Ctrl+Alt+Delete; this capture will be refused.' >&2
        exec "${SHELL:-/bin/sh}"
    fi
    sleep 0.1
done

printf '\033[2J\033[H'
cat <<'MATRIX'
HAGIA t018 REFERENCE CAPTURE -- record what you observe; nothing here is verified

Keys: Super+Return terminal   Super+q close   Super+Ctrl+Alt+1/2/3 frame-tree/notion/i3
      Super+s/v split H/V   Super+u unsplit   Super+Tab/Shift+Tab next/prev tab
      Super+a/z frame parent/child   Super+Shift+s/v i3 split H/V
      Super+w tabbed   Super+e stacking   Super+Shift+e toggle split
      Super+arrows focus   Super+Shift+arrows move   Super+y group
      Super+Shift+f fullscreen   Super+t float   Super+drag move/resize

 1. frame-tree: empty and occupied sibling frames; move, focus and resize
    across them; empty bars have no activation.
 2. notion: group windows; click a hidden member's visible tab; focus and
    client pixels move together.
 3. i3: nest split, tabbed and stacked containers; parent/child navigation.
    split-tree is Hagia's alias for i3, not a separate action.
 4. Both outputs, at different scales where available: bar geometry and
    pointer targets match the presented allocation.
 5. Title change: labels update. Shell recovery: from this terminal run
    `pkill -x narthex`; old tab actions stop at once, neutral bars acquire
    no authority, fresh actions follow the replacement.
 6. Fullscreen suppresses bars; a floating window over a tab prevents
    activating the covered target; restoring needs the current state.
 7. Press Ctrl+Alt+Delete once to log out normally.
MATRIX
exec "${SHELL:-/bin/sh}"

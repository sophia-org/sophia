#!/usr/bin/env bash
# The QEMU xtest-selection scenario's verdict: did a real xterm take PRIMARY
# on an XTEST drag, and did a second real xterm ask for it on an XTEST
# middle-click, inside a guest with a scanned-out head?
#
# The same obligations as `cargo xtask check xtest-selection`, read from the
# guest's serial log: the driver's exact pass line is the session's to
# compare, and this reads the session's own wire counters so a silent
# selection cannot be mistaken for a gesture that landed. One bounded
# completion is required as a whole-session witness, by its fields; this
# reader makes no startup-proof claim and pins no completion schema.
set -euo pipefail

EVIDENCE_FILE="${1:-${SOPHIA_QEMU_EVIDENCE:-/tmp/sophia-qemu-xtest-selection.log}}"

fail() {
    echo "QEMU xtest-selection evidence $1" >&2
    exit 1
}

require_exactly_one() {
    local pattern="$1"
    local description="$2"
    if [[ "$(grep -c "^${pattern}$" "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
        fail "is missing $description"
    fi
}

# A field's value from the last record whose name and status match. The last
# one, because the session prints the record once at completion and a fixture
# or a rerun appended to the same file must not be read as the first run.
field() {
    local record="$1"
    local key="$2"
    grep -E "^${record} " "$EVIDENCE_FILE" | tail -n 1 \
        | tr ' ' '\n' | sed -n "s/^${key}=//p" | head -n 1
}

require_exactly_one \
    'sophia_qemu_xtest_selection schema=1 status=starting isolation=headless control=none host_drm=none host_vt=none gpu=virtio-gpu input=xtest row=[0-7]' \
    'the isolated start marker'
require_exactly_one \
    'sophia_qemu_guest schema=1 status=booting gpu=virtio-gpu scenario=xtest-selection' \
    'the guest boot marker'
require_exactly_one \
    'sophia_qemu_xtest_selection schema=1 status=running row=[0-7]' \
    'the guest scenario marker'
require_exactly_one \
    'sophia_live_session_xtest schema=1 status=admitted group=1' \
    'XTEST admission'
require_exactly_one \
    'sophia_qemu_guest schema=1 status=complete scenario=xtest-selection' \
    'clean guest completion'
require_exactly_one \
    'sophia_qemu_xtest_selection schema=1 status=complete qemu_exit=0' \
    'clean host completion'
if grep -q '^sophia_qemu_.* status=failed' "$EVIDENCE_FILE"; then
    fail "contains a failure marker"
fi

# The driver's stdout never reaches this log: the session captures it,
# compares it byte for byte with the pass line, and reports the comparison in
# its application record. A finding exits 0 from the driver but fails that
# comparison. The record is required here so a log from a session that
# stopped comparing cannot pass on counters alone.
if ! grep -Eq '^sophia_x_application_session schema=1 status=passed class=[a-z0-9_]+ client=xtest_selection_driver profile=classic_shared child_outcome=normal exit_code=0 stdout_match=true protocol_errors=0 first_error=none .*native_presentation=enabled cleanup=clean$' "$EVIDENCE_FILE"; then
    fail "is missing the session's matched application record"
fi

if [[ "$(grep -cE '^sophia_live_session .*status=bounded_complete ' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    fail "is missing one bounded live-session completion"
fi

owner_changes="$(field 'sophia_live_selection schema=1 status=complete' owner_changes)"
conversions="$(field 'sophia_live_selection schema=1 status=complete' conversions)"
[[ "$owner_changes" =~ ^[0-9]+$ && "$conversions" =~ ^[0-9]+$ ]] \
    || fail "is missing the selection counters"
# One SetSelectionOwner is xterm A taking PRIMARY on the release.
(( owner_changes >= 1 )) || fail "shows no client taking PRIMARY (owner_changes=$owner_changes)"
# Two ConvertSelection: the driver reading the text back, then xterm B's paste.
(( conversions >= 2 )) || fail "shows no paste asking for PRIMARY (conversions=$conversions)"

admitted="$(field 'sophia_live_session_xtest schema=1 status=complete' admitted)"
refused="$(field 'sophia_live_session_xtest schema=1 status=complete' refused)"
buttons="$(field 'sophia_live_session_xtest schema=1 status=complete' injected_buttons)"
[[ "$admitted" == true ]] || fail "shows XTEST was not admitted"
[[ "$refused" == 0 ]] || fail "shows a refused injection (refused=$refused)"
[[ "$buttons" =~ ^[0-9]+$ ]] && (( buttons >= 4 )) \
    || fail "shows fewer than the drag's and the paste's four buttons (injected_buttons=${buttons:-none})"

echo "QEMU xtest-selection evidence passed: owner_changes=$owner_changes conversions=$conversions injected_buttons=$buttons ($EVIDENCE_FILE)"

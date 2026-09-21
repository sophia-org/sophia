#!/usr/bin/env bash
set -euo pipefail

EVIDENCE_FILE="${1:-${SOPHIA_QEMU_EVIDENCE:-/tmp/sophia-qemu-emergency-recovery.log}}"

require_exactly_one() {
    local pattern="$1"
    local description="$2"
    if [[ "$(grep -c "^${pattern}$" "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
        echo "QEMU emergency recovery evidence is missing $description" >&2
        exit 1
    fi
}

require_exactly_one \
    'sophia_qemu_recovery schema=1 status=starting isolation=headless control=qmp-unix host_drm=none host_vt=none keyboard=virtio chord=ctrl-alt-backspace' \
    'the isolated start marker'
require_exactly_one \
    'sophia_qemu_guest_recovery schema=1 status=running chord=ctrl-alt-backspace' \
    'the guest recovery marker'
require_exactly_one \
    'sophia_qemu_recovery_input schema=1 status=sent phase=arm source=qmp device=virtio-keyboard chord=ctrl-alt-backspace events=6' \
    'QMP arming chord delivery'
require_exactly_one \
    'sophia_qemu_recovery_input schema=1 status=sent phase=trigger source=qmp device=virtio-keyboard chord=ctrl-alt-backspace events=6' \
    'QMP trigger chord delivery'
require_exactly_one \
    'sophia_session_input_guard schema=1 status=armed' \
    'independent input guard arming'
require_exactly_one \
    'sophia_session_input_guard schema=1 status=triggered' \
    'independent input guard trigger'
require_exactly_one \
    'sophia_live_session_input_pipeline schema=1 status=emergency_exit' \
    'Sophia emergency exit'
require_exactly_one \
    'sophia_qemu_guest_recovery schema=1 status=complete exit_status=0 guard_exit_status=0' \
    'clean guest recovery completion'
require_exactly_one \
    'sophia_qemu_recovery schema=1 status=complete qemu_exit=0' \
    'clean host completion'

# Schema 4 names the seat it opened and splits the device count by kind. The
# keyboard count carries the same weight as the guard's: this scenario proves a
# key chord, and a poller holding pointers alone could not see one.
if ! grep -Eq '^sophia_live_session_input_pipeline schema=4 status=poller_ready source=[a-z_]+ seat=[a-z_]+ devices=[1-9][0-9]* active=[1-9][0-9]* keyboards=[1-9][0-9]* pointers=[0-9]+ touch=[0-9]+ tap_capable=[0-9]+ tap_enabled=[0-9]+ pointer_configured=[0-9]+ settings_unsupported=[0-9]+$' "$EVIDENCE_FILE"; then
    echo "QEMU emergency recovery evidence is missing physical input readiness" >&2
    exit 1
fi
# The guard's readiness line is schema 2 and names what it opened: a seat, its
# devices, and how many of them are keyboards. The count that matters here is
# the keyboards -- the chord this scenario fires is a key combination, and a
# guard holding six devices and no keyboard could not observe it.
if ! grep -Eq '^sophia_session_input_guard schema=2 status=ready source=[a-z_]+ seat=[a-z_]+ devices=[1-9][0-9]* keyboards=[1-9][0-9]*$' "$EVIDENCE_FILE"; then
    echo "QEMU emergency recovery evidence is missing independent input guard readiness" >&2
    exit 1
fi
if ! grep -q '^sophia_live_session_input_pipeline schema=1 status=focus_ready$' "$EVIDENCE_FILE"; then
    echo "QEMU emergency recovery evidence is missing committed input focus" >&2
    exit 1
fi
if ! grep -q '^sophia_live_session_input_pipeline schema=1 status=key_observed$' "$EVIDENCE_FILE"; then
    echo "QEMU emergency recovery evidence did not observe the virtual chord" >&2
    exit 1
fi

if [[ "$(grep -cE '^sophia_live_session .*status=bounded_complete ' "$EVIDENCE_FILE" || true)" -ne 1 ]]; then
    echo "QEMU emergency recovery evidence is missing bounded live-session cleanup" >&2
    exit 1
fi
completion_line="$(grep -E '^sophia_live_session .*status=bounded_complete ' "$EVIDENCE_FILE")"
if [[ ! " $completion_line " =~ " physical_input=enabled " ]]; then
    echo "QEMU emergency recovery did not use the virtual input path" >&2
    exit 1
fi
if [[ ! " $completion_line " =~ " native_in_flight=false " ]] \
    || [[ ! " $completion_line " =~ " native_cleanup_pending=false " ]]; then
    echo "QEMU emergency recovery left native scanout work pending" >&2
    exit 1
fi
if grep -q '^sophia_qemu_.* status=failed' "$EVIDENCE_FILE"; then
    echo "QEMU emergency recovery evidence contains a failure marker" >&2
    exit 1
fi

echo "QEMU Ctrl-Alt-Backspace emergency recovery evidence passed: $EVIDENCE_FILE"

#!/usr/bin/env bash
# The session-lock QEMU scenario (t034): once the physical input proof is
# armed, so keys would otherwise reach zenity, Session locks itself; a wrong
# password typed on the virtio keyboard is rejected by real PAM and the right
# one unlocks. zenity's exact stdout (checked by the session) shows no
# lock-time key reached it; this verifier checks the lock's record and order.
set -euo pipefail

EVIDENCE_FILE="${1:-${SOPHIA_QEMU_EVIDENCE:-/tmp/sophia-qemu-session-lock.log}}"

fail() {
    echo "QEMU session-lock evidence: $1" >&2
    exit 1
}

# The line number of the one line matching an extended pattern.
only_line() {
    local pattern="$1" description="$2" lines
    lines="$(grep -nE "^${pattern}$" "$EVIDENCE_FILE" | cut -d: -f1 || true)"
    [[ -n "$lines" && "$(wc -l <<<"$lines")" -eq 1 ]] || fail "expected exactly one $description"
    echo "$lines"
}

lock='sophia_live_session_lock schema=1'
locking="$(only_line "$lock status=locking source=proof epoch=1 input_epoch=[0-9]+ revoked_leases=[0-9]+" 'lock start')"
locked="$(only_line "$lock status=locked epoch=1" 'covered lock')"
wrong_sent="$(only_line 'sophia_qemu_lock_input schema=1 status=sent source=qmp secret=wrong' 'wrong password delivery')"
checking_wrong="$(only_line "$lock status=checking epoch=1 attempt=1" 'first attempt')"
failed="$(only_line "$lock status=failed epoch=1 attempt=1 verdict=Rejected" 'rejected first attempt')"
right_sent="$(only_line 'sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right' 'right password delivery')"
checking_right="$(only_line "$lock status=checking epoch=1 attempt=2" 'second attempt')"
unlocking="$(only_line "$lock status=unlocking epoch=1 input_epoch=[0-9]+ revoked_leases=[0-9]+" 'accepted verdict')"
unlocked="$(only_line "$lock status=unlocked epoch=1" 'unlock')"
input_ready="$(only_line 'sophia_live_session_input schema=1 status=ready source=physical text=sophia' 'input proof arming')"

# Session's own record is strictly ordered. The host writes each "sent"
# marker after its keys went out, by which time the guest may already have
# acted on them, so a marker bounds only what preceded the typing.
previous=0
for step in "$input_ready" "$locking" "$locked" "$checking_wrong" "$failed" \
    "$checking_right" "$unlocking" "$unlocked"; do
    (( step > previous )) || fail "lock steps are out of order"
    previous="$step"
done
(( wrong_sent > locked && right_sent > failed && right_sent > wrong_sent )) \
    || fail "a password was typed before the lock it was meant for"

if grep -qE "^$lock status=(refused|stale_verdict|unavailable|unlock_repaint_failed|already_locked)( |$)" "$EVIDENCE_FILE"; then
    fail "the lock recorded a refusal, stale verdict or unavailable authenticator"
fi
# The secrets never reach a log: not the session's, the agent's or PAM's.
if grep -qE 'wrongpass|sophialock' "$EVIDENCE_FILE"; then
    fail "a password appears in the evidence"
fi
echo "sophia_qemu_session_lock_evidence schema=1 status=pass"

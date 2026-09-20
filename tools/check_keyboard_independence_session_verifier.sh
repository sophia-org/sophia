#!/usr/bin/env bash
# The session verifier against a passing fixture directory and against every
# mutation that would let a run that did not show the property pass.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$ROOT_DIR/tools/fixtures/keyboard_independence_session_pass"
verifier="$ROOT_DIR/tools/verify_keyboard_independence_session.sh"
work="$(mktemp -d)"
trap 'rm -rf -- "$work"' EXIT

"$verifier" "$fixture" >/dev/null

reject_mutation() {
    local file="$1" expression="$2" description="$3"
    rm -rf -- "$work/rejected"
    cp -r "$fixture" "$work/rejected"
    sed -i "$expression" "$work/rejected/$file"
    if "$verifier" "$work/rejected" >/dev/null 2>&1; then
        echo "keyboard independence session verifier accepted $description" >&2
        exit 1
    fi
}

reject_mutation events.0.log '/status=removed device=/d' 'a session with no removal'
reject_mutation events.0.log '/status=removed device=270 /d' 'a session where only a keyboard nobody typed on was removed'
reject_mutation events.0.log '/key_observed device=270/d' 'an unplugged keyboard nobody had typed on'
reject_mutation events.0.log '/key_observed device=256/d' 'a session with only the unplugged keyboard typed on'
reject_mutation events.0.log 's/key_observed device=256/key_observed device=258/' 'a second device typed on that was a pointer, not a keyboard'
reject_mutation events.0.log 's/status=removed device=270 released=0/status=removed device=270 released=2/' 'a removal that released keys without the flush that says so'
reject_mutation events.0.log 's/status=added device=276 /status=added device=270 /; s/key_observed device=276/key_observed device=270/' 'a return under the removed identity'
reject_mutation events.0.log 's/status=added device=276 /status=added device=256 /; s/key_observed device=276/key_observed device=256/' 'a return under an identity seen before the unplug'
reject_mutation events.0.log '/key_observed device=276/d' 'a returned keyboard nobody typed on'
reject_mutation events.0.log 's/status=added device=275 keyboard=true pointer=true touch=false virtual=false/status=added device=275 keyboard=true pointer=true touch=false virtual=true/; s/status=added device=276 keyboard=true pointer=false touch=false virtual=false/status=added device=276 keyboard=true pointer=false touch=false virtual=true/' 'a return that was virtual'
reject_mutation events.0.log 's/status=added device=256 keyboard=true pointer=false touch=false virtual=false/status=added device=256 keyboard=true pointer=false touch=false virtual=true/' 'a second keyboard that was virtual'
reject_mutation events.0.log 's/key_observed_count=4 key_routed_count=4/key_observed_count=0 key_routed_count=0/' 'a seat that routed nothing between the unplug and the return'
reject_mutation events.0.log 's/status=summary fallbacks=0/status=summary fallbacks=2/' 'a seat that ran on class-identity fallbacks'
reject_mutation outcome 's/exit_status=0/exit_status=1/' 'a session that did not exit cleanly'
reject_mutation lifecycle.log 's/emergency=false/emergency=true/' 'a session that ended through recovery'
reject_mutation health 's/storage_errors=0/storage_errors=1/' 'a record store with storage errors'
reject_mutation manifest '/^release_commit=/d' 'a manifest without a release commit'

echo "keyboard independence session verifier check passed"

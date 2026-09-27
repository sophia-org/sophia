#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$root/tools/lib/session_profile.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
export SOPHIA_PREFLIGHT_CALLS="$work/calls"
printf 'schema 1\n' >"$work/profile.kdl"
cat >"$work/sophia" <<'STUB'
#!/usr/bin/env bash
printf 'engine:%s\n' "$*" >>"$SOPHIA_PREFLIGHT_CALLS"
[[ "$1" == config && "$2" == check-session-profile ]] || exit 99
[[ "$4" == --policy-checker=* || "$4" == --allow-deferred-policy ]] || exit 98
if [[ "${SOPHIA_TEST_OLD_BINARY:-0}" != 1 ]]; then
    echo "sophia_session_profile_preflight schema=1 status=accepted policy=${SOPHIA_TEST_POLICY:-validated}"
fi
exit "${SOPHIA_TEST_ENGINE_STATUS:-0}"
STUB
cat >"$work/checker" <<'STUB'
#!/usr/bin/env bash
printf 'wm:%s\n' "$*" >>"$SOPHIA_PREFLIGHT_CALLS"
exit "${SOPHIA_TEST_WM_STATUS:-0}"
STUB
chmod 700 "$work/sophia" "$work/checker"
sophia_check_session_profile "$work/sophia" "$work/profile.kdl" "$work/checker"
[[ "$(wc -l <"$work/calls")" == 1 ]]
if SOPHIA_TEST_POLICY=deferred sophia_check_session_profile "$work/sophia" "$work/profile.kdl" "$work/checker"; then
    echo 'Required validation silently deferred' >&2; exit 1
fi
SOPHIA_TEST_POLICY=deferred sophia_check_session_profile "$work/sophia" "$work/profile.kdl" --deferred
if sophia_check_session_profile "$work/sophia" "$work/profile.kdl" ''; then
    echo 'Missing validation choice was accepted' >&2; exit 1
fi
# Role selection, policy rejection, private staging and timeout are exercised
# against the real binary by the session_profile_preflight Rust tests.
if SOPHIA_TEST_OLD_BINARY=1 sophia_check_session_profile "$work/sophia" "$work/profile.kdl" "$work/checker"; then
    echo 'An unsupported preflight operation returned zero and was accepted' >&2; exit 1
fi
: >"$work/calls"
if SOPHIA_TEST_ENGINE_STATUS=1 sophia_check_session_profile "$work/sophia" "$work/profile.kdl" "$work/checker"; then
    echo 'Engine rejection was ignored' >&2; exit 1
fi
[[ "$(wc -l <"$work/calls")" == 1 ]]
# The product TTY adapter and its refusal-before-handoff test live in
# niltempus. Sophia's real parser/checker behavior stays in
# crates/sophia-cli/tests/session_profile_preflight.rs.
echo 'Session profile preflight checks passed'

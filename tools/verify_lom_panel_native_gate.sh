#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! -f "$1" ]]; then
    echo "usage: $0 SESSION_LOG" >&2
    exit 2
fi
log=$1
grep -q '^sophia_live_shell_gpu schema=1 status=granted ' "$log" || {
    echo "native session recorded no shell GPU grant" >&2; exit 1;
}
mapfile -t outputs < <(grep '^sophia_live_shell_content schema=1 status=presented ' "$log" \
    | sed -n 's/.* output=\([0-9][0-9]*\) .*/\1/p' | sort -u)
(( ${#outputs[@]} >= 1 )) || { echo "no shell content reached native presentation" >&2; exit 1; }
for output in "${outputs[@]}"; do
    count=$(grep -c "^sophia_live_shell_content schema=1 status=presented output=$output " "$log" || true)
    (( count >= 2 )) || { echo "output $output did not present two panel generations" >&2; exit 1; }
done
if grep -Eq 'runtime_fatal|failure_code=|deadline_exceeded|withdrawn' "$log"; then
    echo "native gate contains a fatal, deadline or withdrawal" >&2
    exit 1
fi
printf 'lom_panel_native_verification schema=1 status=pass outputs=%s native_presentation=true\n' "${#outputs[@]}"

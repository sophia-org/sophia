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
facts=$(grep '^sophia_live_shell_content schema=1 status=outputs ' "$log" | tail -n 1)
[[ "$facts" =~ outputs=([0-9]+) ]] || { echo "native session recorded no output facts" >&2; exit 1; }
expected_outputs=${BASH_REMATCH[1]}
mapfile -t outputs < <(grep '^sophia_live_shell_content schema=1 status=presented ' "$log" \
    | sed -n 's/.* output=\([0-9][0-9]*\) .*/\1/p' | sort -u)
(( expected_outputs > 0 && ${#outputs[@]} == expected_outputs )) || {
    echo "not every admitted output reached native presentation" >&2; exit 1;
}
for output in "${outputs[@]}"; do
    count=$(grep "^sophia_live_shell_content schema=1 status=presented output=$output " "$log" \
        | sed -n 's/.* candidate_generation=\([0-9][0-9]*\) .*/\1/p' | sort -u | wc -l)
    (( count >= 2 )) || { echo "output $output did not present two panel generations" >&2; exit 1; }
done
if grep -Eq 'runtime_fatal|failure_code=|deadline_exceeded|withdrawn' "$log"; then
    echo "native gate contains a fatal, deadline or withdrawal" >&2
    exit 1
fi
printf 'lom_panel_native_verification schema=1 status=pass outputs=%s native_presentation=true\n' "${#outputs[@]}"

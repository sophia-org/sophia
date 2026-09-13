#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ! -f "$1" ]]; then
    echo "usage: $0 SESSION_LOG" >&2
    exit 2
fi
log=$1
records=$(mktemp)
trap 'rm -f "$records"' EXIT
# A native diagnostic directory stores approved records after three tab-separated
# sequence/time fields. Keep accepting plain fixture logs so the verifier itself
# remains testable without starting a session.
awk -F '\t' 'NF >= 4 { print $4; next } { print }' "$log" > "$records"

grep -q '^sophia_live_shell_gpu schema=1 status=granted ' "$records" || {
    echo "native session recorded no shell GPU grant" >&2; exit 1;
}
[[ "$(grep -c '^sophia_live_shell_gpu schema=1 status=granted ' "$records")" -eq 1 ]] || {
    echo "native session restarted or replaced the shell GPU grant" >&2; exit 1;
}
facts=$(grep '^sophia_live_shell_content schema=1 status=outputs ' "$records" | tail -n 1)
[[ "$facts" =~ outputs=([0-9]+) ]] || { echo "native session recorded no output facts" >&2; exit 1; }
expected_outputs=${BASH_REMATCH[1]}
mapfile -t outputs < <(grep '^sophia_live_shell_content schema=1 status=presented ' "$records" \
    | sed -n 's/.* output=\([0-9][0-9]*\) .*/\1/p' | sort -u)
(( expected_outputs > 0 && ${#outputs[@]} == expected_outputs )) || {
    echo "not every admitted output reached native presentation" >&2; exit 1;
}
for output in "${outputs[@]}"; do
    count=$(grep "^sophia_live_shell_content schema=1 status=presented output=$output " "$records" \
        | sed -n 's/.* candidate_generation=\([0-9][0-9]*\) .*/\1/p' | sort -u | wc -l)
    (( count >= 2 )) || { echo "output $output did not present two panel generations" >&2; exit 1; }
done
if grep -Eq 'runtime_fatal|failure_code=|deadline_exceeded|withdrawn|sophia_live_wm_configuration schema=1 status=rejected|sophia_live_metadata_shell schema=1 status=unavailable|sophia_live_shell_gpu schema=1 status=(denied|revoked)|sophia_live_shell_content schema=1 status=(transport_failed|presentation_failed)' "$records"; then
    echo "native gate contains a fatal, deadline or withdrawal" >&2
    exit 1
fi
printf 'lom_panel_native_verification schema=1 status=pass outputs=%s native_presentation=true\n' "${#outputs[@]}"

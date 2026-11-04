#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
model_dir="$root_dir/validation/specula/native-frame-transaction/spec"
source "$root_dir/tools/lib/specula_tlc.sh"
specula_require_tlc
specula_check_model "$model_dir" MC.tla MC.cfg MC.tla Mirror.cfg \
    MC.tla TwoCredits.cfg MC.tla Liveness.cfg MC.tla Reopen.cfg

log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT
while read -r config invariant; do
    if specula_check_model "$model_dir" MC.tla "$config.cfg" >"$log_file" 2>&1; then
        echo "Mutation unexpectedly passed: $config" >&2
        exit 1
    fi
    if ! grep -Fq "Invariant $invariant is violated" "$log_file"; then
        cat "$log_file" >&2
        echo "Mutation did not reach its expected invariant: $config" >&2
        exit 1
    fi
    echo "$config: expected $invariant violation"
done <<'MUTATIONS'
CreditMutation CreditSafe
SourceMutation SourceSafe
SceneMutation SceneSafe
AuthorityMutation AuthoritySafe
RetirementMutation RetirementSafe
CancelMutation PhysicalSettlement
StreamMutation StreamBound
ReopenMutation ReopenSafe
MUTATIONS

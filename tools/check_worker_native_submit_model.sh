#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
model_dir="$root_dir/validation/specula/worker-native-submit/spec"
source "$root_dir/tools/lib/specula_tlc.sh"
specula_require_tlc
specula_check_model "$model_dir" WorkerSubmit.tla Safety.cfg

log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT
while read -r config invariant; do
    if specula_check_model "$model_dir" WorkerSubmit.tla "$config.cfg" >"$log_file" 2>&1; then
        echo "Mutation unexpectedly passed: $config" >&2
        exit 1
    fi
    if ! grep -Fq "Invariant $invariant is violated" "$log_file"; then
        cat "$log_file" >&2
        exit 1
    fi
    echo "$config: expected $invariant violation"
done <<'MUTATIONS'
Unserialized FreshSubmit
LostReply PhysicalCustody
WorkerStop PhysicalCustody
LateLedger SubmittedAccounted
LateCard CompletionArmed
EarlyRelease PhysicalCustody
ForeignCompletion ExactCompletion
MUTATIONS

# This is an open progress obligation of the protected design, not acceptance.
if specula_check_model "$model_dir" WorkerSubmit.tla WithdrawalProgress.cfg >"$log_file" 2>&1; then
    echo "Progress probe changed: review the withdrawal obligation" >&2
    exit 1
fi
if ! grep -Fq 'Temporal property WithdrawalEventuallyApplied was violated' "$log_file"; then
    cat "$log_file" >&2
    exit 1
fi
echo 'WithdrawalProgress: expected temporal violation; T4 implementation NOT qualified'

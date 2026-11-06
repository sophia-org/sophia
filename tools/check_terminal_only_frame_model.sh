#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
model_dir="$root_dir/validation/specula/native-frame-transaction/spec"
source "$root_dir/tools/lib/specula_tlc.sh"
specula_require_tlc

# This checks a proposed mode; the existing r13 model gate is unchanged.
for config in TerminalOnly TerminalOnlyMirror TerminalOnlyTwoCredits \
    TerminalOnlyLiveness TerminalOnlyReopen; do
    specula_check_model "$model_dir" TerminalOnly.tla "$config.cfg"
done
specula_check_model "$model_dir" \
    TerminalOnlyIntake.tla TerminalOnlyIntake.cfg \
    TerminalOnlyIntake.tla TerminalOnlyIntakeProgress.cfg

log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT
while read -r config invariant; do
    if specula_check_model "$model_dir" TerminalOnly.tla "$config.cfg" >"$log_file" 2>&1; then
        echo "Mutation unexpectedly passed: $config" >&2
        exit 1
    fi
    if ! grep -Fq "Invariant $invariant is violated" "$log_file"; then
        cat "$log_file" >&2
        exit 1
    fi
    echo "$config: expected $invariant violation"
done <<'MUTATIONS'
TerminalOnlyCreditMutation CreditSafe
TerminalOnlyWireMutation NoPreparedEvents
TerminalOnlyOrderMutation PreparedBeforeSubmit
MUTATIONS

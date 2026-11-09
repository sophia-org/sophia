#!/usr/bin/env bash
set -euo pipefail
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$root_dir/tools/lib/specula_tlc.sh"
specula_require_tlc
model_dir="$root_dir/validation/tla/sft-control-carrier"
specula_check_model "$model_dir" Carrier.tla Safety.cfg Wake.tla WakeSafe.cfg
log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT
while read -r module config expected; do
    if specula_check_model "$model_dir" "$module.tla" "$config.cfg" >"$log_file" 2>&1; then
        echo "Control unexpectedly passed: $config" >&2
        exit 1
    fi
    if ! grep -Fq "$expected" "$log_file"; then
        cat "$log_file" >&2
        exit 1
    fi
    echo "$config: expected $expected"
done <<'CONTROLS'
Carrier ReserveReply Invariant CapacitySafe is violated
Carrier FreshAuthority Invariant AuthoritySafe is violated
Carrier ExactReceipt Invariant KnownIsReal is violated
Carrier SnapshotInput Invariant ImmutableAcceptance is violated
Carrier SemanticAck Invariant AckBeforeReuse is violated
Wake WakeLateClear Temporal property Progress was violated
CONTROLS

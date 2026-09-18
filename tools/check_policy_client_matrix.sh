#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HAGIA_ROOT="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"

[[ -f "$HAGIA_ROOT/hagia.nimble" ]] || {
    echo "SOPHIA_HAGIA_ROOT must name the independent Hagia checkout" >&2
    exit 2
}

cd "$ROOT_DIR"
tools/check_policy_protocol.sh
cargo test --offline -q -p sophia-wm-demo

cd "$HAGIA_ROOT"
SOPHIA_ROOT="$ROOT_DIR" nimble test -y

printf '%s\n' \
    'sophia_policy_client_matrix schema=8 status=complete public_wire_clients=rust,c,hagia behavior_scenarios=11 sequential=true reconnect_restart=true revision_freeze=false'

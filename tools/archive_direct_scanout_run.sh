#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence="${1:?usage: archive_direct_scanout_run.sh EVIDENCE}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
run_root="${SOPHIA_DIRECT_SCANOUT_RUN_ROOT:-$state_home/sophia/promotion/direct-scanout-runs}"
sophia_bin="${SOPHIA_DIRECT_SCANOUT_SOPHIA_BIN:-$ROOT_DIR/target/release/sophia}"
client_bin="${SOPHIA_DIRECT_SCANOUT_CLIENT_BIN:?set the executable whose identity the evidence binds}"
cd "$ROOT_DIR"
exec cargo --offline --locked --quiet xtask conformance archive direct-scanout "$evidence" "$run_root" "$sophia_bin" "$client_bin"

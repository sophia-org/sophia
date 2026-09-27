#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

tree="$(cargo tree -p sophia-cli --features native-session -e normal)"
if grep -qiE 'sophia-x-bridge|xlibre' <<<"$tree"; then
    echo "Production Sophia dependency graph still contains the XLibre bridge." >&2
    exit 1
fi
if grep -qiE 'sophia-x-bridge|xlibre-research' Cargo.toml crates/sophia-cli/Cargo.toml \
    crates/sophia-cli/src/commands.rs; then
    echo "The live workspace still exposes the historical XLibre bridge." >&2
    exit 1
fi
if grep -qE '/usr/libexec/Xorg|xlibre-25|dummy_drv|--client-backend=xlibre-compat|configure_xlibre' \
    tools/run_sophia_session.sh; then
    echo "Installed Sophia launcher still contains an XLibre/Xorg launch dependency." >&2
    exit 1
fi
echo "Sophia live workspace is XLibre-free; historical sources are isolated under research/xlibre."

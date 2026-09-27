#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

fail() {
    echo "legacy-WM bridge audit failed: $*" >&2
    exit 1
}

[[ ! -e crates/sophia-x11-wm-bridge ]] ||
    fail "the removed bridge crate still exists"
if rg -n 'sophia-x11-wm-bridge' Cargo.toml Cargo.lock crates; then
    fail "a production crate still references the bridge"
fi

legacy_paths="$({
    find tools -type f \
        \( -name '*xmonad*' -o -name '*x11-wm-bridge*' \)
} || true)"
[[ -z "$legacy_paths" ]] || {
    printf '%s\n' "$legacy_paths" >&2
    fail "legacy Sophia compatibility tooling remains"
}

# Exact negative checks are allowed to name the removed artifact. Everything
# else in active launch/package tooling must be unable to select or execute it.
active_hits="$({
    rg -n \
        'SOPHIA_X11_WM_BRIDGE_BIN|SOPHIA_LEGACY_X11_WM|run_sophia_xmonad_session|--profile=xmonad|SOPHIA_TTY_PROFILE=xmonad|sophia-x11-wm-bridge' \
        tools crates examples Cargo.toml Cargo.lock \
        --glob '!check_no_legacy_wm_bridge.sh'
} || true)"
[[ -z "$active_hits" ]] || {
    printf '%s\n' "$active_hits" >&2
    fail "an active product path can still select the bridge"
}

echo "legacy-WM bridge is absent from the native Sophia product surface"

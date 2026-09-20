#!/usr/bin/env bash
# Binds a clean, signed Sophia commit and the exact release binary, then hands
# the keyboard-independence gate to the tty launcher, which takes the display.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [[ ! -t 0 || "$(tty)" != /dev/tty4 ]]; then
    echo "Switch to tty4 with Ctrl+Alt+F4, log in, and run:" >&2
    echo "  SOPHIA_KEYBOARD_A=/dev/input/by-id/...-event-kbd SOPHIA_KEYBOARD_B=/dev/input/by-id/...-event-kbd \\" >&2
    echo "  $ROOT_DIR/tools/run_keyboard_independence_gate_tty4.sh" >&2
    exit 1
fi
if [[ -z "${SOPHIA_KEYBOARD_A:-}" || -z "${SOPHIA_KEYBOARD_B:-}" ]]; then
    echo "set SOPHIA_KEYBOARD_A (the keyboard you will unplug) and SOPHIA_KEYBOARD_B" >&2
    echo "available keyboard paths:" >&2
    find /dev/input/by-id /dev/input/by-path -maxdepth 1 -type l -name '*-event-kbd' -print 2>/dev/null >&2 || true
    exit 1
fi
if [[ -n "$(git -C "$ROOT_DIR" status --short)" ]]; then
    echo "Sophia worktree must be clean before the physical proof." >&2
    exit 1
fi

sophia_commit="$(git -C "$ROOT_DIR" rev-parse HEAD)"
git -C "$ROOT_DIR" verify-commit "$sophia_commit" >/dev/null 2>&1 || {
    echo "Physical-proof HEAD lacks a valid signature: $ROOT_DIR" >&2
    exit 1
}

echo "Building the exact physical-proof binary before DRM takeover..."
echo "Sophia: $sophia_commit"
(
    cd "$ROOT_DIR"
    cargo build --quiet --release --offline -p sophia-cli \
        --features native-session
)

if [[ -n "$(git -C "$ROOT_DIR" status --short)" \
    || "$(git -C "$ROOT_DIR" rev-parse HEAD)" != "$sophia_commit" ]]; then
    echo "Sophia source identity changed during the physical-proof build." >&2
    exit 1
fi
git -C "$ROOT_DIR" verify-commit "$sophia_commit" >/dev/null 2>&1 || {
    echo "Sophia signature no longer verifies after the build." >&2
    exit 1
}

sophia_bin="$ROOT_DIR/target/release/sophia"
sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
echo "Sophia binary: $sophia_sha256"

export SOPHIA_TTY_PROFILE=keyboard-independence
export SOPHIA_TTY_NUMBER=4
export SOPHIA_KEYBOARD_INDEPENDENCE_ARM=1
export SOPHIA_KEYBOARD_INDEPENDENCE_SEAT="${SOPHIA_KEYBOARD_INDEPENDENCE_SEAT:-seat0}"
export SOPHIA_KEYBOARD_INDEPENDENCE_SOURCE_COMMIT="$sophia_commit"
export SOPHIA_KEYBOARD_INDEPENDENCE_SOPHIA_SHA256="$sophia_sha256"
export SOPHIA_LIVE_SESSION_SKIP_BUILD=1
exec "$ROOT_DIR/tools/start_sophia_tty3.sh"

#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOM_SOURCE="${SOPHIA_LOM_SOURCE:-/home/niltempus/dev/lom}"
LOM_TARGET="${SOPHIA_LOM_TARGET_DIR:-$HOME/.cache/lom-target}"
LOM_CONFIG="${SOPHIA_LOM_CONFIG:-$LOM_SOURCE/examples/minimal/live-shell.kdl}"
HAGIA_BIN="${SOPHIA_HAGIA_BIN:-/home/niltempus/dev/hagia/hagia}"
LOM_CORE_CONFIG="${SOPHIA_LOM_CORE_CONFIG:-$ROOT_DIR/tools/fixtures/lom_panel_core.kdl}"
EVIDENCE_DIR="${SOPHIA_LOM_NATIVE_EVIDENCE_DIR:-$ROOT_DIR/.artifacts/lom-panel-native/$(date -u +%Y%m%dT%H%M%SZ)}"

[[ "$(tty)" == /dev/tty4 ]] || { echo "Run this from /dev/tty4 after ending the graphical session." >&2; exit 2; }
[[ "${SOPHIA_LOM_NATIVE_GATE_ARM:-0}" == 1 ]] || { echo "Set SOPHIA_LOM_NATIVE_GATE_ARM=1 to run the native panel gate." >&2; exit 2; }
[[ -z "$(git -C "$ROOT_DIR" status --short)" ]] || { echo "Sophia source must be clean" >&2; exit 2; }
[[ -z "$(git -C "$LOM_SOURCE" status --short)" ]] || { echo "Lom source must be clean" >&2; exit 2; }
git -C "$ROOT_DIR" verify-commit HEAD >/dev/null
git -C "$LOM_SOURCE" verify-commit HEAD >/dev/null
mkdir -p "$EVIDENCE_DIR" "$LOM_TARGET"
chmod 700 "$EVIDENCE_DIR"
cargo build --offline --release -p sophia-cli --features native-session --manifest-path "$ROOT_DIR/Cargo.toml"
CARGO_TARGET_DIR="$LOM_TARGET" cargo build --offline --release --manifest-path "$LOM_SOURCE/Cargo.toml"
LOM_BIN="$LOM_TARGET/release/lom"
SOPHIA_BIN="$ROOT_DIR/target/release/sophia"
{
    printf 'sophia_commit=%s\n' "$(git -C "$ROOT_DIR" rev-parse HEAD)"
    printf 'sophia_binary_sha256=%s\n' "$(sha256sum "$SOPHIA_BIN" | cut -d' ' -f1)"
    printf 'lom_commit=%s\n' "$(git -C "$LOM_SOURCE" rev-parse HEAD)"
    printf 'lom_binary_sha256=%s\n' "$(sha256sum "$LOM_BIN" | cut -d' ' -f1)"
    printf 'lom_config_sha256=%s\n' "$(sha256sum "$LOM_CONFIG" | cut -d' ' -f1)"
    printf 'hagia_binary_sha256=%s\n' "$(sha256sum "$HAGIA_BIN" | cut -d' ' -f1)"
} > "$EVIDENCE_DIR/identity.manifest"

echo "Evidence: $EVIDENCE_DIR"
echo "Checking Lom's protected GPU and content path before graphics takeover."
SOPHIA_LOM_GPU_PROOF_ARM=1 \
SOPHIA_LOM_GPU_EVIDENCE_DIR="$EVIDENCE_DIR/gpu-content" \
SOPHIA_LOM_SOURCE="$LOM_SOURCE" \
SOPHIA_LOM_TARGET_DIR="$LOM_TARGET" \
SOPHIA_LOM_CONFIG="$LOM_CONFIG" \
SOPHIA_LOM_GPU_RENDER_NODE="${SOPHIA_LOM_GPU_RENDER_NODE:-/dev/dri/renderD128}" \
    "$ROOT_DIR/tools/lom_gpu_content_hardware_proof.sh"

echo "The gate runs for 20 seconds. Confirm a Minimal bar on every output, workspace pills on the left, and a seconds clock on the right."
set +e
SOPHIA_BIN="$SOPHIA_BIN" \
SOPHIA_HAGIA_BIN="$HAGIA_BIN" \
SOPHIA_HAGIA_SHELL_BIN="$LOM_BIN" \
SOPHIA_SHELL_CONFIG="$LOM_CONFIG" \
SOPHIA_BUILD_SESSION=false \
SOPHIA_MANAGE_KEYD=true \
SOPHIA_REQUIRE_LOCAL_VT=true \
SOPHIA_TTY_PROFILE=hagia \
SOPHIA_CORE_CONFIG="$LOM_CORE_CONFIG" \
SOPHIA_DESKTOP_PROFILE="$ROOT_DIR/tools/fixtures/lom_panel_desktop.kdl" \
SOPHIA_SESSION_STARTUP=none \
SOPHIA_SESSION_WATCHDOG_SECONDS=20 \
SOPHIA_DIAGNOSTIC_DIR="$EVIDENCE_DIR/session" \
    "$ROOT_DIR/tools/run_sophia_session.sh"
native_status=$?
set -e
if [[ "$native_status" -ne 124 ]]; then
    echo "Native panel session ended unexpectedly with status $native_status" >&2
    exit 1
fi
grep -q '^sophia_tty_recovery schema=3 .*termios_restored=true ' \
    "$EVIDENCE_DIR/session/recovery.log" \
    && grep -q '^sophia_tty_recovery_verification schema=1 .*keyd_restored=true$' \
        "$EVIDENCE_DIR/session/recovery.log" || {
    echo "Native panel session did not prove complete TTY and keyd recovery" >&2
    exit 1
}

"$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$EVIDENCE_DIR/session/events.0.log" \
    | tee "$EVIDENCE_DIR/verification.log"

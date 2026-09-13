#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOM_SOURCE="${SOPHIA_LOM_SOURCE:-/home/niltempus/dev/lom}"
LOM_CONFIG="${SOPHIA_LOM_CONFIG:-$LOM_SOURCE/examples/minimal/live-shell.kdl}"
SEAT="${SOPHIA_LOM_GPU_SEAT:-seat0}"
RENDER_NODE="${SOPHIA_LOM_GPU_RENDER_NODE:-/dev/dri/renderD128}"
EVIDENCE_DIR="${SOPHIA_LOM_GPU_EVIDENCE_DIR:-$ROOT_DIR/.artifacts/lom-gpu-content-proof/$(date -u +%Y%m%dT%H%M%SZ)}"
LOM_TARGET="${SOPHIA_LOM_TARGET_DIR:-$HOME/.cache/lom-target}"
LOG="$EVIDENCE_DIR/proof.log"

[[ "${SOPHIA_LOM_GPU_PROOF_ARM:-0}" == 1 ]] || {
    echo "Set SOPHIA_LOM_GPU_PROOF_ARM=1 to use the admitted render node." >&2
    exit 2
}
[[ -d "$LOM_SOURCE/.git" && -f "$LOM_CONFIG" ]] || { echo "Lom source/config missing" >&2; exit 2; }
[[ -c "$RENDER_NODE" ]] || { echo "Render node is not a character device: $RENDER_NODE" >&2; exit 2; }
[[ -z "$(git -C "$ROOT_DIR" status --short)" ]] || { echo "Sophia source must be clean" >&2; exit 2; }
[[ -z "$(git -C "$LOM_SOURCE" status --short)" ]] || { echo "Lom source must be clean" >&2; exit 2; }
git -C "$ROOT_DIR" verify-commit HEAD >/dev/null
git -C "$LOM_SOURCE" verify-commit HEAD >/dev/null

mkdir -p "$EVIDENCE_DIR" "$LOM_TARGET"
chmod 700 "$EVIDENCE_DIR"
cargo build --offline --release -p sophia-cli --features native-session --manifest-path "$ROOT_DIR/Cargo.toml"
CARGO_TARGET_DIR="$LOM_TARGET" cargo build --offline --release --manifest-path "$LOM_SOURCE/Cargo.toml"
SOPHIA_BIN="$ROOT_DIR/target/release/sophia"
LOM_BIN="$LOM_TARGET/release/lom"
{
    printf 'sophia_commit=%s\n' "$(git -C "$ROOT_DIR" rev-parse HEAD)"
    printf 'sophia_binary_sha256=%s\n' "$(sha256sum "$SOPHIA_BIN" | cut -d' ' -f1)"
    printf 'lom_commit=%s\n' "$(git -C "$LOM_SOURCE" rev-parse HEAD)"
    printf 'lom_binary_sha256=%s\n' "$(sha256sum "$LOM_BIN" | cut -d' ' -f1)"
    printf 'lom_config_sha256=%s\n' "$(sha256sum "$LOM_CONFIG" | cut -d' ' -f1)"
    printf 'seat=%s\nrender_node=%s\n' "$SEAT" "$RENDER_NODE"
} > "$EVIDENCE_DIR/identity.manifest"

echo "Evidence: $EVIDENCE_DIR"
env -u DISPLAY -u WAYLAND_DISPLAY -u WAYLAND_SOCKET \
    "$SOPHIA_BIN" sophia-shell-gpu-content-hardware-proof \
    "--client=$LOM_BIN" "--config=$LOM_CONFIG" "--seat=$SEAT" \
    "--render-node=$RENDER_NODE" 2>&1 | tee "$LOG"
"$ROOT_DIR/tools/verify_lom_gpu_content_hardware_proof.sh" "$LOG" | tee "$EVIDENCE_DIR/verification.log"

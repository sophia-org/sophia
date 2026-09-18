#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REMOTE_HOST="${SOPHIA_REMOTE_HOST:-}"
REMOTE_DIR="${SOPHIA_REMOTE_DIR:-dev/sophia-target}"
EVIDENCE_DIR="${SOPHIA_REMOTE_EVIDENCE_DIR:-$ROOT_DIR/.evidence/remote-target}"

usage() {
    cat <<EOF
Usage: SOPHIA_REMOTE_HOST=HOST tools/remote_target.sh COMMAND

Commands:
  sync            Copy source without Git metadata or generated artifacts.
  build           Build the live release binary on the remote target.
  qemu            Sync and run the two-xterm QEMU integration gate remotely.
  stage           Sync and build, then print the command for the target's TTY.
  status          Report graphical sessions, Sophia processes, DRM, and Git state.
  fetch-evidence  Retrieve Sophia logs into ignored local evidence storage.

Environment:
  SOPHIA_REMOTE_HOST          required SSH host or alias
  SOPHIA_REMOTE_DIR           deployment path (default: dev/sophia-target)
  SOPHIA_REMOTE_EVIDENCE_DIR  local evidence destination
EOF
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "Missing required command: $1" >&2
        exit 1
    fi
}

validate_configuration() {
    if [[ -z "$REMOTE_HOST" ]]; then
        echo "Set SOPHIA_REMOTE_HOST to an SSH host or alias." >&2
        exit 2
    fi
    if [[ ! "$REMOTE_DIR" =~ ^[A-Za-z0-9._/-]+$ || "$REMOTE_DIR" == /* ]]; then
        echo "SOPHIA_REMOTE_DIR must be a shell-safe path below the remote home." >&2
        exit 2
    fi
}

remote_repo() {
    local command="$1"
    ssh "$REMOTE_HOST" "cd -- \"\$HOME/$REMOTE_DIR\" && $command"
}

sync_tree() {
    require_command rsync
    require_command ssh
    ssh "$REMOTE_HOST" "mkdir -p -- \"\$HOME/$REMOTE_DIR\""
    rsync -a \
        --exclude /.git/ \
        --exclude /target/ \
        --exclude /.qemu/ \
        --exclude /.evidence/ \
        "$ROOT_DIR/" "$REMOTE_HOST:$REMOTE_DIR/"
}

build_live() {
    require_command ssh
    remote_repo \
        "cargo build --release --offline -p sophia-cli --features native-session"
}

fetch_evidence() {
    require_command rsync
    mkdir -p "$EVIDENCE_DIR/tmp" "$EVIDENCE_DIR/state"
    rsync -a --ignore-missing-args \
        "$REMOTE_HOST:/tmp/sophia-*.log" "$EVIDENCE_DIR/tmp/"
    rsync -a --ignore-missing-args \
        "$REMOTE_HOST:.local/state/sophia/" "$EVIDENCE_DIR/state/"
    echo "Remote evidence copied to $EVIDENCE_DIR"
}

command="${1:-}"
if [[ "$command" == -h || "$command" == --help || "$command" == help ]]; then
    usage
    exit 0
fi
validate_configuration

case "$command" in
    sync) sync_tree ;;
    build) build_live ;;
    qemu)
        sync_tree
        remote_repo \
            "tools/build_qemu_session_initramfs.sh && SOPHIA_QEMU_TWO_XTERM=1 tools/qemu_session_harness.sh"
        ;;
    stage)
        sync_tree
        build_live
        echo
        echo "Physical proof staged on $REMOTE_HOST."
        echo "At the target's dedicated local text TTY, with its graphical session stopped:"
        echo "  cd ~/$REMOTE_DIR"
        echo "  tools/run_current_hagia_native_gate_tty4.sh"
        echo
        echo "Retrieve logs afterward with:"
        echo "  SOPHIA_REMOTE_HOST=$REMOTE_HOST tools/remote_target.sh fetch-evidence"
        ;;
    status)
        require_command ssh
        remote_repo "tools/remote_target_status.sh"
        ;;
    fetch-evidence) fetch_evidence ;;
    *)
        usage >&2
        exit 2
        ;;
esac

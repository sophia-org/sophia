#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
run_root="${SOPHIA_KEYBOARD_INDEPENDENCE_RUN_ROOT:-$state_home/sophia/promotion/keyboard-independence-runs}"
run="${1:-}"
if [[ -z "$run" ]]; then
    run="$(find "$run_root" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort -V | tail -n 1 || true)"
fi
[[ -n "$run" && -s "$run/SHA256SUMS" ]] || {
    echo "keyboard independence archive is missing: ${run:-$run_root}" >&2
    exit 1
}
(
    cd "$run"
    sha256sum -c --status SHA256SUMS
) || {
    echo "keyboard independence archive checksum verification failed: $run" >&2
    exit 1
}
[[ "$(sed -n 's/^record_kind=//p' "$run/manifest")" == keyboard_independence_physical ]] || {
    echo "keyboard independence archive has the wrong record kind: $run" >&2
    exit 1
}
[[ "$(cat "$run/result.kdl")" == 'sophia_keyboard_independence_physical schema=1 status=passed' ]] || {
    echo "keyboard independence archive is not passing: $run" >&2
    exit 1
}
for key in source_commit proof_text session_sha256 guard_seat_sha256 guard_pinned_sha256 sophia_binary_sha256; do
    [[ "$(grep -c "^${key}=" "$run/manifest")" == 1 ]] || {
        echo "keyboard independence archive has invalid $key cardinality: $run" >&2
        exit 1
    }
done
source_commit="$(sed -n 's/^source_commit=//p' "$run/manifest")"
proof_text="$(sed -n 's/^proof_text=//p' "$run/manifest")"
[[ "$source_commit" =~ ^[0-9a-f]{40}$ ]] &&
    git -C "$ROOT_DIR" cat-file -e "$source_commit^{commit}" || {
    echo "keyboard independence archive has an invalid source commit: $run" >&2
    exit 1
}
git -C "$ROOT_DIR" verify-commit "$source_commit" >/dev/null 2>&1 || {
    echo "keyboard independence archive source commit does not have a valid signature: $run" >&2
    exit 1
}
for pair in session:session.log guard_seat:guard_seat.log guard_pinned:guard_pinned.log; do
    key="${pair%%:*}_sha256"
    file="${pair#*:}"
    [[ "$(sed -n "s/^${key}=//p" "$run/manifest")" == "$(sha256sum "$run/$file" | awk '{ print $1 }')" ]] || {
        echo "keyboard independence $file digest does not match its manifest: $run" >&2
        exit 1
    }
done
identity="$(grep -E '^sophia_keyboard_independence_identity schema=1 status=bound ' "$run/session.log")"
[[ "$(sed -n 's/.* sophia_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")" == "$source_commit" ]] || {
    echo "keyboard independence evidence and manifest name different source commits: $run" >&2
    exit 1
}
[[ "$(sed -n 's/.* sophia_sha256=\([0-9a-f]\{64\}\)$/\1/p' <<<"$identity")" == \
    "$(sed -n 's/^sophia_binary_sha256=//p' "$run/manifest")" ]] || {
    echo "keyboard independence evidence and manifest name different Sophia binaries: $run" >&2
    exit 1
}
"$ROOT_DIR/tools/verify_keyboard_independence_physical.sh" "$run" "$proof_text" >/dev/null

echo "keyboard independence physical archive verified: run=$run commit=$source_commit"

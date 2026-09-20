#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence_dir="${1:?usage: archive_keyboard_independence_physical_run.sh EVIDENCE_DIR [PROOF_TEXT]}"
proof_text="${2:-twokeyboards}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
run_root="${SOPHIA_KEYBOARD_INDEPENDENCE_RUN_ROOT:-$state_home/sophia/promotion/keyboard-independence-runs}"
sophia_bin="${SOPHIA_KEYBOARD_INDEPENDENCE_SOPHIA_BIN:-$ROOT_DIR/target/release/sophia}"

"$ROOT_DIR/tools/verify_keyboard_independence_physical.sh" "$evidence_dir" "$proof_text" >/dev/null
identity="$(grep -E '^sophia_keyboard_independence_identity schema=1 status=bound ' "$evidence_dir/session.log")"
source_commit="$(sed -n 's/.* sophia_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")"
recorded_sophia_sha256="$(sed -n 's/.* sophia_sha256=\([0-9a-f]\{64\}\)$/\1/p' <<<"$identity")"
[[ -n "$source_commit" ]] && git -C "$ROOT_DIR" cat-file -e "$source_commit^{commit}" || {
    echo "keyboard independence evidence has an invalid source commit" >&2
    exit 1
}
git -C "$ROOT_DIR" verify-commit "$source_commit" >/dev/null 2>&1 || {
    echo "keyboard independence evidence source commit does not have a valid signature" >&2
    exit 1
}
[[ -x "$sophia_bin" ]] || { echo "Sophia binary is not executable: $sophia_bin" >&2; exit 1; }
sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
[[ "$sophia_sha256" == "$recorded_sophia_sha256" ]] || {
    echo "Sophia binary no longer matches the verified run" >&2
    exit 1
}

session_sha256="$(sha256sum "$evidence_dir/session.log" | awk '{ print $1 }')"
install -d -m 700 "$run_root"
if grep -rlFx --include=manifest "session_sha256=$session_sha256" "$run_root" 2>/dev/null | grep -q .; then
    echo "keyboard independence evidence is already archived" >&2
    exit 1
fi
sequence=1
while true; do
    run_dir="$run_root/$(printf '%04d' "$sequence")"
    if mkdir -m 700 "$run_dir" 2>/dev/null; then
        break
    fi
    sequence=$((sequence + 1))
done
trap 'rm -rf -- "$run_dir"' ERR HUP INT TERM

for name in session.log guard_seat.log guard_pinned.log; do
    install -m 600 "$evidence_dir/$name" "$run_dir/$name"
done
printf '%s\n' 'sophia_keyboard_independence_physical schema=1 status=passed' >"$run_dir/result.kdl"
printf 'record_schema=1\nrecord_kind=keyboard_independence_physical\nrecorded_at_utc=%s\nsource_commit=%s\nproof_text=%s\nsession_sha256=%s\nguard_seat_sha256=%s\nguard_pinned_sha256=%s\nsophia_binary_sha256=%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$source_commit" "$proof_text" "$session_sha256" \
    "$(sha256sum "$evidence_dir/guard_seat.log" | awk '{ print $1 }')" \
    "$(sha256sum "$evidence_dir/guard_pinned.log" | awk '{ print $1 }')" \
    "$sophia_sha256" >"$run_dir/manifest"
chmod 600 "$run_dir/manifest" "$run_dir/result.kdl"
(
    cd "$run_dir"
    sha256sum manifest result.kdl session.log guard_seat.log guard_pinned.log >SHA256SUMS
)
chmod 600 "$run_dir/SHA256SUMS"
"$ROOT_DIR/tools/verify_keyboard_independence_physical_archive.sh" "$run_dir" >/dev/null
trap - ERR HUP INT TERM
echo "Recorded verified keyboard independence physical run: $run_dir"

#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence="${1:?usage: archive_hagia_policy_physical_run.sh EVIDENCE [PROOF_TEXT]}"
proof_text="${2:-hagiapolicyproof}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
run_root="${SOPHIA_HAGIA_POLICY_RUN_ROOT:-$state_home/sophia/promotion/hagia-policy-runs}"
sophia_bin="${SOPHIA_HAGIA_POLICY_SOPHIA_BIN:-$ROOT_DIR/target/release/sophia}"
hagia_bin="${SOPHIA_HAGIA_BIN:-/opt/sophia/current/target/release/hagia}"
hagia_shell_bin="${SOPHIA_HAGIA_SHELL_BIN:-/opt/sophia/current/target/release/narthex}"
hagia_root="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
narthex_root="${SOPHIA_NARTHEX_ROOT:-$ROOT_DIR/../narthex}"

"$ROOT_DIR/tools/verify_hagia_policy_physical.sh" "$evidence" "$proof_text" >/dev/null
identity="$(grep -E '^sophia_hagia_policy_identity schema=3 status=bound ' "$evidence")"
source_commit="$(sed -n 's/.* sophia_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")"
hagia_commit="$(sed -n 's/.* hagia_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")"
narthex_commit="$(sed -n 's/.* narthex_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")"
recorded_sophia_sha256="$(sed -n 's/.* sophia_sha256=\([0-9a-f]\{64\}\) .*/\1/p' <<<"$identity")"
recorded_hagia_sha256="$(sed -n 's/.* hagia_sha256=\([0-9a-f]\{64\}\) .*/\1/p' <<<"$identity")"
recorded_narthex_sha256="$(sed -n 's/.* narthex_sha256=\([0-9a-f]\{64\}\)$/\1/p' <<<"$identity")"
[[ -d "$hagia_root/.git" ]] || {
    echo "Hagia checkout is unavailable: $hagia_root" >&2
    exit 1
}
for repo_and_commit in "$ROOT_DIR:$source_commit" "$hagia_root:$hagia_commit" \
    "$narthex_root:$narthex_commit"; do
    repo="${repo_and_commit%:*}"
    commit="${repo_and_commit##*:}"
    [[ "$commit" =~ ^[0-9a-f]{40}$ ]] && git -C "$repo" cat-file -e "$commit^{commit}" || {
        echo "Hagia physical evidence has an invalid source commit: $repo" >&2
        exit 1
    }
    git -C "$repo" verify-commit "$commit" >/dev/null 2>&1 || {
        echo "Hagia physical evidence source commit lacks a valid signature: $repo" >&2
        exit 1
    }
done
for binary in "$sophia_bin" "$hagia_bin" "$hagia_shell_bin"; do
    [[ -x "$binary" ]] || {
        echo "Hagia physical evidence binary is unavailable: $binary" >&2
        exit 1
    }
done

evidence_sha256="$(sha256sum "$evidence" | awk '{ print $1 }')"
sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
hagia_sha256="$(sha256sum "$hagia_bin" | awk '{ print $1 }')"
narthex_sha256="$(sha256sum "$hagia_shell_bin" | awk '{ print $1 }')"
[[ "$sophia_sha256" == "$recorded_sophia_sha256" ]] || {
    echo "Hagia physical Sophia binary no longer matches the verified run" >&2
    exit 1
}
[[ "$hagia_sha256" == "$recorded_hagia_sha256" ]] || {
    echo "Hagia physical Hagia binary no longer matches the verified run" >&2
    exit 1
}
[[ "$narthex_sha256" == "$recorded_narthex_sha256" ]] || {
    echo "Hagia physical Hagia Shell binary no longer matches the verified run" >&2
    exit 1
}
install -d -m 700 "$run_root"
# CAPTURED RATHER THAN `... | grep -q .`, AND NOT BECAUSE THIS ONE BROKE.
# Under `set -o pipefail` a `grep -q` that matches exits at once and a
# still-running producer upstream dies of SIGPIPE, so the pipeline reports
# failure and a match reads as no match. This site is in fact safe from it:
# `grep -rl` emits only matching paths, far too little to fill its block
# buffer, so it flushes at exit -- after the walk has already finished. That
# is a property of how little this particular producer prints, not of the
# shape, and it is not a property anyone reviewing the line can see. Captured
# so the guard does not rest on it. The same shape over a long log was a live
# false pass in verify_mixed_output_evidence.sh.
archived_already="$(grep -rlFx --include=manifest "evidence_sha256=$evidence_sha256" \
    "$run_root" 2>/dev/null || true)"
if [[ -n "$archived_already" ]]; then
    echo "Hagia physical policy evidence is already archived" >&2
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

install -m 600 "$evidence" "$run_dir/session.log"
printf '%s\n' \
    'sophia_hagia_policy_physical schema=4 status=passed' \
    >"$run_dir/result.kdl"
printf 'record_schema=4\nrecord_kind=hagia_policy_physical\nrecorded_at_utc=%s\nsource_commit=%s\nhagia_commit=%s\nnarthex_commit=%s\nproof_text=%s\nevidence_sha256=%s\nsophia_binary_sha256=%s\nhagia_binary_sha256=%s\nnarthex_binary_sha256=%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$source_commit" "$hagia_commit" "$narthex_commit" "$proof_text" \
    "$evidence_sha256" "$sophia_sha256" "$hagia_sha256" "$narthex_sha256" \
    >"$run_dir/manifest"
chmod 600 "$run_dir/manifest" "$run_dir/result.kdl"
(
    cd "$run_dir"
    sha256sum manifest result.kdl session.log >SHA256SUMS
)
chmod 600 "$run_dir/SHA256SUMS"

(
    cd "$run_dir"
    sha256sum -c --status SHA256SUMS
)
SOPHIA_HAGIA_ROOT="$hagia_root" \
    "$ROOT_DIR/tools/verify_hagia_policy_physical_archive.sh" "$run_dir" >/dev/null
trap - ERR HUP INT TERM
echo "Recorded verified Hagia physical policy run: $run_dir"

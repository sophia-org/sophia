#!/usr/bin/env bash
set -euo pipefail

if (( $# < 3 || $# > 5 )); then
    echo "usage: archive_frame_fed_output_physical_run.sh SUCCESS_LOG ROLLBACK_LOG CONNECTORS [SUCCESS_TEXT] [ROLLBACK_TEXT]" >&2
    exit 2
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
success_log="$1"
rollback_log="$2"
connectors="$3"
success_text="${4:-outputapply}"
rollback_text="${5:-outputrollback}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
run_root="${SOPHIA_FRAME_FED_OUTPUT_RUN_ROOT:-$state_home/sophia/promotion/frame-fed-output-runs}"
sophia_bin="${SOPHIA_FRAME_FED_OUTPUT_SOPHIA_BIN:-$ROOT_DIR/target/release/sophia}"
hagia_bin="${SOPHIA_FRAME_FED_OUTPUT_HAGIA_BIN:-}"
hagia_root="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
core_config="${SOPHIA_FRAME_FED_OUTPUT_CORE_CONFIG:-$ROOT_DIR/tools/config/sophia/core.kdl}"
desktop_profile="${SOPHIA_FRAME_FED_OUTPUT_DESKTOP_PROFILE:-$ROOT_DIR/tools/fixtures/frame_fed_output_proof.kdl}"

"$ROOT_DIR/tools/verify_frame_fed_output_evidence.sh" \
    "$success_log" "$rollback_log" "$success_text" "$rollback_text" >/dev/null
[[ -s "$connectors" ]] || {
    echo "frame-fed output connector facts are missing" >&2
    exit 1
}
for binary in "$sophia_bin" "$hagia_bin"; do
    [[ -n "$binary" && -x "$binary" ]] || {
        echo "frame-fed output archive binary is unavailable: ${binary:-<unset>}" >&2
        exit 1
    }
done
[[ -d "$hagia_root/.git" ]] || {
    echo "Hagia checkout is unavailable: $hagia_root" >&2
    exit 1
}

identity="$(grep -E '^sophia_frame_fed_output_gate schema=1 status=phase_started phase=success ' "$success_log")"
field() {
    local key="$1"
    sed -n "s/.* ${key}=\([^ ]*\).*/\1/p" <<<"$identity"
}
source_commit="$(field source_commit)"
hagia_commit="$(field hagia_commit)"
recorded_sophia_sha256="$(field sophia_sha256)"
recorded_hagia_sha256="$(field hagia_sha256)"
recorded_core_sha256="$(field core_sha256)"
recorded_profile_sha256="$(field profile_sha256)"
recorded_connectors_sha256="$(field connectors_sha256)"

for repo_and_commit in "$ROOT_DIR:$source_commit" "$hagia_root:$hagia_commit"; do
    repo="${repo_and_commit%:*}"
    commit="${repo_and_commit##*:}"
    [[ "$commit" =~ ^[0-9a-f]{40}$ ]] && git -C "$repo" cat-file -e "$commit^{commit}" \
        || { echo "frame-fed output evidence has an invalid source commit: $repo" >&2; exit 1; }
    git -C "$repo" verify-commit "$commit" >/dev/null 2>&1 \
        || { echo "frame-fed output source commit lacks a valid signature: $repo" >&2; exit 1; }
done

tracked_path() {
    local file="$1" path
    path="$(realpath --relative-to="$ROOT_DIR" "$file")"
    [[ "$path" != ../* && "$path" != /* ]] || return 1
    git -C "$ROOT_DIR" ls-files --error-unmatch "$path" >/dev/null 2>&1 || return 1
    printf '%s\n' "$path"
}
core_config_path="$(tracked_path "$core_config")" \
    || { echo "frame-fed output core configuration must be tracked by Sophia" >&2; exit 1; }
desktop_profile_path="$(tracked_path "$desktop_profile")" \
    || { echo "frame-fed output desktop profile must be tracked by Sophia" >&2; exit 1; }

sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
hagia_sha256="$(sha256sum "$hagia_bin" | awk '{ print $1 }')"
core_sha256="$(sha256sum "$core_config" | awk '{ print $1 }')"
profile_sha256="$(sha256sum "$desktop_profile" | awk '{ print $1 }')"
connectors_sha256="$(sha256sum "$connectors" | awk '{ print $1 }')"
[[ "$sophia_sha256" == "$recorded_sophia_sha256" \
    && "$hagia_sha256" == "$recorded_hagia_sha256" \
    && "$core_sha256" == "$recorded_core_sha256" \
    && "$profile_sha256" == "$recorded_profile_sha256" \
    && "$connectors_sha256" == "$recorded_connectors_sha256" ]] || {
    echo "frame-fed output evidence inputs no longer match their bound identities" >&2
    exit 1
}
committed_digest() {
    local commit="$1" path="$2"
    git -C "$ROOT_DIR" show "$commit:$path" | sha256sum | awk '{ print $1 }'
}
[[ "$core_sha256" == "$(committed_digest "$source_commit" "$core_config_path")" \
    && "$profile_sha256" == "$(committed_digest "$source_commit" "$desktop_profile_path")" ]] || {
    echo "frame-fed output configuration is not from its signed Sophia commit" >&2
    exit 1
}

success_sha256="$(sha256sum "$success_log" | awk '{ print $1 }')"
rollback_sha256="$(sha256sum "$rollback_log" | awk '{ print $1 }')"
pair_sha256="$(printf '%s %s\n' "$success_sha256" "$rollback_sha256" | sha256sum | awk '{ print $1 }')"
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
archived_already="$(grep -rlFx --include=manifest "evidence_pair_sha256=$pair_sha256" \
    "$run_root" 2>/dev/null || true)"
if [[ -n "$archived_already" ]]; then
    echo "frame-fed output evidence pair is already archived" >&2
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

install -m 600 "$success_log" "$run_dir/success.log"
install -m 600 "$rollback_log" "$run_dir/rollback.log"
install -m 600 "$connectors" "$run_dir/connectors.txt"
install -m 600 "$core_config" "$run_dir/core.kdl"
install -m 600 "$desktop_profile" "$run_dir/desktop-profile.kdl"
printf '%s\n' 'sophia_frame_fed_output_physical schema=1 status=passed boundary=after_apply phases=2' \
    >"$run_dir/result.kdl"
printf 'record_schema=1\nrecord_kind=frame_fed_output_physical\nrecorded_at_utc=%s\nsource_commit=%s\nhagia_commit=%s\nsuccess_text=%s\nrollback_text=%s\nsuccess_evidence_sha256=%s\nrollback_evidence_sha256=%s\nevidence_pair_sha256=%s\nsophia_binary_sha256=%s\nhagia_binary_sha256=%s\ncore_config_path=%s\ncore_config_sha256=%s\ndesktop_profile_path=%s\ndesktop_profile_sha256=%s\nconnectors_sha256=%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$source_commit" "$hagia_commit" \
    "$success_text" "$rollback_text" "$success_sha256" "$rollback_sha256" "$pair_sha256" \
    "$sophia_sha256" "$hagia_sha256" "$core_config_path" "$core_sha256" \
    "$desktop_profile_path" "$profile_sha256" "$connectors_sha256" >"$run_dir/manifest"
chmod 600 "$run_dir/manifest" "$run_dir/result.kdl"
(
    cd "$run_dir"
    sha256sum connectors.txt core.kdl desktop-profile.kdl manifest result.kdl \
        rollback.log success.log >SHA256SUMS
)
chmod 600 "$run_dir/SHA256SUMS"
"$ROOT_DIR/tools/verify_frame_fed_output_physical_archive.sh" "$run_dir" >/dev/null
trap - ERR HUP INT TERM
echo "Recorded verified frame-fed output physical run: $run_dir"

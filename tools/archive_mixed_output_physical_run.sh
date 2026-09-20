#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence="${1:?usage: archive_mixed_output_physical_run.sh EVIDENCE EXTENDED_CONNECTOR}"
extended_connector="${2:?usage: archive_mixed_output_physical_run.sh EVIDENCE EXTENDED_CONNECTOR}"
state_home="${XDG_STATE_HOME:-$HOME/.local/state}"
run_root="${SOPHIA_MIXED_RUN_ROOT:-$state_home/sophia/promotion/mixed-output-runs}"
sophia_bin="${SOPHIA_MIXED_SOPHIA_BIN:-$ROOT_DIR/target/release/sophia}"
wm_bin="${SOPHIA_MIXED_WM_BIN:-$ROOT_DIR/target/release/sophia-wm-demo}"
core_config="${SOPHIA_MIXED_CORE_CONFIG:-$ROOT_DIR/tools/config/sophia/core.kdl}"
desktop_profile="${SOPHIA_MIXED_DESKTOP_PROFILE:-$ROOT_DIR/tools/fixtures/mixed_output_probe.kdl}"

[[ "$extended_connector" =~ ^[A-Za-z0-9._-]+$ ]] || {
    echo "mixed-output extended connector has invalid syntax: $extended_connector" >&2
    exit 2
}
"$ROOT_DIR/tools/verify_mixed_output_evidence.sh" "$evidence" "$extended_connector" >/dev/null
grep -Fxq \
    'sophia_mixed_output_visual schema=1 status=confirmed mirror_content=matched extended_text=sharp resampling=none heads=3 groups=2' \
    "$evidence" || {
    echo "mixed-output evidence lacks exact visible-pixel acceptance" >&2
    exit 1
}
grep -Fxq 'sophia_mixed_output_gate schema=1 status=passed exit=0' "$evidence" || {
    echo "mixed-output evidence lacks its passing gate record" >&2
    exit 1
}

mapfile -t identities < <(
    grep -E '^sophia_mixed_output_gate schema=1 status=starting source_commit=[0-9a-f]{40} sophia_sha256=[0-9a-f]{64} wm_sha256=[0-9a-f]{64} heads=3 groups=2$' \
        "$evidence" || true
)
(( ${#identities[@]} == 1 )) || {
    echo "mixed-output evidence must contain one exact source/binary identity" >&2
    exit 1
}
identity="${identities[0]}"
source_commit="$(sed -n 's/.* source_commit=\([0-9a-f]\{40\}\) .*/\1/p' <<<"$identity")"
recorded_sophia_sha256="$(sed -n 's/.* sophia_sha256=\([0-9a-f]\{64\}\) .*/\1/p' <<<"$identity")"
recorded_wm_sha256="$(sed -n 's/.* wm_sha256=\([0-9a-f]\{64\}\) .*/\1/p' <<<"$identity")"

git -C "$ROOT_DIR" cat-file -e "$source_commit^{commit}" || {
    echo "mixed-output evidence has an invalid source commit" >&2
    exit 1
}
git -C "$ROOT_DIR" verify-commit "$source_commit" >/dev/null 2>&1 || {
    echo "mixed-output evidence source commit does not have a valid signature" >&2
    exit 1
}
for file in "$sophia_bin" "$wm_bin" "$core_config" "$desktop_profile"; do
    [[ -r "$file" ]] || {
        echo "mixed-output archive input is unavailable: $file" >&2
        exit 1
    }
done
for binary in "$sophia_bin" "$wm_bin"; do
    [[ -x "$binary" ]] || {
        echo "mixed-output archive binary is not executable: $binary" >&2
        exit 1
    }
done

sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
wm_sha256="$(sha256sum "$wm_bin" | awk '{ print $1 }')"
core_config_sha256="$(sha256sum "$core_config" | awk '{ print $1 }')"
desktop_profile_sha256="$(sha256sum "$desktop_profile" | awk '{ print $1 }')"
[[ "$sophia_sha256" == "$recorded_sophia_sha256" ]] || {
    echo "mixed-output Sophia binary no longer matches the verified run" >&2
    exit 1
}
[[ "$wm_sha256" == "$recorded_wm_sha256" ]] || {
    echo "mixed-output WM binary no longer matches the verified run" >&2
    exit 1
}

committed_file_sha256() {
    local path="$1"
    git -C "$ROOT_DIR" show "$source_commit:$path" | sha256sum | awk '{ print $1 }'
}
[[ "$core_config_sha256" == \
    "$(committed_file_sha256 tools/config/sophia/core.kdl)" ]] || {
    echo "mixed-output core configuration does not match the signed source commit" >&2
    exit 1
}
[[ "$desktop_profile_sha256" == \
    "$(committed_file_sha256 tools/fixtures/mixed_output_probe.kdl)" ]] || {
    echo "mixed-output desktop profile does not match the signed source commit" >&2
    exit 1
}

evidence_sha256="$(sha256sum "$evidence" | awk '{ print $1 }')"
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
    echo "mixed-output physical evidence is already archived" >&2
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
install -m 600 "$core_config" "$run_dir/core.kdl"
install -m 600 "$desktop_profile" "$run_dir/desktop-profile.kdl"
printf '%s\n' 'sophia_mixed_output_physical schema=1 status=passed' >"$run_dir/result.kdl"
printf 'record_schema=1\nrecord_kind=mixed_output_physical\nrecorded_at_utc=%s\nsource_commit=%s\nextended_connector=%s\nevidence_sha256=%s\nsophia_binary_sha256=%s\nwm_binary_sha256=%s\ncore_config_sha256=%s\ndesktop_profile_sha256=%s\n' \
    "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$source_commit" "$extended_connector" \
    "$evidence_sha256" "$sophia_sha256" "$wm_sha256" \
    "$core_config_sha256" "$desktop_profile_sha256" >"$run_dir/manifest"
chmod 600 "$run_dir/manifest" "$run_dir/result.kdl"
(
    cd "$run_dir"
    sha256sum core.kdl desktop-profile.kdl manifest result.kdl session.log >SHA256SUMS
)
chmod 600 "$run_dir/SHA256SUMS"
"$ROOT_DIR/tools/verify_mixed_output_physical_archive.sh" "$run_dir" >/dev/null
trap - ERR HUP INT TERM
echo "Recorded verified mixed-output physical run: $run_dir"

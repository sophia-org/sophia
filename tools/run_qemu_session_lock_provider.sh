#!/usr/bin/env bash
# The session-lock-provider QEMU regression: an accepted verdict unlocks the
# session whatever the lock provider is doing. One guest image from pinned
# candidate binaries, then RUNS runs of each MODE (stall, flood, baseline).
# Each run is bounded by an outer SIGKILL, writes its own evidence file and is
# never repeated: the first failure stops the series and stays as recorded.
#
#   tools/run_qemu_session_lock_provider.sh MANIFEST OUT_DIR [RUNS] [MODES...]
#
# MANIFEST is a sha256sum file naming sophia, sophia-factotum and
# sophia-factotum-pam. Latencies are guest log observations (see
# tools/verify_qemu_session_lock_provider.py), never pass/fail thresholds.
#
# SOPHIA_QEMU_SERIES_PHASE=build builds and records the image only, so a later
# SOPHIA_QEMU_SERIES_PHASE=run can boot it in a window free of builds; run
# re-verifies the manifest, pinned binaries, guest tools and image first.
# The default, all, does both.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MANIFEST="${1:?manifest}"
OUT="${2:?output directory}"
RUNS="${3:-3}"
shift 3 2>/dev/null || shift $#
MODES=("$@")
[[ ${#MODES[@]} -gt 0 ]] || MODES=(stall flood baseline)
RUN_LIMIT_SECONDS="${SOPHIA_QEMU_RUN_LIMIT_SECONDS:-240}"
PHASE="${SOPHIA_QEMU_SERIES_PHASE:-all}"
case "$PHASE" in all|build|run) ;; *) echo "unknown series phase: $PHASE" >&2; exit 1 ;; esac

[[ "$OUT" = /* ]] || { echo "OUT_DIR must be absolute" >&2; exit 1; }
[[ ! -e "$OUT/SUMMARY.txt" ]] || { echo "$OUT already holds a series; use a new directory" >&2; exit 1; }
mkdir -p "$OUT/qemu"
export SOPHIA_QEMU_OUT_DIR="$OUT/qemu"

if [[ "$PHASE" != run ]]; then
    [[ ! -e "$OUT/image.SHA256SUM" ]] || { echo "$OUT already holds a built image" >&2; exit 1; }
    SOPHIA_QEMU_PINNED_MANIFEST="$MANIFEST" "$ROOT_DIR/tools/build_qemu_session_initramfs.sh" \
        > "$OUT/build.log" 2>&1 || { echo "guest build failed: $OUT/build.log" >&2; exit 1; }
    cp "$OUT/qemu/guest-tools/SHA256SUMS" "$OUT/guest-tools.SHA256SUMS"
    cp "$OUT/qemu/guest-tools/FIXTURE.txt" "$OUT/guest-tools.FIXTURE.txt"
    sha256sum "$MANIFEST" > "$OUT/manifest.SHA256SUM"
    cp "$MANIFEST" "$OUT/pinned.SHA256SUMS"
    sha256sum "$OUT"/qemu/*.img > "$OUT/image.SHA256SUM"
    if [[ "$PHASE" == build ]]; then
        echo "image built: $OUT/image.SHA256SUM"
        exit 0
    fi
else
    # The image was built earlier: everything it was built from must still be
    # what was recorded, and must name the same manifest.
    [[ -e "$OUT/image.SHA256SUM" ]] || { echo "$OUT holds no built image" >&2; exit 1; }
    [[ "$(sha256sum "$MANIFEST")" == "$(cat "$OUT/manifest.SHA256SUM")" ]] \
        || { echo "manifest differs from the one the image was built from" >&2; exit 1; }
    sha256sum --quiet -c "$OUT/image.SHA256SUM"
    (cd "$OUT/qemu/pinned" && sha256sum --quiet -c "$OUT/pinned.SHA256SUMS")
    (cd "$OUT/qemu/guest-tools" && sha256sum --quiet -c "$OUT/guest-tools.SHA256SUMS")
fi

for mode in "${MODES[@]}"; do
    for run in $(seq 1 "$RUNS"); do
        evidence="$OUT/$mode-$run.log"
        [[ ! -e "$evidence" ]] || { echo "refusing to overwrite $evidence" >&2; exit 1; }
        set +e
        timeout -s KILL "$RUN_LIMIT_SECONDS" env \
            SOPHIA_QEMU_SCENARIO=session-lock-provider \
            SOPHIA_QEMU_LOCK_PROVIDER_MODE="$mode" \
            SOPHIA_QEMU_EVIDENCE="$evidence" \
            "$ROOT_DIR/tools/qemu_session_harness.sh" > "$OUT/$mode-$run.harness.log" 2>&1
        status=$?
        set -e
        result="$(grep -h '^sophia_qemu_session_lock_provider_evidence ' \
            "$OUT/$mode-$run.harness.log" "$evidence" 2>/dev/null | tail -1 || true)"
        echo "mode=$mode run=$run exit=$status ${result:-no_verdict}" | tee -a "$OUT/SUMMARY.txt"
        if [[ "$status" -ne 0 ]]; then
            echo "first failure kept: $evidence (harness log $OUT/$mode-$run.harness.log)" | tee -a "$OUT/SUMMARY.txt"
            exit 1
        fi
    done
done
echo "series complete" | tee -a "$OUT/SUMMARY.txt"

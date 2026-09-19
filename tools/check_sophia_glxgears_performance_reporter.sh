#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORTER="$ROOT_DIR/tools/report_sophia_glxgears_performance.sh"
FIXTURE="$ROOT_DIR/tools/fixtures/sophia_glxgears_performance_pass.log"
MUTATED="$(mktemp)"
trap 'rm -f "$MUTATED"' EXIT

report="$("$REPORTER" "$FIXTURE")"
[[ "$report" == *" status=pass "* ]]
[[ "$report" == *" role=compatibility_probe "* ]]
[[ "$report" == *" client_mean_fps=59.981 "* ]]
[[ "$report" == *" present_fps=59.999 "* ]]
[[ "$report" == *" p95_frame_msec=16.667 "* ]]
[[ "$report" == *" native_mixed_exports=3 "* ]]
[[ "$report" == *" present_complete_copy=3 "* ]]
[[ "$report" == *" snapshot_captures=3 "* ]]
[[ "$report" == *" snapshot_promotions=3 "* ]]
[[ "$report" == *" import_cache_imports=3 "* ]]
[[ "$report" == *" import_cache_hits=2 "* ]]
[[ "$report" == *" cursor_legacy_updates_primary_in_flight=80 "* ]]
[[ "$report" == *" cursor_max_update_msec=1 "* ]]
# The cursor's own cost closes the line, and is zero on a fixture predating
# schema 7 rather than absent -- a reader that dropped it would otherwise look
# the same as a run that never paid it.
[[ "$report" == *" cursor_only=0 "* ]]
[[ "$report" == *" cursor_only_total_msec=0" ]]

grep -v '^GL_RENDERER' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted missing renderer identity" >&2
    exit 1
fi

sed \
    's/samples=3 advancing_intervals=2/samples=2 advancing_intervals=1/' \
    "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted insufficient advancing cadence" >&2
    exit 1
fi

sed 's/nonadvancing=0/nonadvancing=1/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted a nonadvancing cadence" >&2
    exit 1
fi

sed 's/overflowed=false/overflowed=true/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted an overflowed cadence" >&2
    exit 1
fi

sed 's/native_mixed_exports=3/native_mixed_exports=0/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted missing mixed composition evidence" >&2
    exit 1
fi

sed 's/native_submit_failures=0/native_submit_failures=1/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted native submission failure" >&2
    exit 1
fi

sed 's/import_cache_hits=2/import_cache_hits=0/' "$FIXTURE" >"$MUTATED"
zero_hit_report="$("$REPORTER" "$MUTATED")"
if [[ "$zero_hit_report" != *" import_cache_hits=0 "* ]]; then
    echo "glxgears reporter rejected a valid zero-hit changing-buffer workload" >&2
    exit 1
fi

sed 's/import_cache_imports=3/import_cache_imports=0/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted missing DMA-BUF import evidence" >&2
    exit 1
fi

sed 's/snapshot_promotions=3/snapshot_promotions=0/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted missing snapshot promotion" >&2
    exit 1
fi

sed 's/snapshot_live_entries=0/snapshot_live_entries=1/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted live snapshot debt" >&2
    exit 1
fi

sed 's/updates_primary_in_flight=80/updates_primary_in_flight=0/' \
    "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted no cursor updates overlapping primary flips" >&2
    exit 1
fi

sed 's/max_update_msec=1/max_update_msec=21/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted a blocking legacy cursor update" >&2
    exit 1
fi

# The atomic path passes this gate, and the overlap count is not held against
# it. Only the legacy ioctl increments that counter -- the atomic path returns
# before it is reached -- and the cursor plane is taken at readiness, after the
# first frames, so an atomic session legitimately carries the ioctl updates it
# made beforehand. Both shapes are accepted here; whether motion perturbed
# pacing is the cadence rule's judgement, which measures it directly.
sed -e 's/path=legacy_ioctl/path=atomic_plane/' \
    -e 's/updates_primary_in_flight=80/updates_primary_in_flight=0/' \
    "$FIXTURE" >"$MUTATED"
if ! "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter rejected a valid atomic cursor run" >&2
    exit 1
fi

sed 's/path=legacy_ioctl/path=atomic_plane/' "$FIXTURE" >"$MUTATED"
if ! "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter rejected an atomic run carrying pre-switch ioctl updates" >&2
    exit 1
fi

# Schema 7 renames that counter and adds what a cursor-only commit cost. The
# reporter must read the new name, or every run from here reports nothing
# about the cursor while appearing to pass.
sed -e 's/sophia_live_session_cursor schema=5 /sophia_live_session_cursor schema=7 /' \
    -e 's/ updates_primary_in_flight=80 / legacy_updates_primary_in_flight=80 cursor_only=2 cursor_only_max_msec=1 cursor_only_total_msec=2 /' \
    "$FIXTURE" >"$MUTATED"
if ! schema_seven="$("$REPORTER" "$MUTATED" 2>/dev/null)"; then
    echo "glxgears reporter rejected a schema-7 cursor record" >&2
    exit 1
fi
[[ "$schema_seven" == *" cursor_legacy_updates_primary_in_flight=80 "* ]]
[[ "$schema_seven" == *" cursor_only=2 "* ]]
[[ "$schema_seven" == *" cursor_only_total_msec=2"* ]]

# Shaped so only the path restriction can reject it: with a zero overlap
# count it would satisfy the atomic branch, so if it is refused, it is refused
# for being neither hardware path.
sed -e 's/path=legacy_ioctl/path=composited/' \
    -e 's/updates_primary_in_flight=80/updates_primary_in_flight=0/' \
    "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted a cursor that was neither hardware path" >&2
    exit 1
fi

sed 's/mean_fps=59.999/mean_fps=54.999/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted pointer-motion cadence below 55 FPS" >&2
    exit 1
fi

sed 's/import_cache_descriptor_mismatches=0/import_cache_descriptor_mismatches=1/' \
    "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted a DMA-BUF descriptor mismatch" >&2
    exit 1
fi

# Both session shapes this benchmark can produce are accepted, and nothing
# else is. The gate demanded wm_policy=external until it was noticed that the
# benchmark had long since become a standalone session, which reports
# disabled -- so the gate could only ever pass against its own fixture.
sed 's/wm_policy=external/wm_policy=disabled/' "$FIXTURE" >"$MUTATED"
if ! "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter rejected a valid standalone session" >&2
    exit 1
fi

sed 's/wm_policy=external/wm_policy=hosted/' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted an unknown wm_policy" >&2
    exit 1
fi

# The output extent has two sources, and a run must carry one of them. The
# repaint record is trace-level, so an ordinary benchmark run does not have
# it; the head's ready mode says the same thing about the same output and
# does survive. Requiring only the first made the report unpassable through
# its own benchmark, because enabling trace also diverts the records it reads.
grep -v '^sophia_live_output_repaint' "$FIXTURE" >"$MUTATED"
if "$REPORTER" "$MUTATED" >/dev/null 2>&1; then
    echo "glxgears reporter accepted a run with no output extent at all" >&2
    exit 1
fi

{
    grep -v '^sophia_live_output_repaint' "$FIXTURE"
    printf '%s\n' \
        'sophia_live_native_head schema=2 status=ready output=1 head=1 connector=DP-1 connector_id=94 mode=2560x1440 refresh_millihz=60000 mirrored=false'
} >"$MUTATED"
if ! head_extent="$("$REPORTER" "$MUTATED" 2>/dev/null)"; then
    echo "glxgears reporter rejected a run whose extent comes from the head mode" >&2
    exit 1
fi
[[ "$head_extent" == *" output_pixels=3686400 "* ]]

echo "glxgears performance reporter regressions passed"

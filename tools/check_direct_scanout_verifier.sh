#!/usr/bin/env bash
set -euo pipefail

# Proves the direct-scanout session verifier rejects what it claims to reject.
#
# A verifier nobody has watched fail is a verifier that passes everything. Each
# mutation below removes exactly one of its rules and must be refused with the
# message that rule owns.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verifier="$ROOT_DIR/tools/verify_direct_scanout_sessions.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf -- "$temp_dir"' EXIT

resources='sophia_live_native_resources schema=12 status=complete target_creations=6 renderer_workers=1 worker_result_misroutes=0 worker_max_service_skew=0 direct_scanout_attempts=44 direct_scanout_flips=43 direct_scanout_tests=2 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_unsupported=0 direct_scanout_fallbacks=1'
verdicts='sophia_live_direct_scanout_verdicts schema=2 status=complete eligible=44 layer_count=6 layer_not_active=0 layer_resampled=0 layer_offset=0 layer_not_head_sized=0 layer_clipped=0 layer_not_dma_buf=2 layer_translucent=0 composition_required=0 composed_cursor=0'

write_pass() {
    local path="$1"
    {
        printf '%s\n' "$resources"
        printf '%s\n' "$verdicts"
        printf 'sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=11 reason=none\n'
        printf 'sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=11 reason=none\n'
        printf 'sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=11 reason=none\n'
        printf 'sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=12 reason=none\n'
        printf 'sophia_live_direct_scanout schema=1 status=fell_back output=1 scene_generation=12 reason=none\n'
    } >"$path"
}

pass="$temp_dir/pass.log"
write_pass "$pass"
"$verifier" "$pass" >/dev/null || {
    echo "the verifier rejected a run that should pass" >&2
    exit 1
}

reject() {
    local name="$1" log="$2" expected="$3" output
    if output="$("$verifier" "$log" 2>&1)"; then
        echo "the verifier accepted $name" >&2
        exit 1
    fi
    printf '%s\n' "$output" | grep -Fq "$expected" || {
        echo "the verifier refused $name for the wrong reason:" >&2
        printf '%s\n' "$output" >&2
        exit 1
    }
}

# The whole point of the run: a session in which nothing was ever eligible has
# measured nothing, and must say so rather than pass on zeros.
quiet="$temp_dir/quiet.log"
sed -e 's/ direct_scanout_attempts=44/ direct_scanout_attempts=0/' \
    -e 's/ direct_scanout_flips=43/ direct_scanout_flips=0/' \
    -e 's/ direct_scanout_tests=2/ direct_scanout_tests=0/' \
    -e 's/ direct_scanout_fallbacks=1/ direct_scanout_fallbacks=0/' \
    -e 's/ eligible=44/ eligible=0/' \
    -e '/sophia_live_direct_scanout schema=1/d' \
    "$pass" >"$quiet"
reject "a run in which direct scanout never engaged" "$quiet" \
    "direct scanout never engaged"

# A run that measured a geometry refusal reports the numbers, not only the
# category: "not the head's size" is true of a client short by a scrollbar and
# one short by a monitor, and the fix differs.
measured="$temp_dir/measured.log"
{
    cat "$quiet"
    printf 'sophia_live_direct_scanout_geometry schema=2 status=layer_not_head_sized command=none output=1 head_width=2560 head_height=1440 layer_x=0 layer_y=0 layer_width=2560 layer_height=1428\n'
} >"$measured"
if output="$("$verifier" "$measured" 2>&1)"; then
    echo "the verifier accepted a run that never engaged" >&2
    exit 1
fi
printf '%s\n' "$output" | grep -Fq "layer_height=1428" || {
    echo "the verifier hid the geometry it measured:" >&2
    printf '%s\n' "$output" >&2
    exit 1
}

# Engine's proof disagreeing with the pixels it was computed from.
disagreed="$temp_dir/disagreed.log"
sed 's/ direct_scanout_refusals=0/ direct_scanout_refusals=1/' "$pass" >"$disagreed"
reject "a run whose proof disagreed with its pixels" "$disagreed" \
    "disagreed with the frame it lowered"

# A format or plane layout the backend cannot use is not that. Engine proves
# structure and never looks at a pixel format, so this is the backend answering
# a question Engine did not ask -- and counting the two together made the first
# run that produced one look like a defect.
unsupported="$temp_dir/unsupported.log"
sed -e 's/ direct_scanout_unsupported=0/ direct_scanout_unsupported=1/' \
    -e 's/ direct_scanout_attempts=44/ direct_scanout_attempts=45/' "$pass" >"$unsupported"
"$verifier" "$unsupported" >/dev/null || {
    echo "the verifier read a format refusal as a proof defect" >&2
    exit 1
}

# A client buffer on a plane the driver was never asked about.
untested="$temp_dir/untested.log"
sed 's/ direct_scanout_tests=2/ direct_scanout_tests=0/' "$pass" >"$untested"
reject "a direct flip with no validating commit" "$untested" \
    "no validating commit"

# Counters describing different frames.
overcounted="$temp_dir/overcounted.log"
sed 's/ direct_scanout_attempts=44/ direct_scanout_attempts=43/' "$pass" >"$overcounted"
reject "more settled attempts than attempts" "$overcounted" \
    "settled than were made"

refused_more="$temp_dir/refused_more.log"
sed 's/ direct_scanout_test_rejections=0/ direct_scanout_test_rejections=3/' "$pass" >"$refused_more"
reject "more refused validating commits than issued" "$refused_more" \
    "refused than issued"

# The episode order. A flip whose scene was never exported cannot have carried
# that scene's buffer, whatever the counters say.
unordered="$temp_dir/unordered.log"
grep -v 'status=exported output=1 scene_generation=11' "$pass" >"$unordered"
reject "a flip for a scene never exported" "$unordered" \
    "never exported"

# The flip's own guard, controlled separately: here a test did pass for an
# earlier scene, so the guard on `test_passed` is satisfied and only the guard
# on `flipped` can refuse this. Without a control of its own that guard could
# be deleted and every other case here would still pass.
orphan="$temp_dir/orphan.log"
{
    printf '%s\n' "$resources"
    printf '%s\n' "$verdicts"
    printf 'sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=11 reason=none\n'
    printf 'sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=11 reason=none\n'
    printf 'sophia_live_direct_scanout schema=1 status=fell_back output=1 scene_generation=11 reason=none\n'
    printf 'sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=12 reason=none\n'
} >"$orphan"
reject "a flip whose own scene was never exported" "$orphan" \
    "a direct flip happened for a scene never exported"

# The validating commit's own guard, controlled separately: no flip follows, so
# only the guard on `test_passed` can refuse this. Without a control of its own
# it could be deleted and the flip guard would still catch every other case.
premature="$temp_dir/premature.log"
{
    printf '%s\n' "$resources"
    printf '%s\n' "$verdicts"
    printf 'sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=11 reason=none\n'
} >"$premature"
reject "a validating commit for a scene never exported" "$premature" \
    "a validating commit passed for a scene never exported"

# Evidence from a build that predates the row.
stale="$temp_dir/stale.log"
sed 's/sophia_live_native_resources schema=12/sophia_live_native_resources schema=11/' "$pass" >"$stale"
reject "evidence from a build without the direct path" "$stale" \
    "did not run this build"

# The verdict histogram is the diagnostic; a run without it cannot say why.
blind="$temp_dir/blind.log"
grep -v 'sophia_live_direct_scanout_verdicts' "$pass" >"$blind"
reject "a run that reported no eligibility verdicts" "$blind" \
    "no eligibility verdicts"

# Every verdict must be present. The columns come from the enum itself, so a
# record missing one is named rather than compared against a sibling session:
# a histogram narrower than the vocabulary is how a nine-slot array came to be
# indexed with eleven verdicts.
renamed="$temp_dir/renamed.log"
sed 's/ layer_not_dma_buf=2 / layer_not_dmabuf=2 /' "$pass" >"$renamed"
if output="$("$verifier" "$renamed" 2>&1)"; then
    echo "the verifier accepted a verdict record missing a column" >&2
    exit 1
fi
printf '%s\n' "$output" | grep -Fq "missing layer_not_dma_buf" || {
    echo "the verifier refused an incomplete histogram for the wrong reason:" >&2
    printf '%s\n' "$output" >&2
    exit 1
}

# The standalone probe's own rules, which the session verifier does not own:
# the session finished, no window manager ran, and there was one client. A run
# that fails any of them reports zeros for a reason that has nothing to do with
# this row, and reading that as a defect would send someone looking in the
# wrong place.
standalone="$ROOT_DIR/tools/verify_direct_scanout_standalone.sh"
probe="$temp_dir/probe.log"
{
    printf 'sophia_live_session schema=18 status=bounded_complete display=:77 runtime_surfaces=0 runtime_max_surfaces=2 wm_policy=disabled wm_restarts=0\n'
    printf 'sophia_live_session_present schema=2 status=retired transaction=242 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true\n'
    cat "$pass"
} >"$probe"
"$standalone" "$probe" >/dev/null || {
    echo "the standalone verifier rejected a probe that should pass" >&2
    exit 1
}

probe_reject() {
    local name="$1" log="$2" expected="$3" output
    if output="$("$standalone" "$log" 2>&1)"; then
        echo "the standalone verifier accepted $name" >&2
        exit 1
    fi
    printf '%s\n' "$output" | grep -Fq "$expected" || {
        echo "the standalone verifier refused $name for the wrong reason:" >&2
        printf '%s\n' "$output" >&2
        exit 1
    }
}

managed="$temp_dir/managed.log"
sed 's/ wm_policy=disabled / wm_policy=external /' "$probe" >"$managed"
probe_reject "a probe that acquired a window manager" "$managed" \
    "its chrome makes every frame ineligible"

silent="$temp_dir/silent.log"
grep -v '^sophia_live_session_present schema=2 ' "$probe" >"$silent"
probe_reject "a probe whose client never presented" "$silent" \
    "never presented a frame"

unfinished="$temp_dir/unfinished.log"
grep -v '^sophia_live_session schema=18 ' "$probe" >"$unfinished"
probe_reject "a probe whose session never completed" "$unfinished" \
    "did not reach a bounded completion"

echo "direct scanout session verifier checks passed"

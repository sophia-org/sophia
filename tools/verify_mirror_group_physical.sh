#!/usr/bin/env bash
set -euo pipefail

candidate=false
if [[ "${1:-}" == --candidate ]]; then
    candidate=true
    shift
fi
evidence="${1:?usage: verify_mirror_group_physical.sh [--candidate] EVIDENCE}"
(( $# == 1 )) || {
    echo "usage: verify_mirror_group_physical.sh [--candidate] EVIDENCE" >&2
    exit 2
}

fail() {
    echo "mirror-group physical verification failed: $*" >&2
    exit 1
}

[[ -s "$evidence" ]] || fail "missing or empty evidence: $evidence"
raw_evidence="$evidence"
normalized_evidence="$(mktemp)"
trap 'rm -f -- "$normalized_evidence"' EXIT
# Production tracing prefixes schema payloads with timestamp, target, and ANSI
# state. Normalize only stable Sophia schema records; ordinary client output is
# intentionally left untouched.
sed -E 's/^.*(sophia_(live_[^ ]+|mirror_group[^ ]*|native_[^ ]+|session_[^ ]+|x11_[^ ]+) schema=)/\1/' \
    "$raw_evidence" >"$normalized_evidence"
evidence="$normalized_evidence"

count() {
    grep -Ec "$1" "$evidence" || true
}

field() {
    local line="$1" key="$2" token
    for token in $line; do
        if [[ "$token" == "$key="* ]]; then
            printf '%s\n' "${token#*=}"
            return 0
        fi
    done
    return 1
}

require_field() {
    local line="$1" key="$2" expected="$3" actual
    actual="$(field "$line" "$key")" || fail "record is missing $key"
    [[ "$actual" == "$expected" ]] || fail "$key is $actual, expected $expected"
}

require_positive_field() {
    local line="$1" key="$2" actual
    actual="$(field "$line" "$key")" || fail "record is missing $key"
    [[ "$actual" =~ ^[1-9][0-9]*$ ]] || fail "$key must be a positive integer: $actual"
}

if grep -Eqi '(^Error:|panicked at|status=(failed|degraded)([[:space:]]|$)|status=(hard_stall|head_lost)([[:space:]]|$))' \
    "$evidence"; then
    fail "evidence contains an error, panic, failed/degraded status, stall, or head loss"
fi
if grep -Eq '(sophia_live_session_client_fatal|sophia_x11_authority_backpressure schema=1 status=(shutdown|transport_failure)|X authority observed transaction channel is full|xterm: fatal IO error)' \
    "$evidence"; then
    fail "evidence contains an X11 client failure or abandoned authority observation"
fi
if grep -Eq '^sophia_live_output_damage schema=1 status=queue_rejected .*reason=OutputMismatch([[:space:]]|$)' \
    "$evidence"; then
    fail "a head rejected damage for a different output shape"
fi
if grep -Eq '^sophia_native_composition_sampling schema=2 status=(fallback|unavailable)([[:space:]]|$)' \
    "$evidence"; then
    fail "sharp mirror downsampling was unavailable or fell back"
fi
if grep -Eq '^sophia_live_mirror_bootstrap schema=2 status=direct_cpu ' "$evidence"; then
    fail "mirror startup regressed to a flat direct-CPU raster"
fi

[[ "$(count '^sophia_mirror_group_gate schema=1 status=starting source_commit=[0-9a-f]{40} sophia_sha256=[0-9a-f]{64} profile_sha256=[0-9a-f]{64}$')" == 1 ]] ||
    fail "expected one exact source/binary/profile identity record"
grep -Fxq \
    'sophia_live_outputs schema=2 status=ready discovered=2 presentation=1 native_owned=2 multi_output_scanout=enabled layout=extended_horizontal' \
    "$evidence" || fail "one logical output was not backed by two owned heads"
grep -Fxq 'sophia_live_session_startup schema=2 status=output_baseline_ready outputs=1/1' \
    "$evidence" || fail "the logical output baseline was not presented"
grep -Eq '^sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=(none|[0-9]+)$' \
    "$evidence" || fail "native ownership did not suspend through a clean drain"

mapfile -t ready_heads < <(
    grep -E '^sophia_live_native_head schema=2 status=ready output=[0-9]+ head=[0-9]+ connector=DP-[12] connector_id=[0-9]+ mode=[0-9]+x[0-9]+ refresh_millihz=[0-9]+ mirrored=true$' \
        "$evidence"
)
(( ${#ready_heads[@]} == 2 )) || fail "expected exactly two mirrored ready heads"
dp1="$(printf '%s\n' "${ready_heads[@]}" | grep ' connector=DP-1 ')"
dp2="$(printf '%s\n' "${ready_heads[@]}" | grep ' connector=DP-2 ')"
[[ -n "$dp1" && -n "$dp2" ]] || fail "DP-1 and DP-2 were not both reported"
require_field "$dp1" mode 2560x1440
require_field "$dp2" mode 1920x1080
dp1_output="$(field "$dp1" output)" || fail "DP-1 omitted output identity"
dp2_output="$(field "$dp2" output)" || fail "DP-2 omitted output identity"
[[ "$dp1_output" == "$dp2_output" ]] || fail "the heads do not share one logical output"
mapfile -t startup_outputs < <(
    grep -E '^sophia_live_native_startup_output schema=1 status=presented output=[0-9]+ proof=synchronous_modeset submission=[0-9]+$' \
        "$evidence"
)
(( ${#startup_outputs[@]} == 1 )) || fail "expected one logical startup-output record"
require_field "${startup_outputs[0]}" output "$dp1_output"
dp1_connector="$(field "$dp1" connector_id)" || fail "DP-1 omitted connector identity"
dp2_connector="$(field "$dp2" connector_id)" || fail "DP-2 omitted connector identity"
[[ "$dp1_connector" != "$dp2_connector" ]] || fail "the heads share one connector identity"
# Per-head evidence below carries only the opaque head id; the ready mapping
# lines above are where those ids meet connector names.
dp1_head="$(field "$dp1" head)" || fail "DP-1 omitted head identity"
dp2_head="$(field "$dp2" head)" || fail "DP-2 omitted head identity"
[[ "$dp1_head" != "$dp2_head" ]] || fail "the heads share one head identity"
startup_frame=
startup_scene=
for head in "$dp1_head" "$dp2_head"; do
    grep -Fxq "sophia_live_head_bootstrap schema=1 status=worker_ready output=$dp1_output head=$head workers=1" \
        "$evidence" || fail "head $head did not establish its renderer worker"
    mapfile -t bootstrap_records < <(
        grep -E "^sophia_live_head_bootstrap schema=1 status=worker_composed output=$dp1_output head=$head frame=[0-9]+ scene_generation=[0-9]+ target_generation=[0-9]+ mapping=(fit|cover|exact) exports=1$" \
            "$evidence"
    )
    (( ${#bootstrap_records[@]} == 1 )) ||
        fail "head $head did not establish exactly one worker-composed semantic baseline"
    bootstrap="${bootstrap_records[0]}"
    frame="$(field "$bootstrap" frame)" || fail "head $head bootstrap omitted frame identity"
    scene="$(field "$bootstrap" scene_generation)" ||
        fail "head $head bootstrap omitted scene identity"
    require_positive_field "$bootstrap" frame
    require_positive_field "$bootstrap" scene_generation
    require_positive_field "$bootstrap" target_generation
    require_field "$bootstrap" mapping fit
    if [[ -z "$startup_frame" ]]; then
        startup_frame="$frame"
        startup_scene="$scene"
    elif [[ "$frame" != "$startup_frame" || "$scene" != "$startup_scene" ]]; then
        fail "semantic startup heads did not share one frame and scene generation"
    fi
    queue="$(grep -Em1 "^sophia_live_head_composition_queue schema=1 status=queued output=$dp1_output head=$head frame=$frame scene_generation=$scene target_generation=$(field "$bootstrap" target_generation) mapping=fit " "$evidence" || true)"
    [[ -n "$queue" ]] || fail "head $head semantic startup omitted matching queue identity"
    plan="$(grep -Em1 "^sophia_live_head_composition_plan schema=2 status=ready output=$dp1_output head=$head scene_generation=$scene target_generation=$(field "$bootstrap" target_generation) " "$evidence" || true)"
    [[ -n "$plan" ]] || fail "head $head semantic startup omitted matching plan identity"
    plan_line="$(grep -nFm1 "$plan" "$evidence" | cut -d: -f1)"
    queue_line="$(grep -nFm1 "$queue" "$evidence" | cut -d: -f1)"
    bootstrap_line="$(grep -nFm1 "$bootstrap" "$evidence" | cut -d: -f1)"
    startup_output_line="$(grep -nFm1 "${startup_outputs[0]}" "$evidence" | cut -d: -f1)"
    (( plan_line < queue_line && queue_line < bootstrap_line && bootstrap_line < startup_output_line )) ||
        fail "head $head semantic startup evidence is not plan -> queue -> worker -> modeset ordered"
done
sampling_pair=false
while IFS= read -r dp1_sampling; do
    sampling_scene="$(field "$dp1_sampling" scene_generation)" || continue
    dp1_source="$(field "$dp1_sampling" source)" || continue
    dp1_target="$(field "$dp1_sampling" target)" || continue
    [[ "$dp1_source" =~ ^[1-9][0-9]*x[1-9][0-9]*$ \
        && "$dp1_source" == "$dp1_target" ]] || continue
    dp2_sampling="$(grep -Em1 \
        "^sophia_native_composition_sampling schema=3 status=active output=$dp1_output head=$dp2_head scene_generation=$sampling_scene requested=sharp_downscale effective=sharp_downscale alpha_mode=opaque source=[1-9][0-9]*x[1-9][0-9]* target=[1-9][0-9]*x[1-9][0-9]* frame=1920x1080$" \
        "$evidence" || true)"
    [[ -n "$dp2_sampling" ]] || continue
    dp2_source="$(field "$dp2_sampling" source)" || continue
    dp2_target="$(field "$dp2_sampling" target)" || continue
    [[ "$dp2_source" == "$dp1_source" ]] || continue
    dp1_source_width="${dp1_source%x*}"
    dp1_source_height="${dp1_source#*x}"
    dp2_target_width="${dp2_target%x*}"
    dp2_target_height="${dp2_target#*x}"
    if (( dp2_target_width * 4 == dp1_source_width * 3 \
        && dp2_target_height * 4 == dp1_source_height * 3 )); then
        sampling_pair=true
        break
    fi
done < <(
    grep -E "^sophia_native_composition_sampling schema=3 status=active output=$dp1_output head=$dp1_head scene_generation=[0-9]+ requested=exact_nearest effective=exact_nearest alpha_mode=opaque source=[1-9][0-9]*x[1-9][0-9]* target=[1-9][0-9]*x[1-9][0-9]* frame=2560x1440$" \
        "$evidence" || true
)
[[ "$sampling_pair" == true ]] ||
    fail "the mirror heads lack one exact-primary/sharp-0.75x sampling pair"

max_final_cpu_gray_pixels() {
    local output_extent="$1" line output target gray max=0
    while IFS= read -r line; do
        output="$(field "$line" output || true)"
        target="$(field "$line" target || true)"
        if [[ "$output" != "$output_extent" \
            && "$target" != "${output_extent}_0_0" ]]; then
            continue
        fi
        gray="$(field "$line" region_gray_pixels)" || continue
        [[ "$gray" =~ ^[0-9]+$ ]] || continue
        (( gray > max )) && max="$gray"
    done < <(
        grep -E '^sophia_native_composition_region schema=(1|2|3) status=read composition=final source_stage=cpu layer=[0-9]+ .*region_gray_pixels=[0-9]+([[:space:]]|$)' \
            "$evidence" || true
    )
    printf '%s\n' "$max"
}

dp1_gray_pixels="$(max_final_cpu_gray_pixels 2560x1440)"
dp2_gray_pixels="$(max_final_cpu_gray_pixels 1920x1080)"
(( dp1_gray_pixels > 500 )) || fail "DP-1 final composition did not contain substantial terminal text pixels"
(( dp2_gray_pixels > 500 )) || fail "DP-2 final composition did not contain substantial terminal text pixels"
(( dp2_gray_pixels * 4 >= dp1_gray_pixels )) ||
    fail "DP-2 retained too little terminal text coverage after downsampling"

mapfile -t complete_heads < <(
    grep -E '^sophia_live_native_head schema=3 status=complete output=[0-9]+ head=[0-9]+ scene_generation=[0-9]+ logical_content_checksum=[0-9]+ head_pixel_checksum=(unavailable|[0-9]+) submissions=[0-9]+ retirements=[0-9]+ callbacks=[0-9]+ nonzero_exports=[0-9]+$' \
        "$evidence"
)
(( ${#complete_heads[@]} == 2 )) || fail "expected exactly two completed heads"
for head in "$dp1_head" "$dp2_head"; do
    head_line="$(printf '%s\n' "${complete_heads[@]}" | grep " head=$head ")"
    [[ -n "$head_line" ]] || fail "head $head has no completion record"
    require_field "$head_line" output "$dp1_output"
    # Real native frame ids start at 1; zero is the placeholder the software
    # Present path carries, so a head reporting generation 0 presented nothing
    # this gate can speak for.
    require_positive_field "$head_line" scene_generation
    require_positive_field "$head_line" logical_content_checksum
    require_positive_field "$head_line" submissions
    require_positive_field "$head_line" retirements
    require_positive_field "$head_line" callbacks
    require_positive_field "$head_line" nonzero_exports
done
dp1_scene_generation="$(field "$(printf '%s\n' "${complete_heads[@]}" | grep " head=$dp1_head ")" scene_generation)" ||
    fail "DP-1 completion omitted its scene generation"
dp2_scene_generation="$(field "$(printf '%s\n' "${complete_heads[@]}" | grep " head=$dp2_head ")" scene_generation)" ||
    fail "DP-2 completion omitted its scene generation"
[[ "$dp1_scene_generation" == "$dp2_scene_generation" ]] ||
    fail "mirrored heads did not retire one logical scene generation"
dp1_checksum="$(field "$(printf '%s\n' "${complete_heads[@]}" | grep " head=$dp1_head ")" logical_content_checksum)" ||
    fail "DP-1 completion omitted its logical checksum"
dp2_checksum="$(field "$(printf '%s\n' "${complete_heads[@]}" | grep " head=$dp2_head ")" logical_content_checksum)" ||
    fail "DP-2 completion omitted its logical checksum"
[[ "$dp1_checksum" == "$dp2_checksum" ]] ||
    fail "mirrored heads did not retain one logical frame checksum"

# Completion counters can be positive for unrelated generations. Find one exact
# logical frame that crossed submit, callback, and retire on both heads *and*
# carried positive projected damage in each head's native coordinate space.
mirror_frame_has_positive_head_damage() {
    local frame="$1" head_and_mode head size width height rects pixels
    local -a records
    for head_and_mode in \
        "$dp1_head:2560:1440" \
        "$dp2_head:1920:1080"; do
        head="${head_and_mode%%:*}"
        size="${head_and_mode#*:}"
        width="${size%%:*}"
        height="${size#*:}"
        mapfile -t records < <(
            grep -E "^sophia_live_mirror_head_damage schema=2 status=presented output=$dp1_output head=$head frame=$frame width=$width height=$height mode=(partial|full) rects=[0-9]+ pixels=[0-9]+$" \
                "$evidence"
        )
        (( ${#records[@]} == 1 )) || return 1
        rects="$(field "${records[0]}" rects)" || return 1
        pixels="$(field "${records[0]}" pixels)" || return 1
        [[ "$rects" =~ ^[0-9]+$ && "$pixels" =~ ^[0-9]+$ ]] || return 1
        (( rects > 0 && pixels > 0 )) || return 1
    done
}

# A native-size buffer is proof of per-head composition only when its passive
# identity still names the Engine scene, committed target generation, mapping,
# and logical checksum that produced it. Require that plan/queue chain before
# accepting the later KMS lifecycle as physical evidence.
mirror_frame_has_head_composition_plan() {
    local frame="$1" head_and_mode head size width height density_key
    local queue_line plan_line scene_generation queue_number plan_number submit_number
    local common_scene=
    local -a records
    for head_and_mode in \
        "$dp1_head:2560:1440:exact" \
        "$dp2_head:1920:1080:exact"; do
        head="${head_and_mode%%:*}"
        size="${head_and_mode#*:}"
        width="${size%%:*}"
        size="${size#*:}"
        height="${size%%:*}"
        density_key="${size#*:}"
        mapfile -t records < <(
            grep -E "^sophia_live_head_composition_queue schema=1 status=queued output=$dp1_output head=$head frame=$frame " \
                "$evidence" || true
        )
        (( ${#records[@]} == 1 )) || return 1
        queue_line="${records[0]}"
        [[ "$(field "$queue_line" width)" == "$width" \
            && "$(field "$queue_line" height)" == "$height" \
            && "$(field "$queue_line" mapping)" == fit \
            && "$(field "$queue_line" logical_content_checksum)" == "$dp1_checksum" \
            && "$(field "$queue_line" source)" == head_plan ]] || return 1
        scene_generation="$(field "$queue_line" scene_generation)" || return 1
        [[ "$scene_generation" =~ ^[0-9]+$ ]] && (( scene_generation > 0 )) || return 1
        if [[ -z "$common_scene" ]]; then
            common_scene="$scene_generation"
        elif [[ "$scene_generation" != "$common_scene" ]]; then
            return 1
        fi
        mapfile -t records < <(
            grep -E "^sophia_live_head_composition_plan schema=2 status=ready output=$dp1_output head=$head scene_generation=$scene_generation " \
                "$evidence" || true
        )
        (( ${#records[@]} == 1 )) || return 1
        plan_line="${records[0]}"
        [[ "$(field "$plan_line" target_generation)" == "$(field "$queue_line" target_generation)" \
            && "$(field "$plan_line" width)" == "$width" \
            && "$(field "$plan_line" height)" == "$height" \
            && "$(field "$plan_line" mapping)" == fit \
            && "$(field "$plan_line" logical_content_checksum)" == "$dp1_checksum" \
            && "$(field "$plan_line" fallback)" == 0 \
            && "$(field "$plan_line" unavailable)" == 0 ]] || return 1
        [[ "$(field "$plan_line" "$density_key")" =~ ^[1-9][0-9]*$ \
            && "$(field "$plan_line" active)" =~ ^[1-9][0-9]*$ \
            && "$(field "$plan_line" compositor_primitives)" =~ ^[1-9][0-9]*$ ]] || return 1
        plan_number="$(grep -nFm1 "$plan_line" "$evidence" | cut -d: -f1)"
        queue_number="$(grep -nFm1 "$queue_line" "$evidence" | cut -d: -f1)"
        submit_number="$(grep -nEm1 "^sophia_live_native_head_page_flip schema=2 status=submitted output=$dp1_output head=$head .* frame=$frame$" "$evidence" | cut -d: -f1 || true)"
        [[ -n "$plan_number" && -n "$queue_number" && -n "$submit_number" ]] \
            && (( plan_number < queue_number && queue_number < submit_number )) || return 1
    done
}

common_frame=
while read -r frame; do
    [[ -n "$frame" ]] || continue
    causal=true
    for head in "$dp1_head" "$dp2_head"; do
        submit_line="$(grep -nEm1 "^sophia_live_native_head_page_flip schema=2 status=submitted output=$dp1_output head=$head .* frame=$frame$" "$evidence" | cut -d: -f1 || true)"
        callback_line="$(grep -nEm1 "^sophia_live_native_head_page_flip schema=2 status=callback_accepted output=$dp1_output head=$head callbacks=1 kernel_sequence=[0-9]+ frame=$frame$" "$evidence" | cut -d: -f1 || true)"
        retire_line="$(grep -nEm1 "^sophia_live_native_head_page_flip schema=2 status=retired output=$dp1_output head=$head submission=[0-9]+ frame=$frame$" "$evidence" | cut -d: -f1 || true)"
        if [[ -z "$submit_line" || -z "$callback_line" || -z "$retire_line" ]] \
            || (( submit_line >= callback_line || callback_line >= retire_line )); then
            causal=false
        fi
    done
    if [[ "$causal" == true ]] \
        && mirror_frame_has_positive_head_damage "$frame" \
        && mirror_frame_has_head_composition_plan "$frame"; then
        common_frame="$frame"
        break
    fi
done < <(
    sed -n "s/^sophia_live_mirror_generation schema=2 status=presented output=$dp1_output frame=\([0-9][0-9]*\) source=head_composition logical_content_checksum=$dp1_checksum$/\1/p" "$evidence"
)
[[ -n "$common_frame" ]] ||
    fail "no final-checksum per-head composition frame completed causally with positive projected damage on both heads"
[[ "$common_frame" == "$dp1_scene_generation" ]] ||
    fail "completed head evidence does not name the causally proven logical generation"

mapfile -t primary_presented_records < <(
    grep -E "^sophia_live_mirror_pacing schema=1 status=primary_presented output=$dp1_output primary=$dp1_head frame=$common_frame$" \
        "$evidence" || true
)
(( ${#primary_presented_records[@]} == 1 )) ||
    fail "the causally proven frame was not presented exactly once by the configured primary head"
mapfile -t release_records < <(
    grep -E "^sophia_live_mirror_pacing schema=1 status=released output=$dp1_output frame=$common_frame$" \
        "$evidence" || true
)
(( ${#release_records[@]} == 1 )) ||
    fail "the causally proven frame did not cross the last-head release barrier"
primary_presented_line="$(grep -nFm1 "${primary_presented_records[0]}" "$evidence" | cut -d: -f1)"
release_line="$(grep -nFm1 "${release_records[0]}" "$evidence" | cut -d: -f1)"
primary_retire_line="$(grep -nEm1 "^sophia_live_native_head_page_flip schema=2 status=retired output=$dp1_output head=$dp1_head submission=[0-9]+ frame=$common_frame$" "$evidence" | cut -d: -f1)"
last_common_retire_line="$(grep -nE "^sophia_live_native_head_page_flip schema=2 status=retired output=$dp1_output head=($dp1_head|$dp2_head) submission=[0-9]+ frame=$common_frame$" "$evidence" | tail -n1 | cut -d: -f1)"
(( primary_retire_line < primary_presented_line && last_common_retire_line < release_line )) ||
    fail "primary presentation and last-head release are not causally ordered"

common_scene="$(
    grep -Em1 "^sophia_live_head_composition_queue schema=1 status=queued output=$dp1_output head=$dp1_head frame=$common_frame " "$evidence" \
        | sed -n 's/.* scene_generation=\([0-9][0-9]*\) .*/\1/p'
)"
[[ -n "$common_scene" ]] || fail "final native-density frame omitted its scene generation"
dp1_content="$(grep -Em1 "^sophia_live_head_content schema=1 status=selected output=$dp1_output head=$dp1_head scene_generation=$common_scene .* source=cpu handle=[0-9]+ density_millis=1000 sampling=exact fidelity=authority_raster$" "$evidence" || true)"
dp2_content="$(grep -Em1 "^sophia_live_head_content schema=1 status=selected output=$dp1_output head=$dp2_head scene_generation=$common_scene .* source=cpu handle=[0-9]+ density_millis=750 sampling=exact fidelity=authority_raster$" "$evidence" || true)"
[[ -n "$dp1_content" && -n "$dp2_content" ]] ||
    fail "final mirrored xterm scene did not select exact authority rasters at 1000/750 density"
require_field "$dp2_content" surface "$(field "$dp1_content" surface)"
dp1_content_handle="$(field "$dp1_content" handle)"
dp2_content_handle="$(field "$dp2_content" handle)"
[[ "$dp1_content_handle" != "$dp2_content_handle" ]] ||
    fail "both heads selected one shared CPU raster instead of density-specific backing stores"
if grep -Eq '^sophia_x11_raster_requirement schema=1 status=sampled_fallback ' "$evidence"; then
    fail "X Authority reported a sampled fallback for native-density demand"
fi

# A shared logical snapshot is not valid evidence for different physical modes:
# each head must accept and present its own projected geometry for the exact
# generation proven above.
for head_and_mode in \
    "$dp1_head:2560:1440" \
    "$dp2_head:1920:1080"; do
    head="${head_and_mode%%:*}"
    size="${head_and_mode#*:}"
    width="${size%%:*}"
    height="${size#*:}"
    mapfile -t damage_records < <(
        grep -E "^sophia_live_mirror_head_damage schema=2 status=presented output=$dp1_output head=$head frame=$common_frame width=$width height=$height mode=(partial|full) rects=[0-9]+ pixels=[0-9]+$" \
            "$evidence"
    )
    (( ${#damage_records[@]} == 1 )) ||
        fail "head $head needs exactly one projected-damage record for frame $common_frame"
    require_positive_field "${damage_records[0]}" rects
    require_positive_field "${damage_records[0]}" pixels
done

grep -Fxq 'sophia_live_vsync schema=1 status=complete outputs=2 overlap_rejections=0 phase_rejections=0 policy=page_flip_paced' \
    "$evidence" || fail "both heads did not complete without pacing rejection"
session="$(grep -E '^sophia_live_session schema=(16|18) status=bounded_complete ' "$evidence")"
[[ "$(printf '%s\n' "$session" | wc -l)" == 1 ]] || fail "expected one bounded session completion"
for pair in \
    native_presentation=enabled \
    native_submit_failures=0 \
    native_retire_failures=0 \
    native_callback_rejected=0 \
    native_callback_queue_saturated=0 \
    native_in_flight=false \
    native_cleanup_pending=false; do
    require_field "$session" "${pair%%=*}" "${pair#*=}"
done
require_positive_field "$session" native_submissions
require_positive_field "$session" native_retirements
require_positive_field "$session" native_callback_accepted
require_positive_field "$session" native_nonzero_exports
require_positive_field "$session" cpu_checksum
# The CPU replay report hashes composed pixels, while the head checksum hashes
# the Engine's logical scene plan. Their domains differ; the plan, queue,
# submission, page-flip, and retirement checks above bind the latter to both
# heads.
resources="$(grep -E '^sophia_live_native_resources schema=(5|6|7|8|9|10|11|12) status=complete ' "$evidence")"
[[ "$(printf '%s\n' "$resources" | wc -l)" == 1 ]] ||
    fail "expected one native renderer resource completion"
require_positive_field "$resources" worker_requests
require_positive_field "$resources" worker_completions
require_positive_field "$resources" exact_nearest_draws
require_positive_field "$resources" sharp_downscale_draws
# Schema 5 counted degraded draws as `sharp_downscale_fallbacks`; later schemas
# call them `linear_fallback_draws`, because the count covers either direction
# and no longer double-counts as an upscale. Either name must read zero, and one
# must be present -- a record carrying neither stopped reporting whether the
# reconstruction shader ran at all.
if field "$resources" linear_fallback_draws >/dev/null; then
    require_field "$resources" linear_fallback_draws 0
elif field "$resources" sharp_downscale_fallbacks >/dev/null; then
    require_field "$resources" sharp_downscale_fallbacks 0
else
    fail "the resource completion reports no reconstruction fallback count"
fi
# A mirror group's heads share a card, so they share its renderer thread once
# coalescing is on: the same result stream serves both, and a result reaching
# the wrong head would attach one screen's buffer to the other's scanout lease.
# Schema 10 is the first evidence that says which happened.
if field "$resources" worker_result_misroutes >/dev/null; then
    require_field "$resources" worker_result_misroutes 0
fi
for key in worker_failures worker_hard_stalls worker_release_enqueue_failures; do
    require_field "$resources" "$key" 0
done
resource_schema="$(field "$resources" schema)"
if (( resource_schema >= 7 )); then
    slot_keys=(frame_slot_acquisitions frame_slot_reuses frame_slot_deferrals
        frame_slot_stale_releases frame_slots_leased frame_slots_high_watermark)
    if (( resource_schema >= 8 )); then
        slot_keys+=(frame_slot_partial_repaints frame_slot_full_repaints
            frame_slot_history_invalidations frame_slot_history_records)
    fi
    if (( resource_schema >= 9 )); then
        slot_keys+=(max_in_flight_per_output pending_frame_supersessions)
    fi
    if (( resource_schema >= 11 )); then
        slot_keys+=(direct_scanout_attempts direct_scanout_flips
            direct_scanout_tests direct_scanout_test_rejections
            direct_scanout_refusals direct_scanout_fallbacks)
    fi
    if (( resource_schema >= 12 )); then
        slot_keys+=(direct_scanout_unsupported)
    fi
    for key in "${slot_keys[@]}"; do
        value="$(field "$resources" "$key")" || fail "record is missing $key"
        [[ "$value" =~ ^[0-9]+$ ]] || fail "$key must be numeric: $value"
    done
    require_field "$resources" frame_slot_stale_releases 0
    if (( resource_schema >= 11 )); then
        # A mirror group never takes the direct path, by two independent
        # refusals: eligibility is proven about one head's plan, and a cohort
        # projects one scene into several heads' own modes, so no single client
        # buffer is the group's image. Stated here as a number rather than a
        # claim, because the second refusal is a line of code that a later
        # change could drop without any other evidence noticing.
        for key in direct_scanout_attempts direct_scanout_flips \
            direct_scanout_tests direct_scanout_refusals; do
            require_field "$resources" "$key" 0
        done
    fi
    requests="$(field "$resources" worker_requests)"
    completions="$(field "$resources" worker_completions)"
    deferrals="$(field "$resources" frame_slot_deferrals)"
    (( requests == completions + deferrals )) ||
        fail "renderer-worker requests did not settle as completion or bounded deferral"
else
    require_field "$resources" worker_completions "$(field "$resources" worker_requests)"
fi
keys="$(grep -E '^sophia_live_session_keys schema=2 status=complete ' "$evidence")"
[[ "$(printf '%s\n' "$keys" | wc -l)" == 1 ]] || fail "expected one key-state completion"
require_field "$keys" pending 0
require_field "$keys" release_barrier_pending 0
require_field "$keys" repeat_active_seats 0
session_line="$(grep -nEm1 '^sophia_live_session schema=(16|18) status=bounded_complete ' "$evidence" | cut -d: -f1)"
last_retire_line="$(grep -nE '^sophia_live_native_head_page_flip schema=2 status=retired ' "$evidence" | tail -n1 | cut -d: -f1)"
(( last_retire_line < session_line )) || fail "bounded completion preceded physical retirement"
grep -Eq '^sophia_live_session_health schema=1 status=clean protocol_errors=0 pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false$' \
    "$evidence" || fail "session health was not clean"
grep -Fxq 'sophia_live_output_topology_health schema=1 status=clean quarantined=false' \
    "$evidence" || fail "output topology remained quarantined"
grep -Fxq 'sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed' \
    "$evidence" || fail "frontend workers or session authority resources did not cleanly stop"
grep -Fxq 'sophia_mirror_group_gate schema=1 status=visual_confirmed outputs=1 connectors=2 heads=2 dp1_mode=2560x1440 dp2_mode=1920x1080' \
    "$evidence" || fail "operator did not confirm the visible mirror"
grep -Fxq 'sophia_mirror_group_visual schema=1 status=confirmed content=logically_identical case=mixed legibility=stable_both scaled_text=sharp_not_blocky foreground=white background=black' \
    "$evidence" || fail "operator did not confirm sharp, non-blocky scaled text"
passed_count="$(count '^sophia_mirror_group_gate schema=1 status=passed( |$)')"
exact_passed_count="$(count '^sophia_mirror_group_gate schema=1 status=passed exit=0$')"
if [[ "$candidate" == true ]]; then
    [[ "$passed_count" == 0 ]] || fail "candidate evidence was marked passed before verification"
else
    [[ "$passed_count" == 1 && "$exact_passed_count" == 1 ]] ||
        fail "gate did not record exactly one valid passing exit"
fi

echo "mirror-group physical evidence passed: $raw_evidence"

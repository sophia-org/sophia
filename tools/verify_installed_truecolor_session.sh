#!/usr/bin/env bash
set -euo pipefail

STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
LOG_DIR="${SOPHIA_HAGIA_LOG_DIR:-$STATE_HOME/sophia/hagia-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"
GUARD_LOG="${2:-$LOG_DIR/input-guard.log}"
RECOVERY_LOG="${3:-$LOG_DIR/recovery.log}"

fail() {
    echo "installed TrueColor verification failed: $*" >&2
    exit 1
}
require_file() {
    [[ -s "$1" ]] || fail "missing or empty evidence file: $1"
}
require_line() {
    grep -Eq "$1" "$2" || fail "$3 ($2)"
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
line_after() {
    local pattern="$1" minimum="$2"
    grep -nE "$pattern" "$SESSION_LOG" |
        cut -d: -f1 |
        awk -v minimum="$minimum" '$1 > minimum { print; exit }'
}

require_file "$SESSION_LOG"
require_file "$GUARD_LOG"
require_file "$RECOVERY_LOG"
if grep -Eqi '(^Error:|panicked at|^sophia_[^[:space:]]+ .*status=(failed|degraded)([[:space:]]|$))' \
    "$SESSION_LOG"; then
    fail "session log contains a Sophia error, panic, or degraded status"
fi
require_line \
    '^sophia_live_session schema=7 status=running .*native_presentation=enabled .*physical_input=enabled .*wm_policy=external ' \
    "$SESSION_LOG" "the installed proof did not run under native Hagia policy"
for app in terminal palette; do
    require_line "^sophia_session_app schema=1 status=started id=${app} source=startup$" \
        "$SESSION_LOG" "startup application is missing: $app"
done
require_line \
    '^sophia_live_outputs schema=2 status=ready discovered=2 presentation=2 native_owned=2 multi_output_scanout=enabled ' \
    "$SESSION_LOG" "two-output native ownership was not established"
startup="$(grep -E '^sophia_live_session_startup schema=2 status=ready ' "$SESSION_LOG" | tail -n 1 || true)"
[[ -n "$startup" ]] || fail "the final presentation-ready record is missing"
for assignment in outputs_ready=2/2 presented=true; do
    key="${assignment%%=*}"
    expected="${assignment#*=}"
    [[ "$(field "$startup" "$key" 2>/dev/null || true)" == "$expected" ]] ||
        fail "startup field $key did not equal $expected"
done
require_line \
    '^sophia_truecolor_client schema=2 status=ready width=640 height=240 target=640x240_1600_64 palette=asymmetric_rgb_cmy_gray put_image=exact get_image=exact alloc_color=exact alloc_named_color=exact query_colors=exact$' \
    "$SESSION_LOG" "the X11 palette did not round-trip through core TrueColor requests"

palette_line=""
palette_number=""
while IFS=: read -r number line; do
    [[ "$(field "$line" target)" =~ ^640x240_-?[0-9]+_-?[0-9]+$ ]] || continue
    valid=true
    for assignment in \
        region_pixels=153600 region_nonzero_rgb_pixels=153600 \
        region_red_pixels=9600 region_green_pixels=14400 \
        region_blue_pixels=19200 region_yellow_pixels=24000 \
        region_cyan_pixels=28800 region_magenta_pixels=33600 \
        region_gray_pixels=24000 region_other_pixels=0; do
        key="${assignment%%=*}"
        expected="${assignment#*=}"
        [[ "$(field "$line" "$key" 2>/dev/null || true)" == "$expected" ]] || valid=false
    done
    if [[ "$valid" == true ]]; then
        palette_line="$line"
        palette_number="$number"
        break
    fi
done < <(grep -nE 'sophia_native_composition_region schema=(1|2|3) status=read composition=final source_stage=cpu ' "$SESSION_LOG" || true)
[[ -n "$palette_line" ]] ||
    fail "the composed palette has a missing, swapped, collapsed, or contaminated color channel"

submit_line="$(line_after 'sophia_live_native_page_flip schema=1 status=submitted output=[0-9]+ ' "$palette_number")"
[[ "$submit_line" =~ ^[0-9]+$ ]] || fail "the final exact palette was not submitted"
submission_record="$(sed -n "${submit_line}p" "$SESSION_LOG")"
[[ "$submission_record" == *" status=submitted output=1 "* ]] ||
    fail "the exact palette's next native submission did not target output 1"
submission="$(field "$submission_record" submission)" || fail "palette submission omitted its ID"
frame="$(field "$submission_record" frame)" || fail "palette submission omitted its frame"
retire_line="$(line_after "sophia_live_native_page_flip schema=1 status=retired output=1 submission=${submission} frame=${frame}$" "$submit_line")"
[[ "$retire_line" =~ ^[0-9]+$ ]] || fail "the exact palette frame did not retire through KMS"

kitty_dma_buf=false
while read -r candidate; do
    transaction="$(field "$candidate" transaction)" || continue
    surface="$(field "$candidate" surface)" || continue
    if grep -Eq "^sophia_live_visual_candidate_identity schema=1 status=selected transaction=${transaction} surface=${surface} source=dma_buf buffer=[1-9][0-9]*$" \
        "$SESSION_LOG"; then
        kitty_dma_buf=true
        break
    fi
done < <(grep -E '^sophia_live_visual_candidate schema=1 status=selected transaction=[0-9]+ surface=[0-9]+ width=[0-9]+ height=[0-9]+ evidence=PresentedBuffer$' "$SESSION_LOG" || true)
[[ "$kitty_dma_buf" == true ]] || fail "Kitty did not reach composition as a presented DMA-BUF"

kitty_palette=false
kitty_number=""
while IFS=: read -r number line; do
    chromatic=true
    for key in region_red_pixels region_green_pixels region_blue_pixels \
        region_yellow_pixels region_cyan_pixels region_magenta_pixels; do
        count="$(field "$line" "$key" 2>/dev/null || true)"
        [[ "$count" =~ ^[0-9]+$ && "$count" -gt 0 ]] || chromatic=false
    done
    if [[ "$chromatic" == true ]]; then
        kitty_palette=true
        kitty_number="$number"
        break
    fi
done < <(grep -nE 'sophia_native_composition_region schema=(1|2|3) status=read composition=final source_stage=dmabuf ' "$SESSION_LOG" || true)
[[ "$kitty_palette" == true ]] || fail "Kitty's 24-bit ANSI sample did not compose all RGB/CMY channels"
kitty_submit_line="$(line_after 'sophia_live_native_page_flip schema=1 status=submitted output=[0-9]+ ' "$kitty_number")"
[[ "$kitty_submit_line" =~ ^[0-9]+$ ]] || fail "the final Kitty color frame was not submitted"
kitty_submission_record="$(sed -n "${kitty_submit_line}p" "$SESSION_LOG")"
[[ "$kitty_submission_record" == *" status=submitted output=1 "* ]] ||
    fail "Kitty's chromatic frame next submitted to an unexpected output"
kitty_submission="$(field "$kitty_submission_record" submission)" ||
    fail "Kitty submission omitted its ID"
kitty_frame="$(field "$kitty_submission_record" frame)" || fail "Kitty submission omitted its frame"
kitty_retire_line="$(line_after "sophia_live_native_page_flip schema=1 status=retired output=1 submission=${kitty_submission} frame=${kitty_frame}$" "$kitty_submit_line")"
[[ "$kitty_retire_line" =~ ^[0-9]+$ ]] || fail "the final Kitty color frame did not retire through KMS"

require_line '^sophia_live_wm schema=1 status=session_action_committed .* action=Logout$' \
    "$SESSION_LOG" "normal Super-Shift-Q logout was not committed"
require_line '^sophia_live_session_health schema=1 status=clean .*wm_degraded=false$' \
    "$SESSION_LOG" "final session health was not clean"
require_line '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$SESSION_LOG" "the TrueColor proof emitted an unexpected X protocol error"
require_line '^sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=none$' \
    "$SESSION_LOG" "native presentation did not drain"
require_line '^sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed$' \
    "$SESSION_LOG" "application or X Authority ownership remained after logout"
for output in 1 2; do
    require_line "^sophia_live_output schema=1 status=complete output=${output} .*nonzero_exports=[1-9][0-9]*$" \
        "$SESSION_LOG" "output $output did not finish with visible exports"
done
completion="$(grep -E '^sophia_live_session schema=(14|15|16|18) status=bounded_complete ' "$SESSION_LOG" | tail -n 1 || true)"
[[ -n "$completion" ]] || fail "bounded session completion is missing"
for assignment in \
    native_presentation=enabled physical_input=enabled wm_policy=external \
    wm_restarts=0 wm_degraded=false native_submit_failures=0 \
    native_retire_failures=0 native_callback_rejected=0 \
    native_in_flight=false native_cleanup_pending=false; do
    key="${assignment%%=*}"
    expected="${assignment#*=}"
    [[ "$(field "$completion" "$key" 2>/dev/null || true)" == "$expected" ]] ||
        fail "completion field $key did not equal $expected"
done
physical_keys="$(field "$completion" physical_keys_routed)" ||
    fail "completion omitted physical_keys_routed"
[[ "$physical_keys" =~ ^[0-9]+$ && "$physical_keys" -gt 0 ]] ||
    fail "the proof routed no physical logout keys"

require_line '^sophia_session_input_guard schema=1 status=armed$' "$GUARD_LOG" \
    "input guard was not armed"
if grep -Eq '^sophia_session_input_guard schema=1 status=triggered$' "$GUARD_LOG"; then
    fail "the proof used emergency recovery instead of normal logout"
fi
recovery="$(grep -E '^sophia_tty_recovery schema=3 profile=hagia ' "$RECOVERY_LOG" | tail -n 1 || true)"
[[ -n "$recovery" ]] || fail "normal Hagia TTY recovery is missing"
for assignment in termios_restored=true emergency=false session_shutdown=not_requested session_exit_status=none; do
    key="${assignment%%=*}"
    expected="${assignment#*=}"
    [[ "$(field "$recovery" "$key" 2>/dev/null || true)" == "$expected" ]] ||
        fail "recovery field $key did not equal $expected"
done
[[ "$(field "$recovery" kd_mode_before)" == "$(field "$recovery" kd_mode_after)" ]] ||
    fail "KD mode was not restored exactly"

echo "installed TrueColor session passed: palette_frame=$frame kitty_dma_buf=true outputs=2"

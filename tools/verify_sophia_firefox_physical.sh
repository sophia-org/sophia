#!/usr/bin/env bash
set -euo pipefail

STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
LOG_DIR="${SOPHIA_HAGIA_LOG_DIR:-$STATE_HOME/sophia/hagia-session}"
SESSION_LOG="${1:-$LOG_DIR/session.log}"
GUARD_LOG="${2:-$LOG_DIR/input-guard.log}"
RECOVERY_LOG="${3:-$LOG_DIR/recovery.log}"

fail() {
    echo "physical Firefox M10 verification failed: $*" >&2
    exit 1
}
require_file() {
    [[ -s "$1" ]] || fail "missing or empty evidence file: $1"
}
count() {
    grep -Ec "$1" "$SESSION_LOG" || true
}
line_number() {
    local pattern="$1" ordinal="${2:-1}"
    grep -nE "$pattern" "$SESSION_LOG" 2>/dev/null |
        sed -n "${ordinal}p" |
        cut -d: -f1 || true
}
line_number_after() {
    local pattern="$1" minimum="$2"
    grep -nE "$pattern" "$SESSION_LOG" 2>/dev/null |
        cut -d: -f1 |
        awk -v minimum="$minimum" '$1 > minimum { print; exit }' || true
}
require_line_number() {
    local pattern="$1" description="$2" observed
    observed="$(line_number "$pattern")"
    [[ -n "$observed" ]] || fail "$description"
    printf '%s\n' "$observed"
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
require_eq() {
    local line="$1" key="$2" expected="$3" actual
    actual="$(field "$line" "$key")" || fail "record is missing $key"
    [[ "$actual" == "$expected" ]] || fail "$key is $actual, expected $expected"
}
require_at_least() {
    local line="$1" key="$2" minimum="$3" actual
    actual="$(field "$line" "$key")" || fail "record is missing $key"
    [[ "$actual" =~ ^[0-9]+$ ]] || fail "$key is not an integer: $actual"
    (( actual >= minimum )) || fail "$key is $actual, expected at least $minimum"
}
require_file "$SESSION_LOG"
require_file "$GUARD_LOG"
require_file "$RECOVERY_LOG"

if grep -Eqi '(^Error:|panicked at|^sophia_[^[:space:]]+ .*status=(failed|degraded)([[:space:]]|$))' \
    "$SESSION_LOG"; then
    # Surface the specific proof/stage failure before the generic verdict so a
    # run self-reports (e.g. "died on resize") without a manual log read.
    grep -E '(Firefox M[0-9]+( Kitty)? proof incomplete: [^"]*|status=failed reason=[a-z_]+( stage=[a-z]+)?)' \
        "$SESSION_LOG" | sed 's/^/  cause: /' >&2 || true
    fail "session log contains a Sophia error, panic, or degraded status"
fi
if grep -Eqi '(Gdk-CRITICAL|gdk_window_thaw_toplevel_updates)' "$SESSION_LOG"; then
    fail "Firefox reported an unbalanced GDK toplevel update freeze"
fi
if grep -Eq '^sophia_live_session_pointer schema=5 status=focus_handoff_dropped reason=' \
    "$SESSION_LOG"; then
    fail "session dropped a pointer focus handoff"
fi
if grep -Eq 'stage_complete stage=(clipboard|primary) |checkpoint=(clipboard_peer|primary_peer) ' \
    "$SESSION_LOG"; then
    fail "promotion replayed focused CLIPBOARD or PRIMARY work"
fi
# Keep old archives readable without accepting a hybrid of the two protocols.
mapfile -t wm_ready < <(grep -E '^sophia_live_wm schema=[^ ]+ status=ready([[:space:]]|$)' "$SESSION_LOG" || true)
(( ${#wm_ready[@]} == 1 )) || fail "expected exactly one WM readiness record"
case "${wm_ready[0]}" in
    'sophia_live_wm schema=4 status=ready adapter=sophia_wm_v1 socket=session_owned epoch=1 restarts=0')
        restart_pattern='^sophia_live_wm schema=4 status=restarted adapter=sophia_wm_v1 epoch=[1-9][0-9]* restarts=[1-9][0-9]* preserved_layout=true$'
        ;;
    'sophia_live_wm schema=1 status=ready adapter=external socket=private restarts=0')
        restart_pattern='^sophia_live_wm schema=1 status=restarted restarts=[1-9][0-9]* preserved_layout=true$'
        ;;
    *) fail "WM readiness does not match a supported native or historical tuple" ;;
esac
while IFS= read -r restart; do
    [[ "$restart" =~ $restart_pattern ]] || fail "WM restart does not match its readiness protocol"
done < <(grep -E '^sophia_live_wm schema=[^ ]+ status=restarted([[:space:]]|$)' "$SESSION_LOG" || true)
grep -Eq '^sophia_live_outputs schema=2 status=ready discovered=2 presentation=2 native_owned=2 multi_output_scanout=enabled ' \
    "$SESSION_LOG" || fail "two-output native ownership was not established"
grep -Eq '^sophia_live_session_startup schema=2 status=output_baseline_ready outputs=2/2$' \
    "$SESSION_LOG" || fail "both output baselines were not presented"
mapfile -t startup_outputs < <(
    grep -E '^sophia_live_native_startup_output schema=1 status=presented output=[0-9]+ proof=synchronous_modeset submission=1$' \
        "$SESSION_LOG"
)
(( ${#startup_outputs[@]} == 2 )) ||
    fail "expected two synchronously presented startup outputs"
[[ "$(printf '%s\n' "${startup_outputs[@]}" | sed -n 's/.* output=\([0-9][0-9]*\) .*/\1/p' | sort -u | wc -l)" == 2 ]] ||
    fail "startup output evidence contains duplicate output identities"
# A damage-idle output correctly keeps its startup modeset instead of issuing
# redundant flips. Require asynchronous retirement somewhere in the session,
# while the per-output startup records prove that both scanouts presented.
grep -Eq 'sophia_live_native_page_flip schema=1 status=retired output=[0-9]+ ' \
    "$SESSION_LOG" || fail "no asynchronous page flip retired"

queued_vt_line="$(
    require_line_number \
        '^sophia_live_session_vt schema=4 status=queued target=[0-9]+ modifier_releases=[2-4]$' \
        'VT switch was not queued after modifier release'
)"
preparing_vt_line="$(
    require_line_number \
        '^sophia_live_session_vt schema=4 status=preparing target=[0-9]+$' \
        'VT switch was not prepared'
)"
renderer_capture_line="$(
    require_line_number \
        '^sophia_live_renderer_handoff schema=1 status=captured images=[1-9][0-9]*$' \
        'VT release did not retain renderer-owned images'
)"
quiesced_vt_line="$(
    require_line_number \
        '^sophia_live_session_vt schema=6 status=quiesced target=[0-9]+ outcome=drained drained=true abandoned_scanouts=0 skipped_present=none$' \
        'VT release did not drain native scanout'
)"
requested_vt_line="$(
    require_line_number \
        '^sophia_live_session_vt schema=4 status=requested target=[0-9]+$' \
        'VT switch was not requested'
)"
release_vt_line="$(
    require_line_number \
        '^sophia_live_seat schema=1 status=release_pending$' \
        'seat release did not become pending'
)"
suspended_vt_line="$(
    require_line_number \
        '^sophia_live_seat schema=1 status=suspended$' \
        'seat did not suspend'
)"
acquire_vt_line="$(
    require_line_number \
        '^sophia_live_seat schema=1 status=acquire_pending$' \
        'seat reacquisition did not become pending'
)"
renderer_restore_line="$(
    require_line_number \
        '^sophia_live_renderer_handoff schema=1 status=restored images=[1-9][0-9]* source=seat_resume$' \
        'VT resume did not restore renderer-owned images'
)"
resumed_vt_line="$(
    require_line_number \
        '^sophia_live_seat schema=1 status=active source=resume$' \
        'seat did not resume'
)"
post_resume_flip_line="$(
    line_number_after \
        'sophia_live_native_page_flip schema=1 status=retired output=1 ' \
        "$resumed_vt_line"
)"
[[ -n "$post_resume_flip_line" ]] || fail "no primary page flip retired after VT resume"
renderer_capture="$(sed -n "${renderer_capture_line}p" "$SESSION_LOG")"
renderer_restore="$(sed -n "${renderer_restore_line}p" "$SESSION_LOG")"
captured_images="$(field "$renderer_capture" images)" || fail "renderer capture omitted its count"
restored_images="$(field "$renderer_restore" images)" || fail "renderer restore omitted its count"
[[ "$captured_images" == "$restored_images" ]] ||
    fail "renderer-image handoff restored $restored_images of $captured_images images"
(( queued_vt_line < preparing_vt_line
    && preparing_vt_line < renderer_capture_line
    && renderer_capture_line < quiesced_vt_line
    && quiesced_vt_line < requested_vt_line
    && requested_vt_line < release_vt_line
    && release_vt_line < suspended_vt_line
    && suspended_vt_line < acquire_vt_line
    && acquire_vt_line < renderer_restore_line
    && renderer_restore_line < resumed_vt_line
    && resumed_vt_line < post_resume_flip_line )) ||
    fail "VT handoff, seat lifecycle, and resumed retirement are out of order"
if grep -Eq 'status=forced_detach|outcome=forced_detach_|remained in flight during teardown' \
    "$SESSION_LOG"; then
    fail "Firefox proof used the forced VT-detach fallback"
fi

[[ "$(count '^sophia_session_app schema=1 status=started id=terminal source=(startup|action)$')" == 2 ]] ||
    fail "the run must contain exactly two Kitty processes"
[[ "$(count '^sophia_session_app schema=1 status=started id=firefox source=action$')" == 2 ]] ||
    fail "Firefox was not action-launched exactly twice"
[[ "$(count '^sophia_session_app schema=1 status=exited id=firefox source=managed exit_status=exit status: 0$')" == 2 ]] ||
    fail "Firefox did not exit successfully exactly twice"

kitty_before_a="$(require_line_number '^sophia_firefox_m10 schema=1 status=kitty_checkpoint terminal=a checkpoint=before content=redacted$' 'Kitty A pre-Firefox checkpoint is missing')"
kitty_before_b="$(require_line_number '^sophia_firefox_m10 schema=1 status=kitty_checkpoint terminal=b checkpoint=before content=redacted$' 'Kitty B pre-Firefox checkpoint is missing')"
kitty_normal_a="$(require_line_number '^sophia_firefox_m10 schema=1 status=kitty_checkpoint terminal=a checkpoint=after_normal_close content=redacted$' 'Kitty A normal-close checkpoint is missing')"
kitty_normal_b="$(require_line_number '^sophia_firefox_m10 schema=1 status=kitty_checkpoint terminal=b checkpoint=after_normal_close content=redacted$' 'Kitty B normal-close checkpoint is missing')"
kitty_forced_a="$(require_line_number '^sophia_firefox_m10 schema=1 status=kitty_checkpoint terminal=a checkpoint=after_forced_close content=redacted$' 'Kitty A forced-close checkpoint is missing')"
kitty_forced_b="$(require_line_number '^sophia_firefox_m10 schema=1 status=kitty_checkpoint terminal=b checkpoint=after_forced_close content=redacted$' 'Kitty B forced-close checkpoint is missing')"
first_start="$(line_number '^sophia_session_app schema=1 status=started id=firefox source=action$' 1)"
second_start="$(line_number '^sophia_session_app schema=1 status=started id=firefox source=action$' 2)"
first_exit="$(line_number '^sophia_session_app schema=1 status=exited id=firefox source=managed exit_status=exit status: 0$' 1)"
second_exit="$(line_number '^sophia_session_app schema=1 status=exited id=firefox source=managed exit_status=exit status: 0$' 2)"
[[ -n "$first_start" && -n "$first_exit" && -n "$second_start" && -n "$second_exit" ]] ||
    fail "Firefox launch/exit ordering evidence is incomplete"
(( kitty_before_a < first_start && kitty_before_b < first_start )) ||
    fail "both Kitty windows were not interactive before Firefox"
(( first_start < first_exit
    && first_exit < kitty_normal_a && first_exit < kitty_normal_b
    && kitty_normal_a < second_start && kitty_normal_b < second_start
    && second_start < second_exit
    && second_exit < kitty_forced_a && second_exit < kitty_forced_b )) ||
    fail "Firefox close/restart and Kitty retention checkpoints are out of order"

mapfile -t firefox_admission_starts < <(
    grep -E '^sophia_session_app schema=2 status=started id=firefox source=action transaction=[0-9]+$' \
        "$SESSION_LOG"
)
(( ${#firefox_admission_starts[@]} == 2 )) ||
    fail "expected exactly two correlated Firefox admission starts"
firefox_exit_lines=("$first_exit" "$second_exit")
firefox_surfaces=()
admission_restart_count=0
for index in 0 1; do
    start_record="${firefox_admission_starts[$index]}"
    action_transaction="$(field "$start_record" transaction)" ||
        fail "Firefox admission start lacks its action transaction"
    admission_start_line="$(line_number "^sophia_session_app schema=2 status=started id=firefox source=action transaction=${action_transaction}$")"
    surface_observed_line="$(line_number_after "^sophia_session_app schema=2 status=surface_observed source=action transaction=${action_transaction} surface=[0-9]+$" "$admission_start_line")"
    [[ -n "$surface_observed_line" ]] ||
        fail "Firefox action $action_transaction never observed a surface"
    surface_record="$(sed -n "${surface_observed_line}p" "$SESSION_LOG")"
    firefox_admission_surface="$(field "$surface_record" surface)" ||
        fail "Firefox surface observation lacks its opaque surface"
    firefox_surfaces[$index]="$firefox_admission_surface"
    admitted_line="$(line_number_after "^sophia_session_app schema=2 status=admitted source=action transaction=${action_transaction} surface=${firefox_admission_surface}$" "$surface_observed_line")"
    [[ -n "$admitted_line" ]] ||
        fail "Firefox action $action_transaction never completed visual admission"
    (( admitted_line < firefox_exit_lines[index] )) ||
        fail "Firefox action $action_transaction exited before admission completed"
    visual_presented_line="$(line_number_after "^sophia_live_visual_admission schema=1 status=presented transaction=[0-9]+ surface=${firefox_admission_surface}$" "$surface_observed_line")"
    [[ -n "$visual_presented_line" ]] && (( visual_presented_line < admitted_line )) ||
        fail "Firefox action $action_transaction lacks retired admission pixels"

    mapfile -t admission_restarts < <(awk -v first="$surface_observed_line" -v last="$admitted_line" -v restart_pattern="$restart_pattern" '
        NR > first && NR < last && $0 ~ restart_pattern { print NR }
    ' "$SESSION_LOG")
    (( ${#admission_restarts[@]} <= 1 )) ||
        fail "Firefox action $action_transaction restarted the WM more than once"
    admission_restart_count=$((admission_restart_count + ${#admission_restarts[@]}))
    if (( ${#admission_restarts[@]} == 1 )); then
        restart_line="${admission_restarts[0]}"
        committed_phase_line="$(line_number_after '^sophia_live_wm schema=4 status=reseed_queued phase=committed_layout request=relayout$' "$restart_line")"
        manage_phase_line="$(line_number_after "^sophia_live_wm schema=4 status=reseed_queued phase=pending_admission request=manage surface=${firefox_admission_surface}$" "$committed_phase_line")"
        [[ -n "$committed_phase_line" && -n "$manage_phase_line" ]] \
            && (( committed_phase_line < manage_phase_line && manage_phase_line < admitted_line )) ||
            fail "Firefox recovery did not queue committed layout before manage replay"
        seed_commit_line="$(line_number_after '^sophia_live_wm schema=1 status=layout_committed .* outcome=Committed$' "$manage_phase_line")"
        [[ -n "$seed_commit_line" ]] && (( seed_commit_line < admitted_line )) ||
            fail "Firefox recovery lacks a committed-layout reseed"
        seed_commit="$(sed -n "${seed_commit_line}p" "$SESSION_LOG")"
        seed_transaction="$(field "$seed_commit" transaction)" ||
            fail "Firefox committed-layout reseed lacks a transaction"
        seed_configures="$(field "$seed_commit" configure_deliveries)" ||
            fail "Firefox committed-layout reseed lacks a geometry-control count"
        [[ "$seed_configures" =~ ^[1-9][0-9]*$ ]] ||
            fail "Firefox committed-layout reseed did not reassert stale client geometry"
        seed_geometry_acks="$(awk -v first="$restart_line" -v last="$seed_commit_line" \
            -v transaction="$seed_transaction" '
            NR > first && NR < last &&
                /^sophia_live_surface_geometry schema=1 status=frontend_configured / &&
                $0 ~ ("transaction=" transaction " ") { count++ }
            END { print count + 0 }
        ' "$SESSION_LOG")"
        (( seed_geometry_acks == seed_configures )) ||
            fail "Firefox committed-layout reseed did not acknowledge every geometry reassertion"
        if awk -v first="$restart_line" -v last="$seed_commit_line" -v surface="$firefox_admission_surface" '
            NR > first && NR < last &&
                $0 ~ /^sophia_live_visual_admission schema=1 status=armed / &&
                $0 ~ ("surface=" surface "$") { found=1 }
            END { exit !found }
        ' "$SESSION_LOG"; then
            fail "committed-layout reseed consumed Firefox admission pixels"
        fi
        replay_arm_line="$(line_number_after "^sophia_live_visual_admission schema=1 status=armed transaction=[0-9]+ surface=${firefox_admission_surface}$" "$seed_commit_line")"
        [[ -n "$replay_arm_line" ]] && (( replay_arm_line < visual_presented_line )) ||
            fail "Firefox manage replay did not arm its retained visual candidate"
    fi
done
firefox_surface="${firefox_surfaces[0]}"

forced_close="$(line_number_after '^sophia_live_wm schema=1 status=session_action_committed .* action=CloseFocused$' "$second_start")"
[[ -n "$forced_close" ]] && (( forced_close < second_exit )) ||
    fail "second Firefox was not closed through the WM close path"
if awk -v first="$first_start" -v last="$first_exit" \
    'NR > first && NR < last && /status=session_action_committed .* action=CloseFocused$/ { found=1 } END { exit !found }' \
    "$SESSION_LOG"; then
    fail "first Firefox used the WM close path instead of Ctrl+Q"
fi

grep -Eq '^sophia_firefox_m8 schema=1 status=page_ready .* content=redacted$' \
    "$SESSION_LOG" || fail "offline Firefox page never became ready"
for stage in loaded keyboard scroll layout refocus dialog; do
    [[ "$(count "^sophia_firefox_m8 schema=1 status=stage_complete stage=$stage ")" == 1 ]] ||
        fail "Firefox stage did not complete exactly once: $stage"
done
keyboard_line="$(line_number '^sophia_firefox_m8 schema=1 status=stage_complete stage=keyboard ')"
navigation_ready_line="$(require_line_number '^sophia_firefox_m8 schema=1 status=navigation_ready content=redacted$' 'replacement Firefox document never became ready')"
scroll_line="$(line_number '^sophia_firefox_m8 schema=1 status=stage_complete stage=scroll ')"
(( keyboard_line < navigation_ready_line && navigation_ready_line < scroll_line )) ||
    fail "Firefox navigation readiness is not ordered between keyboard and scroll completion"
axis_routes="$(awk -v first="$navigation_ready_line" -v last="$scroll_line" '
    NR > first && NR < last && /^sophia_live_session_pointer schema=9 status=axis_batch / {
        for (i = 1; i <= NF; i++) {
            if ($i ~ /^routed=[0-9]+$/) {
                split($i, field, "=")
                count += field[2]
            }
        }
    }
    END { print count + 0 }
' "$SESSION_LOG")"
(( axis_routes >= 1 )) ||
    fail "Firefox's DOM scroll stage has no causally ordered routed wheel packet"
layout_line="$(line_number '^sophia_firefox_m8 schema=1 status=stage_complete stage=layout ')"
layout_commit_line="$(line_number_after '^sophia_live_wm schema=1 status=layout_committed .* surfaces=4 .*moved_surfaces=[1-9][0-9]* .*outcome=Committed$' "$scroll_line")"
layout_action="$(line_number_after '^sophia_live_wm schema=1 status=physical_action_committed action=3$' "$scroll_line")"
layout_projection="$(line_number_after '^sophia_live_wm schema=2 status=workspace_projection_committed .* visible_surfaces=3 focus=surface$' "$layout_action")"
focus_away_action="$(line_number_after '^sophia_live_wm schema=1 status=physical_action_committed action=1$' "$layout_line")"
[[ -n "$layout_commit_line" && -n "$layout_action" && -n "$layout_line"
    && -n "$focus_away_action" ]] || fail "the Firefox layout stage is incomplete"
layout_commit="$(sed -n "${layout_commit_line}p" "$SESSION_LOG")"
layout_transaction="$(field "$layout_commit" transaction)" ||
    fail "the Firefox layout commit has no transaction identity"
layout_moved_surfaces="$(field "$layout_commit" moved_surfaces)" ||
    fail "the Firefox layout commit has no moved-surface count"
layout_configure_deliveries="$(field "$layout_commit" configure_deliveries)" ||
    fail "the Firefox layout commit has no geometry-control count"
[[ "$layout_moved_surfaces" =~ ^[0-9]+$ && "$layout_configure_deliveries" =~ ^[0-9]+$ ]] ||
    fail "the Firefox layout geometry counts are not integers"
(( layout_configure_deliveries == layout_moved_surfaces )) ||
    fail "the Firefox layout did not deliver geometry for every moved surface"
layout_epoch_line="$(line_number_after "^sophia_live_resize_epoch schema=1 status=committed transaction=${layout_transaction} matched_surfaces=[1-9][0-9]*$" "$scroll_line")"
[[ -n "$layout_epoch_line" ]] || fail "the Firefox layout stage lacks a complete resize epoch"
layout_epoch="$(sed -n "${layout_epoch_line}p" "$SESSION_LOG")"
matched_surfaces="$(field "$layout_epoch" matched_surfaces)" ||
    fail "the Firefox layout epoch has no matched-surface count"
(( layout_commit_line < layout_action
    && layout_epoch_line < layout_action
    && layout_action < layout_line
    && layout_line < focus_away_action )) ||
    fail "the Firefox geometry change is not ordered after one committed Super+Space layout"
layout_action_count="$(awk -v first="$scroll_line" -v last="$layout_line" '
    NR > first && NR < last && /^sophia_live_wm schema=1 status=physical_action_committed action=3$/ { count++ }
    END { print count + 0 }
' "$SESSION_LOG")"
(( layout_action_count == 1 )) ||
    fail "the Firefox layout stage must contain exactly one Super+Space action"
[[ -n "$layout_projection" ]] \
    && (( layout_action < layout_projection && layout_projection < layout_line )) ||
    fail "the Firefox layout stage did not retain all three managed surfaces"
layout_projection_record="$(sed -n "${layout_projection}p" "$SESSION_LOG")"
require_eq "$layout_projection_record" transaction "$layout_transaction"
geometry_ack_line="$(line_number_after "^sophia_live_surface_geometry schema=1 status=frontend_configured transaction=${layout_transaction} surface=${firefox_surface}$" "$scroll_line")"
[[ -n "$geometry_ack_line" ]] \
    && (( scroll_line < geometry_ack_line && geometry_ack_line < layout_line )) ||
    fail "the move-only Firefox surface lacks a correlated geometry acknowledgement"
geometry_ack_count="$(awk -v first="$scroll_line" -v last="$layout_line" \
    -v transaction="$layout_transaction" -v surface="$firefox_surface" '
    NR > first && NR < last &&
        /^sophia_live_surface_geometry schema=1 status=frontend_configured / &&
        $0 ~ ("transaction=" transaction " ") && $0 ~ ("surface=" surface "$") { count++ }
    END { print count + 0 }
' "$SESSION_LOG")"
(( geometry_ack_count == 1 )) ||
    fail "the move-only Firefox geometry was not acknowledged exactly once"

mapfile -t visual_armed_records < <(awk -v first="$scroll_line" -v last="$focus_away_action" -v epoch="$layout_transaction" '
    NR > first && NR < last && /^sophia_live_resize_epoch schema=3 status=visual_armed / &&
        $0 ~ ("epoch=" epoch " ") { print NR ":" $0 }
' "$SESSION_LOG")
(( ${#visual_armed_records[@]} == matched_surfaces )) ||
    fail "the Firefox layout epoch did not arm every matched surface exactly once"
for armed_entry in "${visual_armed_records[@]}"; do
    visual_armed_line="${armed_entry%%:*}"
    visual_armed="${armed_entry#*:}"
    transaction="$(field "$visual_armed" transaction)" || fail "visual candidate lacks transaction"
    surface="$(field "$visual_armed" surface)" || fail "visual candidate lacks surface"
    width="$(field "$visual_armed" width)" || fail "visual candidate lacks width"
    height="$(field "$visual_armed" height)" || fail "visual candidate lacks height"
    (( width > 0 && height > 0 )) || fail "visual candidate has an invalid extent"
    mapfile -t visual_commits < <(awk -v first="$visual_armed_line" -v last="$focus_away_action" \
        -v transaction="$transaction" -v surface="$surface" -v width="$width" -v height="$height" '
        NR > first && NR < last && /^sophia_live_resize_epoch schema=3 status=visual_committed / &&
            $0 ~ ("transaction=" transaction " ") && $0 ~ ("surface=" surface " ") &&
            $0 ~ ("width=" width " ") && $0 ~ ("height=" height "($| )") { print NR }
    ' "$SESSION_LOG")
    (( ${#visual_commits[@]} == 1 )) ||
        fail "layout candidate $transaction/$surface lacks one exact native retirement"
done
if awk -v first="$scroll_line" -v last="$focus_away_action" \
    'NR > first && NR < last && /outcome=RejectedStaleSurface/ { found=1 } END { exit !found }' \
    "$SESSION_LOG"; then
    fail "a Firefox layout Present became stale before visual retirement"
fi
firefox_present_line="$(awk -v first="$layout_action" -v last="$focus_away_action" -v surface="$firefox_surface" '
    NR > first && NR < last && /^sophia_live_session_present schema=(2|4) status=retired / &&
        $0 ~ ("surface=" surface " ") { print NR; exit }
' "$SESSION_LOG")"
[[ -n "$firefox_present_line" ]] ||
    fail "the correlated Firefox surface did not present after the layout change"
promotion_completion="$(grep -E '^sophia_firefox_promotion schema=1 status=complete stages=6 selection_gates=focused content=redacted$' "$SESSION_LOG" | tail -n 1 || true)"
[[ -n "$promotion_completion" ]] || fail "Firefox six-stage promotion proof did not complete"
refocus_line="$(line_number '^sophia_firefox_m8 schema=1 status=stage_complete stage=refocus ')"
focus_away_line="$(awk -v first="$focus_away_action" -v last="$refocus_line" -v firefox="$firefox_surface" '
    NR > first && NR < last && /^sophia_live_wm schema=1 status=focus_reconciled / && $0 !~ ("index: " firefox ",") { print NR; exit }
' "$SESSION_LOG")"
[[ -n "$focus_away_line" ]] || fail "Firefox refocus stage lacks a committed focus transition away"
focus_away_applied="$(line_number_after '^sophia_live_session_input_pipeline schema=1 status=focus_applied source=x11-control$' "$focus_away_line")"
focus_return_action="$(line_number_after '^sophia_live_wm schema=1 status=physical_action_committed action=1$' "$focus_away_applied")"
focus_return_line="$(line_number_after "^sophia_live_wm schema=1 status=focus_reconciled .* surface=SurfaceId \\{ index: ${firefox_surface}," "$focus_return_action")"
focus_return_applied="$(line_number_after '^sophia_live_session_input_pipeline schema=1 status=focus_applied source=x11-control$' "$focus_return_line")"
xi_focus_out="$(line_number_after "sophia_x11_focus_delivery schema=1 .* window=${firefox_surface} focused=false .* xi2_selected=true content=redacted$" "$layout_line")"
xi_focus_in="$(line_number_after "sophia_x11_focus_delivery schema=1 .* window=${firefox_surface} focused=true .* xi2_selected=true content=redacted$" "$xi_focus_out")"
[[ -n "$focus_away_action" && -n "$focus_away_applied" && -n "$focus_return_action"
    && -n "$focus_return_line" && -n "$focus_return_applied" ]] ||
    fail "Firefox refocus action/control ordering evidence is incomplete"
[[ -n "$xi_focus_out" && -n "$xi_focus_in" ]] ||
    fail "Firefox did not receive selected XI2 FocusOut/FocusIn"
(( layout_line < focus_away_action
    && focus_away_action < focus_away_line
    && focus_away_line < focus_away_applied
    && focus_away_applied < focus_return_action
    && focus_return_action < focus_return_line
    && focus_return_line < focus_return_applied
    && focus_return_applied < refocus_line
    && layout_line < xi_focus_out
    && xi_focus_out < xi_focus_in
    && xi_focus_in < refocus_line )) ||
    fail "Firefox DOM refocus is not ordered after focus-away, XI2 out/in, and focus return"
if awk -v first="$layout_line" -v last="$refocus_line" '
    NR > first && NR < last &&
        /^sophia_live_wm schema=1 status=layout_committed / &&
        $0 ~ /moved_surfaces=[1-9][0-9]*/ { found=1 }
    END { exit !found }
' "$SESSION_LOG"; then
    fail "focus-only activity changed surface geometry"
fi
firefox_present_target="$(sed -n "${firefox_present_line}p" "$SESSION_LOG")"
firefox_present_target="$(field "$firefox_present_target" target)" ||
    fail "the post-layout Firefox Present has no target geometry"
if awk -v first="$layout_action" -v last="$refocus_line" -v surface="$firefox_surface" \
    -v expected="$firefox_present_target" '
    NR > first && NR < last &&
        /^sophia_live_session_present schema=(2|4) status=retired / &&
        $0 ~ ("surface=" surface " ") {
            target = ""
            for (i = 1; i <= NF; i++) {
                if ($i ~ /^target=/) {
                    split($i, field, "=")
                    target = field[2]
                }
            }
            if (target != expected) drift=1
        }
    END { exit !drift }
' "$SESSION_LOG"; then
    fail "Firefox Present target drifted during focus-only activity"
fi
dialog_ready_line="$(require_line_number '^sophia_firefox_m8 schema=1 status=dialog_ready content=redacted$' 'Firefox modal never became ready')"
dialog_line="$(line_number '^sophia_firefox_m8 schema=1 status=stage_complete stage=dialog ')"
if awk -v first="$refocus_line" -v last="$dialog_line" '
    NR > first && NR < last &&
        ($0 ~ /^sophia_live_wm .*status=layout_timeout / || $0 ~ /^sophia_live_wm .*status=restarted /) { found=1 }
    END { exit !found }
' "$SESSION_LOG"; then
    fail "Firefox modal interaction timed out or restarted WM policy"
fi
(( refocus_line < dialog_ready_line
    && dialog_ready_line < dialog_line
    && dialog_line < first_exit )) ||
    fail "Firefox modal ready/confirm lifecycle is out of order"
m10_completion="$(grep -E '^sophia_firefox_m10 schema=3 status=complete kitty_checkpoints=[0-9]+ selection_gates=focused content=redacted$' "$SESSION_LOG" | tail -n 1 || true)"
[[ -n "$m10_completion" ]] || fail "Firefox M10 Kitty proof did not complete"
require_eq "$m10_completion" kitty_checkpoints 6

grep -Eq '^sophia_live_wm schema=1 status=session_action_committed .* action=Logout$' \
    "$SESSION_LOG" || fail "normal logout was not committed"
grep -Eq '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$' \
    "$SESSION_LOG" || fail "unexpected X protocol errors were observed"
health="$(grep -E '^sophia_live_session_health schema=1 status=clean ' "$SESSION_LOG" | tail -n 1)"
[[ -n "$health" ]] || fail "clean session health summary is missing"
for assignment in protocol_errors=0 pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false; do
    require_eq "$health" "${assignment%%=*}" "${assignment#*=}"
done
layout_health="$(grep -E '^sophia_live_layout_health schema=2 status=clean ' "$SESSION_LOG" | tail -n 1 || true)"
[[ -n "$layout_health" ]] || fail "clean layout health summary is missing"
for assignment in recovery_extents=0 standing_targets=0 constraint_relayout_pending=false; do
    require_eq "$layout_health" "${assignment%%=*}" "${assignment#*=}"
done
keys="$(grep -E '^sophia_live_session_keys schema=2 status=complete ' "$SESSION_LOG" | tail -n 1)"
[[ -n "$keys" ]] || fail "final key-state summary is missing"
for assignment in pending=0 release_barrier_pending=0 repeat_active_seats=0 repeat_capacity_exhausted=0; do
    require_eq "$keys" "${assignment%%=*}" "${assignment#*=}"
done

mapfile -t completions < <(grep -E '^sophia_live_session schema=(14|15|16|18) status=bounded_complete ' "$SESSION_LOG")
(( ${#completions[@]} == 1 )) || fail "expected exactly one bounded session completion"
completion="${completions[0]}"
require_at_least "$completion" startup_ready_msec 0
for assignment in \
    native_presentation=enabled physical_input=enabled wm_policy=external \
    wm_degraded=false native_submit_failures=0 \
    native_retire_failures=0 native_callback_rejected=0 \
    native_callback_queue_saturated=0 native_in_flight=false \
    native_cleanup_pending=false present_disconnect_failures=0 \
    present_live_sources=0 present_live_fences=0 present_live_transactions=0; do
    require_eq "$completion" "${assignment%%=*}" "${assignment#*=}"
done
completion_restarts="$(field "$completion" wm_restarts)" || fail "completion lacks wm_restarts"
[[ "$completion_restarts" =~ ^[0-9]+$ ]] ||
    fail "wm_restarts is not an integer: $completion_restarts"
(( completion_restarts == admission_restart_count )) ||
    fail "completion reports $completion_restarts WM restarts, observed $admission_restart_count during Firefox admission"
expected="$(field "$completion" input_events_expected)" || fail "completion lacks input_events_expected"
flushed="$(field "$completion" input_events_flushed)" || fail "completion lacks input_events_flushed"
[[ "$expected" == "$flushed" ]] || fail "input queue did not drain: expected=$expected flushed=$flushed"

grep -Eq '^sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed$' \
    "$SESSION_LOG" || fail "application/frontend/authority cleanup was not clean"
grep -Eq '^sophia_session_input_guard schema=2 status=ready .* keyboards=[1-9][0-9]*$' \
    "$GUARD_LOG" || fail "input guard did not discover a keyboard"
grep -Eq '^sophia_session_input_guard schema=1 status=armed$' \
    "$GUARD_LOG" || fail "input guard was not armed"
if grep -Eq '^sophia_session_input_guard schema=1 status=triggered$' "$GUARD_LOG"; then
    fail "run used emergency recovery instead of normal logout"
fi
recovery="$(grep -E '^sophia_tty_recovery schema=3 profile=hagia ' "$RECOVERY_LOG" | tail -n 1)"
[[ -n "$recovery" ]] || fail "Hagia TTY recovery record is missing"
for assignment in termios_restored=true emergency=false session_shutdown=not_requested session_exit_status=none; do
    require_eq "$recovery" "${assignment%%=*}" "${assignment#*=}"
done
kd_before="$(field "$recovery" kd_mode_before)" || fail "recovery lacks kd_mode_before"
kd_after="$(field "$recovery" kd_mode_after)" || fail "recovery lacks kd_mode_after"
[[ "$kd_before" == "$kd_after" ]] || fail "KD mode was not restored"

echo "physical Firefox M10 workflow verified: $SESSION_LOG"

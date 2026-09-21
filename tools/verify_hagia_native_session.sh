#!/usr/bin/env bash
set -euo pipefail

# Verifies one native Hagia session against the bounded product workflow on the
# roadmap's critical path: three terminal launches, a visible focus-next, one
# close, and a normal logout, with Sophia's WM and shell protocols carrying all
# of it.
#
# The promotion this evidence supports is the three native frame slots, so the
# schema-7 block is checked as a balance rather than as a set of present fields:
# every renderer-worker request must have settled as a completion or a bounded
# deferral, no slot may have been leased at completion, and no stale release may
# have been refused.

evidence="${1:?usage: verify_hagia_native_session.sh EVIDENCE [PROOF_TEXT]}"
proof_text="${2:-hagianativeproof}"

# The smallest sampled population a halves comparison can say anything about.
# Twenty is roughly a hundred seconds of a settled session at the five-second
# cadence, which leaves seven readings a side after warmup is dropped.
SOPHIA_MIN_RESOURCE_SAMPLES="${SOPHIA_MIN_RESOURCE_SAMPLES:-20}"
# Resident size is the one sampled figure that includes allocations Sophia does
# not account for. glibc returns freed memory to its arenas rather than to the
# kernel, so RSS drifts upward under identical work; the accounted gauges carry
# no allowance at all.
SOPHIA_RSS_GROWTH_TOLERANCE_KIB="${SOPHIA_RSS_GROWTH_TOLERANCE_KIB:-32768}"

fail() {
    echo "Hagia native session verification failed: $*" >&2
    exit 1
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

count() {
    grep -Ec "$1" "$evidence" || true
}

first_line() {
    grep -nEm1 "$1" "$evidence" | cut -d: -f1
}

require_exactly() {
    local description="$1" pattern="$2" expected="$3" observed
    observed="$(count "$pattern")"
    (( observed == expected )) ||
        fail "expected $expected $description, found $observed"
}

require_line() {
    local description="$1" pattern="$2"
    grep -Eq "$pattern" "$evidence" || fail "missing $description"
}

[[ -s "$evidence" ]] || fail "evidence is missing or empty: $evidence"
[[ "$proof_text" =~ ^[a-z]{1,24}$ ]] ||
    fail "proof text must contain 1-24 lowercase ASCII letters"

# Identity. One line, every signed commit, all three binary digests, and the
# desktop profile the session actually loaded.
#
# schema=2 binds the Narthex commit and names the shell binary narthex.
# schema=1 is the pre-split spelling and is still accepted, because this
# verifier also reads archives written before the split, where the old field
# name means "older", not "wrong". Every archive is re-verified by
# `cargo xtask check`, so refusing schema=1 here would break the build on
# history that is correct for its own time.
identity_pattern_v2='^sophia_hagia_native_identity schema=2 status=bound sophia_commit=[0-9a-f]{40} hagia_commit=[0-9a-f]{40} narthex_commit=[0-9a-f]{40} sophia_sha256=[0-9a-f]{64} hagia_sha256=[0-9a-f]{64} narthex_sha256=[0-9a-f]{64} desktop_profile_sha256=[0-9a-f]{64}$'
identity_pattern_v1='^sophia_hagia_native_identity schema=1 status=bound sophia_commit=[0-9a-f]{40} hagia_commit=[0-9a-f]{40} sophia_sha256=[0-9a-f]{64} hagia_sha256=[0-9a-f]{64} hagia_shell_sha256=[0-9a-f]{64} desktop_profile_sha256=[0-9a-f]{64}$'
if (( $(count "$identity_pattern_v2") == 1 )); then
    identity_pattern="$identity_pattern_v2"
else
    identity_pattern="$identity_pattern_v1"
fi
require_exactly "bound Sophia/Hagia/Narthex identity" "$identity_pattern" 1
identity="$(grep -E "$identity_pattern" "$evidence")"
profile_sha256="$(field "$identity" desktop_profile_sha256)"

# The profile identity must describe the profile that ran. A session started
# with --no-config loads the compiled profile while an exported digest still
# names a file on disk, which is how the switcher gate came to print an identity
# for a profile it was not using.
profile_line="$(grep -E '^sophia_live_desktop_profile schema=1 status=loaded ' "$evidence" || true)"
[[ -n "$profile_line" ]] || fail "session recorded no desktop-profile identity"
[[ "$(field "$profile_line" root_sha256)" == "$profile_sha256" ]] ||
    fail "the loaded desktop profile is not the one bound to this run"

if grep -Eq '(^Error:|panicked at|status=(failed|degraded)([[:space:]]|$))' "$evidence"; then
    fail "evidence contains a Sophia error, panic, or degraded status"
fi

require_line "native WM readiness" \
    '^sophia_live_wm schema=4 status=ready adapter=sophia_wm_v1 socket=session_owned epoch=1 restarts=0$'
require_line "a presented startup output" \
    '^sophia_live_native_startup_output schema=1 status=presented output=[0-9]+ proof=synchronous_modeset submission=1$'

# The physical input path, proven before the workflow while the startup terminal
# was the session's only window.
require_line "exact physical text completion" \
    "^sophia_live_session_input schema=2 status=complete source=physical text=$proof_text expected_events=[1-9][0-9]* matched_events=[1-9][0-9]* pixel_change=true$"

# The workflow itself. Session actions and their physical commits are separate
# facts: the first says policy decided, the second says Sophia committed the
# operator's keypress, and a run missing either did not prove the shortcut path.
require_exactly "committed terminal launches" \
    '^sophia_live_wm schema=1 status=session_action_committed transaction=[1-9][0-9]* action=LaunchTerminal$' 3
require_exactly "committed close action" \
    '^sophia_live_wm schema=1 status=session_action_committed transaction=[1-9][0-9]* action=CloseFocused$' 1
require_exactly "committed logout action" \
    '^sophia_live_wm schema=1 status=session_action_committed transaction=[1-9][0-9]* action=Logout$' 1
launch_admissions="$(count '^sophia_session_app schema=2 status=admitted source=action transaction=[1-9][0-9]* surface=[1-9][0-9]*$')"
(( launch_admissions >= 3 )) ||
    fail "expected three admitted launch surfaces, found $launch_admissions"

# Ordering. Each launch must commit its layout before the next is requested, so
# the commits are read as a sequence rather than as a set: a run that committed
# three launches and laid them out in one late batch is not the ordered commit
# path this gate promotes.
#
# `layout_committed` is the record this path produces. Hagia's workspace model
# remains private policy state, so requiring a separate workspace projection
# would describe a protocol surface that does not exist here.
mapfile -t launch_lines < <(
    grep -nE '^sophia_live_wm schema=1 status=session_action_committed transaction=[1-9][0-9]* action=LaunchTerminal$' \
        "$evidence" | cut -d: -f1
)
mapfile -t layout_lines < <(
    grep -nE '^sophia_live_wm schema=1 status=layout_committed transaction=[1-9][0-9]* surfaces=[0-9]+ moved_surfaces=[0-9]+ configure_deliveries=[0-9]+ outcome=Committed$' \
        "$evidence" | cut -d: -f1
)
(( ${#layout_lines[@]} >= 4 )) ||
    fail "expected at least four committed layouts, found ${#layout_lines[@]}"
for index in 0 1 2; do
    launch="${launch_lines[$index]}"
    settled=false
    for layout in "${layout_lines[@]}"; do
        if (( layout > launch )); then
            settled=true
            break
        fi
    done
    [[ "$settled" == true ]] ||
        fail "terminal launch $((index + 1)) never reached a committed layout"
done

focus_next_line="$(first_line '^sophia_live_wm schema=1 status=physical_action_committed action=1$')"
close_line="$(first_line '^sophia_live_wm schema=1 status=session_action_committed transaction=[1-9][0-9]* action=CloseFocused$')"
logout_line="$(first_line '^sophia_live_wm schema=1 status=session_action_committed transaction=[1-9][0-9]* action=Logout$')"
[[ -n "$focus_next_line" ]] || fail "focus-next was never committed"
(( focus_next_line > launch_lines[2] )) ||
    fail "focus-next was committed before the third terminal launch"
(( close_line > focus_next_line )) ||
    fail "the close was committed before the focus change it follows"
(( logout_line > close_line )) ||
    fail "logout was committed before the close"

# A focus change nobody could see is not the proof this step asks for, so the
# focus-next must be followed by Engine committing focus to a surface.
focus_committed=false
while read -r committed; do
    if (( committed > focus_next_line && committed < close_line )); then
        focus_committed=true
        break
    fi
done < <(grep -nE '^sophia_live_wm schema=1 status=focus_committed transaction=[1-9][0-9]* target=surface$' \
    "$evidence" | cut -d: -f1)
[[ "$focus_committed" == true ]] ||
    fail "focus-next committed no visible focus change"

# A stale policy response is an ordinary scene race and must re-arm. One that
# did not is a policy proposal silently dropped, which is a different failure
# from the recovered races this workload legitimately produces.
if grep -qE '^sophia_live_wm schema=1 status=stale_response_rejected transaction=[1-9][0-9]* reason=[a-z_]+ rearmed=false$' \
    "$evidence"; then
    fail "a stale policy response was rejected without re-arming"
fi

# Shell-role separation. No switcher runs in this workflow, so what is checked
# is the boundary rather than the feature: the broker and the shell are separate
# protected admissions, descriptors reach Engine redacted, and both stop cleanly.
require_exactly "protected metadata-broker admission" \
    '^sophia_live_metadata_broker schema=1 status=ready protected=true peer_pid=[1-9][0-9]* revision=[1-9][0-9]*$' 1
require_exactly "protected Hagia Shell admission" \
    '^sophia_live_metadata_shell schema=1 status=ready protected=true peer_pid=[1-9][0-9]* revision=1 connection_epoch=1$' 1
require_exactly "clean metadata-broker shutdown" \
    '^sophia_live_metadata_broker schema=1 status=stopped transport=disconnected process=terminated$' 1
require_exactly "clean Hagia Shell shutdown" \
    '^sophia_live_metadata_shell schema=1 status=stopped transport=disconnected process=terminated$' 1
require_line "a redacted descriptor commit" \
    '^sophia_live_metadata_broker schema=1 status=descriptor_committed surface=[0-9]+ content=redacted$'
if grep -Eq '(protected metadata (broker|shell) exited|^sophia_live_metadata_(broker|shell) schema=1 status=(failed|transport_failed|candidate_rejected|activation_rejected|unavailable|disconnect_failed) )' "$evidence"; then
    fail "evidence contains a metadata broker or shell failure"
fi
broker_ready_line="$(first_line '^sophia_live_metadata_broker schema=1 status=ready ')"
broker_descriptor_line="$(first_line '^sophia_live_metadata_broker schema=1 status=descriptor_committed ')"
broker_stopped_line="$(first_line '^sophia_live_metadata_broker schema=1 status=stopped ')"
shell_ready_line="$(first_line '^sophia_live_metadata_shell schema=1 status=ready ')"
shell_stopped_line="$(first_line '^sophia_live_metadata_shell schema=1 status=stopped ')"
(( broker_ready_line < broker_descriptor_line && broker_descriptor_line < broker_stopped_line )) ||
    fail "metadata-broker lifecycle is not ready -> descriptor -> stopped"
(( shell_ready_line < shell_stopped_line )) ||
    fail "Hagia Shell lifecycle is not ready -> stopped"

# Session-control transport: a balanced ledger and bounded latency. Input and WM
# work share the owner thread, so an unbounded dwell here is what a session that
# felt unusable looks like in evidence.
mapfile -t session_control_records < <(
    grep -E '^sophia_live_session_control schema=(1|2) status=complete ' "$evidence"
)
(( ${#session_control_records[@]} == 1 )) ||
    fail "expected one session-control completion record"
session_control="${session_control_records[0]}"
for assignment in rejected=0 timed_out=0 unexpected=0 pending=0; do
    [[ " $session_control " == *" $assignment "* ]] ||
        fail "session-control ledger was not clean: $assignment"
done
control_stale_retired=0
if [[ "$(field "$session_control" schema)" == 2 ]]; then
    control_stale_retired="$(field "$session_control" stale_retired)"
fi
(( $(field "$session_control" enqueued) == $(field "$session_control" dispatched) &&
    $(field "$session_control" dispatched) ==
    $(field "$session_control" delivered) + control_stale_retired )) ||
    fail "session-control enqueue, dispatch, and delivery counts diverged"
(( $(field "$session_control" max_queue_dwell_msec) <= 100 &&
    $(field "$session_control" max_ack_msec) <= 100 )) ||
    fail "session-control latency exceeded 100ms"

# Native drain and clean teardown.
require_line "clean native presentation drain" \
    '^sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=none$'
require_line "clean session health" \
    '^sophia_live_session_health schema=1 status=clean protocol_errors=0 pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false$'
require_line "clean output topology" \
    '^sophia_live_output_topology_health schema=1 status=clean quarantined=false$'
require_line "clean process cleanup" \
    '^sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed$'
require_line "zero unexpected protocol errors" \
    '^sophia_live_session_protocol_errors schema=1 expected=[0-9]+ unexpected=0$'
require_line "drained client key state" \
    '^sophia_live_session_keys schema=2 status=complete pending=0 release_barrier_pending=0 .* removed_surface_keys=0 repeat_active_seats=0 .* repeat_capacity_exhausted=0$'

mapfile -t completions < <(
    grep -E '^sophia_live_session schema=(16|18) status=bounded_complete ' "$evidence"
)
(( ${#completions[@]} == 1 )) ||
    fail "expected one completed session, found ${#completions[@]}"
completion="${completions[0]}"
for assignment in \
    native_submit_failures=0 \
    native_retire_failures=0 \
    native_callback_rejected=0 \
    native_callback_queue_saturated=0 \
    native_in_flight=false \
    native_cleanup_pending=false \
    present_disconnect_failures=0 \
    present_live_sources=0 \
    present_live_fences=0 \
    present_live_transactions=0 \
    input_text_match=true \
    input_pixel_change=true \
    wm_restarts=0 \
    wm_degraded=false; do
    [[ " $completion " == *" $assignment "* ]] ||
        fail "completion does not contain $assignment"
done
for key in input_queue_dwell_max_msec native_max_submit_to_page_flip_msec \
    native_max_render_msec native_max_upload_msec; do
    value="$(field "$completion" "$key")" || fail "completion is missing $key"
    [[ "$value" =~ ^[0-9]+$ ]] || fail "completion has nonnumeric $key=$value"
    (( value <= 100 )) || fail "$key exceeded the 100ms promotion budget: $value"
done
(( $(field "$completion" native_nonzero_exports) > 0 )) ||
    fail "the session presented no nonzero content"

# The three-slot promotion evidence. Schema 7 remains accepted because archive
# 0001 is schema-7 evidence and must stay independently verifiable; schema 8
# additionally carries the buffer-age damage outcomes.
mapfile -t resource_lines < <(
    grep -E '^sophia_live_native_resources schema=(7|8|9|10|11|12) status=complete ' "$evidence"
)
(( ${#resource_lines[@]} == 1 )) ||
    fail "expected one schema-7 or schema-8 native resource-lifetime record"
resources="${resource_lines[0]}"
resource_schema="$(field "$resources" schema)"
slot_keys=(frame_slot_acquisitions frame_slot_reuses frame_slot_deferrals
    frame_slot_stale_releases frame_slots_leased frame_slots_high_watermark
    worker_requests worker_completions worker_failures worker_hard_stalls
    worker_release_enqueue_failures)
if (( resource_schema >= 8 )); then
    slot_keys+=(frame_slot_partial_repaints frame_slot_full_repaints
        frame_slot_history_invalidations frame_slot_history_records)
fi
if (( resource_schema >= 9 )); then
    slot_keys+=(max_in_flight_per_output pending_frame_supersessions)
fi
# Schema 10 reports how many renderer threads the session ran and whether any
# result reached an output that did not ask for it. Both are meaningless
# until outputs can share a worker, so earlier evidence owes neither.
if (( resource_schema >= 10 )); then
    slot_keys+=(renderer_workers worker_result_misroutes worker_max_service_skew)
fi
# Schema 11 reports the direct scanout path. Every field is meaningless before
# it existed, so earlier evidence owes none of them; the numbers are checked
# for consistency below rather than for having fired, because an ordinary
# session runs with the path off and must still verify.
if (( resource_schema >= 11 )); then
    slot_keys+=(direct_scanout_attempts direct_scanout_flips direct_scanout_tests
        direct_scanout_test_rejections direct_scanout_refusals
        direct_scanout_fallbacks)
fi
# Schema 12 counts a refusal the backend made for a reason of its own -- a
# format or plane layout it cannot use -- apart from a structural disagreement
# with Engine's proof. Only the second is a defect.
if (( resource_schema >= 12 )); then
    slot_keys+=(direct_scanout_unsupported)
fi
for key in "${slot_keys[@]}"; do
    value="$(field "$resources" "$key")" ||
        fail "resource record is missing $key"
    [[ "$value" =~ ^[0-9]+$ ]] ||
        fail "resource record has nonnumeric $key=$value"
done
# Schema-8 evidence exists to promote damage-limited repaint, and the gate
# enables the feature, so a run in which it never fired is not the run being
# promoted. Schema-7 evidence predates the feature and owes nothing here.
if (( resource_schema >= 8 )); then
    (( $(field "$resources" frame_slot_partial_repaints) >= 1 )) ||
        fail "no frame rendered partially, so the buffer-age boundary was not exercised"
fi
for key in worker_failures worker_hard_stalls worker_release_enqueue_failures \
    frame_slot_stale_releases; do
    (( $(field "$resources" "$key") == 0 )) || fail "$key must be zero"
done
if (( resource_schema >= 11 )); then
    # A refusal means Engine proved a frame the lowered pixels contradict.
    # That is a defect, not ordinary ineligibility: an ineligible frame never
    # becomes an attempt at all, so this counter has no benign nonzero value.
    (( $(field "$resources" direct_scanout_refusals) == 0 )) ||
        fail "Engine's direct-scanout proof disagreed with the frame it lowered"
    direct_attempts="$(field "$resources" direct_scanout_attempts)"
    direct_flips="$(field "$resources" direct_scanout_flips)"
    direct_fallbacks="$(field "$resources" direct_scanout_fallbacks)"
    # Every attempt ends exactly one of three ways, and the third -- still
    # outstanding at session end -- is at most one per head.
    direct_unsupported=0
    if (( resource_schema >= 12 )); then
        direct_unsupported="$(field "$resources" direct_scanout_unsupported)"
    fi
    (( direct_flips + direct_fallbacks + direct_unsupported <= direct_attempts )) ||
        fail "direct scanout settled more attempts than it made"
    (( $(field "$resources" direct_scanout_test_rejections) <=
        $(field "$resources" direct_scanout_tests) )) ||
        fail "direct scanout refused more validating commits than it issued"
    # A flip is only lawful under a validating commit, so a session that
    # flipped without ever asking the driver is one that skipped the question.
    if (( direct_flips > 0 )); then
        (( $(field "$resources" direct_scanout_tests) > 0 )) ||
            fail "a client buffer reached a plane with no validating commit"
    fi
fi
(( $(field "$resources" frame_slot_acquisitions) > 0 )) ||
    fail "the native frame-slot pool was never acquired"
(( $(field "$resources" frame_slots_high_watermark) > 0 )) ||
    fail "the native frame-slot pool reported no live ownership"
# Three slots are owned per physical head, and the record sums its watermark
# across the per-head workers, so the session-wide ceiling is three per head
# that presented. Comparing the aggregate against three would fail every
# two-output run on hardware that has two outputs.
heads="$(grep -oE '^sophia_live_native_startup_output schema=1 status=presented output=[0-9]+ ' "$evidence" \
    | grep -oE 'output=[0-9]+' | sort -u | wc -l)"
(( heads > 0 )) || fail "no presented startup output identifies a physical head"
(( $(field "$resources" frame_slots_high_watermark) <= heads * 3 )) ||
    fail "the native frame-slot pool exceeded three slots per presented head"

# One KMS submission in flight per head. A mirror output holds one per head by
# design, so the bound is the presented head count rather than one: asserting
# one per output would fail every mirror session and would be wrong, not
# strict. `heads` is counted from the presented startup outputs above.
if (( resource_schema >= 9 )); then
    depth="$(field "$resources" max_in_flight_per_output)"
    (( depth >= 1 )) ||
        fail "no KMS submission was ever in flight, so the session presented nothing"
    (( depth <= heads )) ||
        fail "an output held $depth concurrent KMS submissions across $heads presented heads"
fi
# A result may only reach the output that asked for it. Per-output reply
# channels make the alternative unreachable rather than unlikely, so anything
# here means the routing that structure guarantees was subverted.
if (( resource_schema >= 10 )); then
    (( $(field "$resources" worker_result_misroutes) == 0 )) ||
        fail "a renderer result reached an output that did not request it"
    # Bounded inter-output service skew, asserted only where outputs actually
    # share a thread. Independent threads interleave as the GPU allows, and a
    # FIFO bound over them would be a claim about parallelism rather than
    # about fairness. The figure is sampled on the tick, so it is a lower
    # bound: exceeding the bound is real, staying under it is evidence rather
    # than proof.
    workers="$(field "$resources" renderer_workers)"
    (( workers >= 1 )) ||
        fail "session reported $workers renderer threads"
    if (( workers < heads )); then
        skew="$(field "$resources" worker_max_service_skew)"
        (( skew <= heads - 1 )) ||
            fail "one output was passed over $skew times across $heads heads sharing $workers threads"
    fi
fi
# No leaked lease. A slot still leased after the session drained means a page
# flip retired without releasing its buffer, which is exactly the failure the
# three-slot ledger exists to make impossible. Nothing else checks this today.
(( $(field "$resources" frame_slots_leased) == 0 )) ||
    fail "a native frame slot was still leased at completion"
(( $(field "$resources" worker_requests) ==
    $(field "$resources" worker_completions) + $(field "$resources" frame_slot_deferrals) )) ||
    fail "renderer-worker requests did not settle as completion or bounded deferral"

# The resource sampler ran for as long as the session did.
#
# This gate drives a ramp, not a steady state: three launches, a focus, a close,
# a logout. Every live gauge legitimately climbs while the operator is acquiring
# windows, so comparing halves of it would refuse a healthy run for doing the
# work it was asked to do. A first version of this rule did exactly that, and the
# physical run is what showed it -- snapshot_live_entries went 2, 2, 6, 8 across
# a twenty-four-second session because three terminals appeared.
#
# Growth belongs to a workload that settles, and lives in the soak verifier.
# What is checkable here is that the sampler was alive throughout, which is the
# difference between a short session and a lost sampler. The completion record's
# zero-live-entry rules already own whether this session drained.
steady_state_line="$(grep -E '^sophia_live_resource_steady_state schema=1 status=complete ' \
    "$evidence" || true)"
if [[ -n "$steady_state_line" ]]; then
    sample_count="$(field "$steady_state_line" samples)"
    sample_interval="$(field "$steady_state_line" interval_msec)"
    [[ "$sample_count" =~ ^[0-9]+$ && "$sample_interval" =~ ^[1-9][0-9]*$ ]] ||
        fail "the steady-state record has a nonnumeric sample count or interval"
    recorded="$(grep -cE '^sophia_live_resource_sample schema=1 seq=[0-9]+ ' "$evidence" || true)"
    (( recorded == sample_count )) ||
        fail "the session claimed $sample_count resource samples and recorded $recorded"
    elapsed_msec="$(field "$completion" elapsed_msec)"
    if [[ "$elapsed_msec" =~ ^[0-9]+$ ]] && (( sample_count > 0 || elapsed_msec >= sample_interval )); then
        # One sample per interval, give or take the pass the session ended on.
        expected=$(( elapsed_msec / sample_interval ))
        (( sample_count >= expected - 1 && sample_count <= expected + 1 )) ||
            fail "the sampler recorded $sample_count samples across ${elapsed_msec}ms, where its ${sample_interval}ms cadence owes about $expected"
    fi
fi

# Which hardware cursor path the session took, and which one it asked for. The
# record was emitted for two archives before anything read it, so a run could
# take the legacy ioctl on a card that refused the plane and nothing would say
# so.
#
# Absence is not failed here. Archives 0001 through 0003 predate the cursor
# tranche entirely and must stay independently verifiable, and this file cannot
# tell evidence that lost the record from evidence written before it existed.
# Requiring a current run to carry it belongs to the gate, which knows it is
# running current code; this checks that whatever the evidence does say is
# consistent.
cursor_path_line="$(grep -E '^sophia_live_cursor_path schema=2 status=selected ' \
    "$evidence" || true)"
if [[ -n "$cursor_path_line" ]]; then
    [[ "$cursor_path_line" =~ ^sophia_live_cursor_path\ schema=2\ status=selected\ requested=(atomic_plane|legacy_ioctl)\ path=(atomic_plane|legacy_ioctl)$ ]] ||
        fail "the cursor-path record does not name both a request and a path"
    cursor_requested="$(field "$cursor_path_line" requested)"
    cursor_taken="$(field "$cursor_path_line" path)"
    # Only one direction is a defect. Asking for the plane and getting the
    # ioctl is the probe refusing a card, which is the fallback this row
    # retained on purpose. Asking for the ioctl and getting the plane is the
    # preference being ignored.
    if [[ "$cursor_requested" == legacy_ioctl && "$cursor_taken" != legacy_ioctl ]]; then
        fail "the session asked for the legacy cursor and took $cursor_taken"
    fi
fi

# Exact TTY restoration, which the runner records after the session returns.
require_line "exact TTY recovery" \
    '^sophia_tty_recovery schema=3 profile=hagia kd_mode_before=[^ ]+ kd_mode_after=[^ ]+ termios_restored=true emergency=false session_shutdown=not_requested session_exit_status=none$'

# The run the guide asked for, against the run that happened. The guide's waits
# are the only statement of expected totals; restating them here would make this
# file a second owner of the same fact.
guide="${SOPHIA_HAGIA_NATIVE_GUIDE:-$(dirname "$0")/fixtures/hagia_native_session_guide.sh}"
[[ -r "$guide" ]] ||
    fail "the native guide is unreadable, so its action totals cannot be checked: $guide"

declare -A expected_actions=()
while read -r action expectation; do
    # Each step waits for a cumulative count, so the largest is the run's total.
    if [[ -z "${expected_actions[$action]:-}" ]] || (( expectation > expected_actions[$action] )); then
        expected_actions[$action]="$expectation"
    fi
done < <(grep -oE '^[[:space:]]*wait_for_action_count [0-9]+ [0-9]+' "$guide" | awk '{ print $2, $3 }')
(( ${#expected_actions[@]} != 0 )) ||
    fail "the native guide requested no actions, which cannot be right"

declare -A committed_actions=()
while read -r observed action; do
    committed_actions[$action]="$observed"
done < <(grep -oE '^sophia_live_wm schema=1 status=physical_action_committed action=[0-9]+$' "$evidence" \
    | sed 's/.*action=//' | sort -n | uniq -c | awk '{ print $1, $2 }')

action_total_failures=0
for action in "${!expected_actions[@]}"; do
    observed="${committed_actions[$action]:-0}"
    if (( observed != expected_actions[$action] )); then
        echo "action $action was committed $observed times; the guide asked for ${expected_actions[$action]}" >&2
        action_total_failures=$((action_total_failures + 1))
    fi
done
for action in "${!committed_actions[@]}"; do
    if [[ -z "${expected_actions[$action]:-}" ]]; then
        echo "action $action was committed ${committed_actions[$action]} times but the guide never asked for it" >&2
        action_total_failures=$((action_total_failures + 1))
    fi
done
(( action_total_failures == 0 )) ||
    fail "the session that ran is not the session the guide specified; re-run the proof"

echo "Hagia native session evidence passed"

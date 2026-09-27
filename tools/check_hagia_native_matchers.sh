#!/bin/sh
set -eu

# Offline proof that the native-session guide, verifier, archiver, and archive
# verifier work before any hardware is taken over. A physical gate costs a
# session and an operator; every mistake caught here is one that does not.
#
# The evidence below is synthesized rather than captured, so the mutations are
# the point: each required line is deleted in turn and the verifier must reject
# what remains, and each failure mode the gate exists to catch is injected and
# must be refused.

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
temp_dir=$(mktemp -d)
trap 'rm -rf -- "$temp_dir"' EXIT HUP INT TERM

proof_text=hagianativeproof
guide="$root_dir/tools/fixtures/hagia_native_session_guide.sh"
verifier="$root_dir/tools/verify_hagia_native_session.sh"

# Source invariants the guide's instructions depend on. A binding renamed in the
# compiled profile without the guide following it would produce a run whose
# keystrokes commit nothing, which reads on screen as Sophia ignoring the
# operator.
for binding in \
    'bind "Super+Return" "session:spawn-terminal"' \
    'bind "Super+j" "policy:focus-next"' \
    'bind "Super+q" "session:close-window"' \
    'bind "Ctrl+Alt+Delete" "session:logout"'; do
    grep -Fq "$binding" "$root_dir/crates/sophia-config/src/desktop_profile.rs" || {
        echo "the compiled desktop profile no longer binds: $binding" >&2
        exit 1
    }
done

# The gate must hand the session an explicit profile and refuse without one.
# `--no-config` runs the compiled profile while an exported digest still names a
# file on disk, which is how a gate comes to print an identity for a profile it
# never loaded. The runner turns SOPHIA_DESKTOP_PROFILE into --desktop-profile,
# so the gate's obligation is to bind it, not to spell the flag.
grep -Fq 'SOPHIA_DESKTOP_PROFILE="$desktop_profile"' \
    "$root_dir/tools/hagia_native_session_gate.sh" || {
    echo "the native gate does not pass an explicit desktop profile" >&2
    exit 1
}
grep -Fq 'set SOPHIA_DESKTOP_PROFILE to the absolute Hagia profile' \
    "$root_dir/tools/hagia_native_session_gate.sh" || {
    echo "the native gate does not refuse an unset desktop profile" >&2
    exit 1
}
if grep -v '^[[:space:]]*#' "$root_dir/tools/hagia_native_session_gate.sh" \
    | grep -Fq -- '--no-config'; then
    echo "the native gate must not run the compiled profile behind a bound digest" >&2
    exit 1
fi
# Exact TTY recovery is an exit criterion, and only the runner produces it.
grep -Fq 'SOPHIA_TTY_PROFILE=hagia' "$root_dir/tools/hagia_native_session_gate.sh" || {
    echo "the native gate does not route through the hagia runner profile" >&2
    exit 1
}
# `--expect-physical-text` without `--max-runtime-ms` or `--max-ticks` is
# refused during argument validation, before any window appears. The runner
# passes no runtime bound of its own because ordinary sessions have no
# lifetime, so the gate must supply one. This restates a rule that lives in
# `PersistentXtermSessionConfig::from_args`, which is duplication worth its
# keep: nothing else here reaches the real parser, and the omission already
# cost one physical attempt that ended before Kitty appeared.
if grep -Fq -- '--expect-physical-text' "$root_dir/tools/hagia_native_session_gate.sh" \
    && ! grep -Eq -- '--(max-runtime-ms|max-ticks)' "$root_dir/tools/hagia_native_session_gate.sh"; then
    echo "the native gate requests an input proof without a bounded runtime" >&2
    exit 1
fi
# With a physical text proof requested, a normal session requires the terminal
# action to name its single startup application and refuses the proof otherwise.
# Splitting the guide's terminal from the workflow's terminal therefore cannot
# work, however tempting it looks: that split cost one physical attempt, which
# ended at argument validation before any window appeared.
if grep -v '^[[:space:]]*#' "$root_dir/tools/hagia_native_session_gate.sh" \
    | grep -Fq -- '--session-action-app='; then
    echo "the native gate overrides the terminal action while requesting a physical text proof" >&2
    exit 1
fi
# Damage-limited repaint is the default, so the gate no longer turns it on.
# What it may not do is turn it off: the verifier requires a frame that
# rendered partially, and a gate that opted out would fail that check for a
# reason the operator would have to guess at from the session log.
if grep -v '^[[:space:]]*#' "$root_dir/tools/hagia_native_session_gate.sh" \
    | grep -Fq 'SOPHIA_ENABLE_BUFFER_AGE_DAMAGE=0'; then
    echo "the native gate disables damage-limited repaint for its promotion run" >&2
    exit 1
fi
# So the guide stands down instead. The gate must give it something to claim.
grep -Fq 'SOPHIA_HAGIA_NATIVE_GUIDE_CLAIM="$guide_claim"' \
    "$root_dir/tools/hagia_native_session_gate.sh" || {
    echo "the native gate does not give the guide a claim to stand down against" >&2
    exit 1
}
for step in \
    'Press Super+Return once.' \
    'Press Super+J once.' \
    'Press Super+q once to close the focused terminal.' \
    'Press Ctrl+Alt+Delete once to log out normally.'; do
    grep -Fq "$step" "$guide" || {
        echo "the native guide no longer instructs: $step" >&2
        exit 1
    }
done

evidence="$temp_dir/evidence.log"
proof_result="$temp_dir/proof.result"
profile_sha256=dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd

# The evidence below is a real session, not an invented one. It is the record
# set of the run that completed the workflow on hardware, reduced to the lines
# this verifier reads and with its identity and profile digest replaced. An
# earlier fixture was written from what the verifier expected rather than from
# what a session emits, so it agreed with the verifier about a record that no
# native session produces and both were wrong together.
printf '%s\n' \
    "sophia_live_desktop_profile schema=1 status=loaded mode=packaged-promotion generation=1 digest=93db9214 root_sha256=$profile_sha256 sources=1" \
    'sophia_live_wm schema=4 status=ready adapter=sophia_wm_v1 socket=session_owned epoch=1 restarts=0' \
    'sophia_live_metadata_broker schema=1 status=ready protected=true peer_pid=30552 revision=2' \
    'sophia_live_metadata_shell schema=1 status=ready protected=true peer_pid=30555 revision=1 connection_epoch=1' \
    'sophia_live_cursor_path schema=2 status=selected requested=atomic_plane path=atomic_plane' \
    'sophia_live_native_startup_output schema=1 status=presented output=1 proof=synchronous_modeset submission=1' \
    'sophia_live_native_startup_output schema=1 status=presented output=2 proof=synchronous_modeset submission=1' \
    'sophia_live_wm schema=1 status=layout_committed transaction=2 surfaces=0 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=3 surfaces=0 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_metadata_broker schema=1 status=descriptor_committed surface=2097166 content=redacted' \
    'sophia_live_wm schema=1 status=layout_committed transaction=4 surfaces=1 moved_surfaces=1 configure_deliveries=1 outcome=Committed' \
    'sophia_live_wm schema=1 status=focus_committed transaction=4 target=surface' \
    'sophia_live_wm schema=1 status=layout_committed transaction=5 surfaces=1 moved_surfaces=1 configure_deliveries=1 outcome=Committed' \
    'sophia_live_session_input schema=2 status=complete source=physical text=hagianativeproof expected_events=34 matched_events=34 pixel_change=true' \
    'sophia_live_wm schema=1 status=layout_committed transaction=6 surfaces=1 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=physical_action_committed action=29' \
    'sophia_live_wm schema=1 status=layout_committed transaction=7 surfaces=1 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=session_action_committed transaction=7 action=LaunchTerminal' \
    'sophia_live_wm schema=1 status=stale_response_rejected transaction=8 reason=scene_advanced rearmed=true' \
    'sophia_live_metadata_broker schema=1 status=descriptor_committed surface=4194318 content=redacted' \
    'sophia_live_wm schema=1 status=layout_committed transaction=9 surfaces=2 moved_surfaces=2 configure_deliveries=2 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=10 surfaces=2 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=11 surfaces=2 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_session_app schema=2 status=admitted source=action transaction=7 surface=4194318' \
    'sophia_live_wm schema=1 status=layout_committed transaction=12 surfaces=2 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=13 surfaces=2 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=physical_action_committed action=29' \
    'sophia_live_wm schema=1 status=layout_committed transaction=14 surfaces=2 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=session_action_committed transaction=14 action=LaunchTerminal' \
    'sophia_live_wm schema=1 status=stale_response_rejected transaction=15 reason=scene_advanced rearmed=true' \
    'sophia_live_wm schema=1 status=layout_committed transaction=16 surfaces=3 moved_surfaces=3 configure_deliveries=3 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=17 surfaces=3 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=18 surfaces=3 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_session_app schema=2 status=admitted source=action transaction=14 surface=6291470' \
    'sophia_live_wm schema=1 status=layout_committed transaction=19 surfaces=3 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=20 surfaces=3 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=physical_action_committed action=29' \
    'sophia_live_wm schema=1 status=layout_committed transaction=21 surfaces=3 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=session_action_committed transaction=21 action=LaunchTerminal' \
    'sophia_live_wm schema=1 status=stale_response_rejected transaction=22 reason=scene_advanced rearmed=true' \
    'sophia_live_wm schema=1 status=layout_committed transaction=23 surfaces=4 moved_surfaces=4 configure_deliveries=4 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=24 surfaces=4 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=25 surfaces=4 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_session_app schema=2 status=admitted source=action transaction=21 surface=8388622' \
    'sophia_live_wm schema=1 status=layout_committed transaction=26 surfaces=4 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=27 surfaces=4 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=physical_action_committed action=1' \
    'sophia_live_wm schema=1 status=focus_committed transaction=27 target=surface' \
    'sophia_live_wm schema=1 status=layout_committed transaction=28 surfaces=4 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=physical_action_committed action=31' \
    'sophia_live_wm schema=1 status=layout_committed transaction=29 surfaces=4 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=session_action_committed transaction=29 action=CloseFocused' \
    'sophia_live_wm schema=1 status=stale_response_rejected transaction=30 reason=scene_advanced rearmed=true' \
    'sophia_live_wm schema=1 status=layout_committed transaction=31 surfaces=3 moved_surfaces=3 configure_deliveries=3 outcome=Committed' \
    'sophia_live_wm schema=1 status=focus_committed transaction=31 target=surface' \
    'sophia_live_wm schema=1 status=layout_committed transaction=32 surfaces=3 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=layout_committed transaction=33 surfaces=3 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=physical_action_committed action=32' \
    'sophia_live_wm schema=1 status=layout_committed transaction=34 surfaces=3 moved_surfaces=0 configure_deliveries=0 outcome=Committed' \
    'sophia_live_wm schema=1 status=session_action_committed transaction=34 action=Logout' \
    'sophia_live_session_native_suspend schema=2 outcome=drained drained=true abandoned_scanouts=0 skipped_present=none' \
    'sophia_live_native_resources schema=9 status=complete target_creations=237 pipeline_creations=237 frame_surface_creations=237 cpu_target_creations=0 dmabuf_target_creations=231 composition_target_creations=6 composition_target_reuses=257 generation_replacements=0 recovery_replacements=0 snapshot_captures=231 snapshot_promotions=231 snapshot_rollbacks=0 snapshot_evictions=231 snapshot_live_entries=0 snapshot_live_bytes=0 import_cache_imports=410 import_cache_hits=244 import_cache_evictions=410 import_cache_live_entries=0 import_cache_descriptor_mismatches=0 import_cache_capacity_rejections=0 exact_nearest_draws=1148 sharp_downscale_draws=0 sharp_upscale_draws=0 linear_fallback_draws=0 worker_requests=263 worker_completions=263 worker_failures=0 worker_soft_stalls=0 worker_hard_stalls=0 worker_release_enqueue_failures=0 frame_slot_acquisitions=263 frame_slot_reuses=257 frame_slot_deferrals=0 frame_slot_stale_releases=0 frame_slots_leased=0 frame_slots_high_watermark=6 max_in_flight_per_output=2 pending_frame_supersessions=41 frame_slot_partial_repaints=118 frame_slot_full_repaints=145 frame_slot_history_invalidations=2 frame_slot_history_records=261 max_worker_request_msec=61' \
    'sophia_live_session_health schema=1 status=clean protocol_errors=0 pending_wm=0 pending_actions=0 pending_input=0 wm_degraded=false' \
    'sophia_live_output_topology_health schema=1 status=clean quarantined=false' \
    'sophia_live_session_protocol_errors schema=1 expected=0 unexpected=0' \
    'sophia_live_session schema=16 status=bounded_complete display=:292 elapsed_msec=41802 startup_ready_msec=602 session_ticks=32549 authority_batches=392 authority_transactions=235 authority_queue_capacity=256 authority_batches_dropped=0 backend_ticks=1153 runtime_committed=228 runtime_surfaces=3 cpu_layers=0 cpu_nonzero_pixel_bytes=13 cpu_max_nonzero_pixel_bytes=143628 cpu_nonzero_frames=245 cpu_checksum=17049933347202034697 cpu_max_compose_msec=5 injected_input=false input_events_expected=45 input_events_flushed=45 input_flush_latency_msec=1 input_pixel_change=true input_text_match=true input_presented_latency_msec=34 input_dispatch_max_gap_msec=1 input_queue_max_depth=3 input_queue_dwell_max_msec=1 physical_events=57 physical_keys_routed=44 pointer_pixel_change=false physical_pointer_events=0 physical_pointer_routed=0 pointer_proof=disabled native_presentation=enabled native_submissions=259 native_submit_deferred=1383 native_submit_failures=0 native_retirements=257 native_retire_failures=0 native_max_in_flight_ticks=0 native_max_submit_to_page_flip_msec=17 native_max_upload_msec=0 native_max_target_create_msec=9 native_max_frame_surface_create_msec=0 native_max_render_msec=28 native_target_creations=237 native_target_recreations=0 native_pipeline_creations=237 native_frame_surface_creations=237 native_frame_uploads=0 native_callback_accepted=257 native_callback_rejected=0 native_callback_queue_saturated=0 native_nonzero_exports=259 native_mixed_exports=263 native_export_attempts=263 native_in_flight=false native_cleanup_pending=false physical_input=enabled wm_policy=external wm_requests=28 wm_committed=24 wm_restarts=0 wm_degraded=false namespace_profile=classic_shared output_update=disabled output_notifications=0 surface_resize=disabled present_complete_copy=231 present_complete_flip=0 present_complete_skip=4 present_idle=235 present_complete_routed=235 present_idle_routed=235 present_route_failures=0 present_idle_fence_triggers=235 present_disconnect_sources=6 present_disconnect_fences=6 present_disconnect_failures=0 present_live_sources=0 present_live_fences=0 present_live_transactions=0 present_acquire_waits=0 present_controlled_rejections=0' \
    'sophia_live_session_control schema=2 status=complete enqueued=26 dispatched=26 delivered=26 stale_retired=0 rejected=0 timed_out=0 unexpected=0 pending=0 peak_depth=5 max_queue_dwell_msec=2 max_ack_msec=1' \
    'sophia_live_session_keys schema=2 status=complete pending=0 release_barrier_pending=0 peak_pressed=2 synthetic_releases=3 state_only_releases=1 orphan_releases_suppressed=2 removed_surface_keys=0 repeat_active_seats=0 repeat_armed=17 repeat_routed=0 repeat_pulses=0 repeat_coalesced=0 repeat_cancelled=15 repeat_capacity_exhausted=0' \
    'sophia_live_session_cleanup schema=1 status=clean app_groups=0 frontend_workers=0 namespace=revoked xauthority=removed' \
    'sophia_live_metadata_shell schema=1 status=stopped transport=disconnected process=terminated' \
    'sophia_live_metadata_broker schema=1 status=stopped transport=disconnected process=terminated' \
    'sophia_tty_recovery schema=3 profile=hagia kd_mode_before=0 kd_mode_after=0 termios_restored=true emergency=false session_shutdown=not_requested session_exit_status=none' \
    "sophia_hagia_native_identity schema=2 status=bound sophia_commit=1111111111111111111111111111111111111111 hagia_commit=2222222222222222222222222222222222222222 narthex_commit=3333333333333333333333333333333333333333 sophia_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa hagia_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb narthex_sha256=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc desktop_profile_sha256=$profile_sha256" \
    >"$evidence"

# A terminal launched by the workflow runs this same script and must stand down
# rather than drive the proof. The evidence here is empty, so a guide that kept
# going would abort on its first wait; standing down runs the replacement shell
# instead, which this test makes `true`. Exit 0 therefore means it stood down.
claim="$temp_dir/startup.claim"
: >"$claim"
set +e
timeout 30s env \
    SOPHIA_LIVE_SESSION_PERSISTENT_EVIDENCE="$temp_dir/empty.log" \
    SOPHIA_INPUT_PROOF_RESULT="$temp_dir/unused.result" \
    SOPHIA_HAGIA_NATIVE_TEXT="$proof_text" \
    SOPHIA_HAGIA_NATIVE_GUIDE_CLAIM="$claim" \
    SHELL=/usr/bin/true \
    "$guide" </dev/null >/dev/null 2>&1
standdown_status=$?
set -e
if [ "$standdown_status" -ne 0 ]; then
    echo "a workflow terminal did not stand down into a shell: $standdown_status" >&2
    exit 1
fi
if [ -e "$temp_dir/unused.result" ]; then
    echo "a workflow terminal wrote the proof result it should never reach" >&2
    exit 1
fi
rm -f "$claim"

# The guide drives this evidence to completion: every step it waits for is
# present, so it must finish rather than stall. A guide that cannot cross its own
# passing log would hang an operator in front of a working session.
set +e
printf '%s\n' "$proof_text" | timeout 30s env \
    SOPHIA_LIVE_SESSION_PERSISTENT_EVIDENCE="$evidence" \
    SOPHIA_INPUT_PROOF_RESULT="$proof_result" \
    SOPHIA_HAGIA_NATIVE_TEXT="$proof_text" \
    SOPHIA_HAGIA_NATIVE_GUIDE_CLAIM="$claim" \
    "$guide" >/dev/null
guide_status=$?
set -e
if [ ! -e "$claim" ]; then
    echo "the startup guide did not claim its terminal" >&2
    exit 1
fi
if [ "$guide_status" -ne 0 ]; then
    echo "the native guide did not cross its own passing evidence: $guide_status" >&2
    exit 1
fi
if [ "$(cat "$proof_result" 2>/dev/null || true)" != "$proof_text" ]; then
    echo "the native guide did not record the typed proof phrase" >&2
    exit 1
fi

# The guide's waits must be bounded. An unbounded wait turns a session that can
# no longer produce a line into a hang instead of a failure, which is how the
# switcher gate became unrunnable without saying so.
if grep -Eq 'while .*grep -Eq .*; do$' "$guide"; then
    echo "the native guide contains an unbounded evidence wait" >&2
    exit 1
fi

"$verifier" "$evidence" "$proof_text" >/dev/null

# Every required line, deleted one at a time.
for missing in \
    'sophia_live_desktop_profile schema=1 status=loaded' \
    'sophia_live_wm schema=4 status=ready' \
    'sophia_live_native_startup_output schema=1 status=presented' \
    'sophia_live_metadata_broker schema=1 status=ready' \
    'sophia_live_metadata_broker schema=1 status=descriptor_committed' \
    'sophia_live_metadata_shell schema=1 status=ready' \
    'sophia_live_metadata_shell schema=1 status=stopped' \
    'sophia_live_metadata_broker schema=1 status=stopped' \
    'sophia_live_session_input schema=2 status=complete' \
    'sophia_live_wm schema=1 status=session_action_committed transaction=21 action=LaunchTerminal' \
    'sophia_live_wm schema=1 status=session_action_committed transaction=29 action=CloseFocused' \
    'sophia_live_wm schema=1 status=session_action_committed transaction=34 action=Logout' \
    'sophia_session_app schema=2 status=admitted source=action transaction=21' \
    'sophia_live_wm schema=1 status=physical_action_committed action=1' \
    'sophia_live_session_control schema=2 status=complete' \
    'sophia_live_session_native_suspend schema=2' \
    'sophia_live_session_keys schema=2 status=complete' \
    'sophia_live_session schema=16 status=bounded_complete' \
    'sophia_live_native_resources schema=9 status=complete' \
    'sophia_live_session_health schema=1 status=clean' \
    'sophia_live_output_topology_health schema=1 status=clean' \
    'sophia_live_session_cleanup schema=1 status=clean' \
    'sophia_live_session_protocol_errors schema=1' \
    'sophia_tty_recovery schema=3 profile=hagia' \
    'sophia_hagia_native_identity schema=2 status=bound'; do
    rejected="$temp_dir/rejected.log"
    grep -vF "$missing" "$evidence" >"$rejected"
    if "$verifier" "$rejected" "$proof_text" >/dev/null 2>&1; then
        echo "the native verifier accepted evidence without: $missing" >&2
        exit 1
    fi
done

# A mutation rejected for the wrong reason proves nothing about the check it was
# written for, so each one states the reason it expects. Without this, a sed that
# silently stopped matching would still look like a passing negative case as long
# as the evidence failed some other way.
reject_mutation() {
    description="$1"
    mutated="$2"
    expected="$3"
    reason="$("$verifier" "$mutated" "$proof_text" 2>&1 >/dev/null || true)"
    if "$verifier" "$mutated" "$proof_text" >/dev/null 2>&1; then
        echo "the native verifier accepted $description" >&2
        exit 1
    fi
    case "$reason" in
        *"$expected"*) ;;
        *)
            echo "the native verifier rejected $description for the wrong reason:" >&2
            echo "  expected: $expected" >&2
            echo "  observed: $reason" >&2
            exit 1
            ;;
    esac
}

# A slot still leased after the session drained is a page flip that retired
# without releasing its buffer. Nothing else in the tree checks this today.
leaked="$temp_dir/leaked.log"
sed 's/frame_slots_leased=0/frame_slots_leased=1/' "$evidence" >"$leaked"
reject_mutation "a leaked frame-slot lease" "$leaked" \
    "a native frame slot was still leased at completion"

stale="$temp_dir/stale.log"
sed 's/frame_slot_stale_releases=0/frame_slot_stale_releases=2/' "$evidence" >"$stale"
reject_mutation "a refused stale slot release" "$stale" \
    "frame_slot_stale_releases must be zero"

# Requests must settle as completions or bounded deferrals. A request that did
# neither is a frame the renderer silently dropped.
unbalanced="$temp_dir/unbalanced.log"
sed 's/worker_completions=263/worker_completions=262/' "$evidence" >"$unbalanced"
reject_mutation "an unbalanced renderer-worker ledger" "$unbalanced" \
    "did not settle as completion or bounded deferral"

overcapacity="$temp_dir/overcapacity.log"
sed 's/frame_slots_high_watermark=6/frame_slots_high_watermark=7/' "$evidence" >"$overcapacity"
reject_mutation "a frame-slot pool above three slots per head" "$overcapacity" \
    "exceeded three slots per presented head"

slow="$temp_dir/slow.log"
sed 's/max_ack_msec=1$/max_ack_msec=140/' "$evidence" >"$slow"
reject_mutation "session-control latency above its budget" "$slow" \
    "session-control latency exceeded 100ms"

diverged="$temp_dir/diverged.log"
sed 's/delivered=26 stale_retired=0/delivered=25 stale_retired=0/' "$evidence" >"$diverged"
reject_mutation "a session-control ledger that did not balance" "$diverged" \
    "enqueue, dispatch, and delivery counts diverged"

# Ordering, not just presence. A focus change committed before the launches, or
# a close before the focus change, is not the workflow this gate specifies.
reordered="$temp_dir/reordered.log"
grep -vF 'sophia_live_wm schema=1 status=physical_action_committed action=1' "$evidence" \
    | awk '
        { print }
        /^sophia_live_metadata_broker schema=1 status=descriptor_committed / {
            print "sophia_live_wm schema=1 status=physical_action_committed action=1"
        }
    ' >"$reordered"
reject_mutation "a focus change committed before its launches" "$reordered" \
    "focus-next was committed before the third terminal launch"

# The checks that read this run's real shape. A launch whose layout never
# committed, a focus-next that changed no committed focus, and a stale response
# that did not re-arm are each a different failure from the recovered scene
# races this workload legitimately produces.
unsettled="$temp_dir/unsettled.log"
awk '
    /^sophia_live_wm schema=1 status=session_action_committed transaction=21 action=LaunchTerminal$/ { third = 1 }
    third && /^sophia_live_wm schema=1 status=layout_committed / { next }
    { print }
' "$evidence" >"$unsettled"
reject_mutation "a launch whose layout never committed" "$unsettled" \
    "terminal launch 3 never reached a committed layout"

unfocused="$temp_dir/unfocused.log"
awk '
    /^sophia_live_wm schema=1 status=physical_action_committed action=1$/ { focusing = 1 }
    /^sophia_live_wm schema=1 status=session_action_committed transaction=29 action=CloseFocused$/ { focusing = 0 }
    focusing && /^sophia_live_wm schema=1 status=focus_committed / { next }
    { print }
' "$evidence" >"$unfocused"
reject_mutation "a focus-next that changed no committed focus" "$unfocused" \
    "focus-next committed no visible focus change"

unarmed="$temp_dir/unarmed.log"
sed 's/status=stale_response_rejected transaction=8 reason=scene_advanced rearmed=true/status=stale_response_rejected transaction=8 reason=scene_advanced rearmed=false/' \
    "$evidence" >"$unarmed"
reject_mutation "a stale response that did not re-arm" "$unarmed" \
    "rejected without re-arming"

# Schema-8 evidence without the damage fields, or with the feature never
# firing, is not the promotion run the gate exists to produce.
undamaged="$temp_dir/undamaged.log"
sed 's/ frame_slot_partial_repaints=118 frame_slot_full_repaints=145 frame_slot_history_invalidations=2 frame_slot_history_records=261//' \
    "$evidence" >"$undamaged"
reject_mutation "schema-8 evidence without damage fields" "$undamaged" \
    "resource record is missing frame_slot_partial_repaints"

unfired="$temp_dir/unfired.log"
sed 's/frame_slot_partial_repaints=118/frame_slot_partial_repaints=0/' "$evidence" >"$unfired"
reject_mutation "a promotion run in which no frame rendered partially" "$unfired" \
    "the buffer-age boundary was not exercised"

# The one-in-flight bound. Two presented heads may hold two submissions; three
# would mean an output outran its own heads.
overdepth="$temp_dir/overdepth.log"
sed 's/max_in_flight_per_output=2/max_in_flight_per_output=3/' "$evidence" >"$overdepth"
reject_mutation "an output holding more submissions than it has heads" "$overdepth" \
    "concurrent KMS submissions across 2 presented heads"

idle="$temp_dir/idle.log"
sed 's/max_in_flight_per_output=2/max_in_flight_per_output=0/' "$evidence" >"$idle"
reject_mutation "a session that never had a submission in flight" "$idle" \
    "no KMS submission was ever in flight"

undepth="$temp_dir/undepth.log"
sed 's/ max_in_flight_per_output=2 pending_frame_supersessions=41//' "$evidence" >"$undepth"
reject_mutation "schema-9 evidence without the in-flight depth" "$undepth" \
    "resource record is missing max_in_flight_per_output"

# Archive 0001 predates the feature; its schema-7 record must stay acceptable.
legacy="$temp_dir/legacy.log"
sed -e 's/sophia_live_native_resources schema=9 status=complete/sophia_live_native_resources schema=7 status=complete/' \
    -e 's/ max_in_flight_per_output=2 pending_frame_supersessions=41//' \
    -e 's/ frame_slot_partial_repaints=118 frame_slot_full_repaints=145 frame_slot_history_invalidations=2 frame_slot_history_records=261//' \
    "$evidence" >"$legacy"
"$verifier" "$legacy" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier rejected schema-7 evidence, which orphans archive 0001" >&2
    exit 1
}

# Archive 0002 is schema-8 evidence and must stay verifiable by the same rule.
previous="$temp_dir/previous.log"
sed -e 's/sophia_live_native_resources schema=9 status=complete/sophia_live_native_resources schema=8 status=complete/' \
    -e 's/ max_in_flight_per_output=2 pending_frame_supersessions=41//' \
    "$evidence" >"$previous"
"$verifier" "$previous" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier rejected schema-8 evidence, which orphans archive 0002" >&2
    exit 1
}

# Schema 10 adds the renderer-thread count, the misroute counter, and the
# service-skew figure. Real schema-10 evidence needs a physical run with
# outputs sharing a worker; until that archive exists, this derives the shape
# from the same real record the other schema controls mutate.
forward="$temp_dir/forward.log"
sed -e 's/sophia_live_native_resources schema=9 status=complete/sophia_live_native_resources schema=10 status=complete/' \
    -e 's/ max_worker_request_msec=61/ max_worker_request_msec=61 renderer_workers=1 worker_result_misroutes=0 worker_max_service_skew=1/' \
    "$evidence" >"$forward"
"$verifier" "$forward" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier rejected schema-10 evidence" >&2
    exit 1
}

# A shared worker that misrouted a result is not the run being promoted: the
# reply channels make that unreachable, so any count above zero means the
# structure was subverted.
misrouted="$temp_dir/misrouted.log"
sed 's/ worker_result_misroutes=0/ worker_result_misroutes=1/' "$forward" >"$misrouted"
reject_mutation "a run whose renderer misrouted a result" "$misrouted" \
    "a renderer result reached an output that did not request it"

# Skew is bounded only where outputs share a thread, and two heads on one
# thread may be passed over at most once each.
starved="$temp_dir/starved.log"
sed 's/ worker_max_service_skew=1/ worker_max_service_skew=2/' "$forward" >"$starved"
reject_mutation "a run that starved an output behind its sibling" "$starved" \
    "was passed over"

# The same skew on independent threads is not a fault: threads interleave as
# the GPU allows, and bounding that would assert something about parallelism.
unshared="$temp_dir/unshared.log"
sed -e 's/ renderer_workers=1/ renderer_workers=2/' \
    -e 's/ worker_max_service_skew=1/ worker_max_service_skew=2/' "$forward" >"$unshared"
"$verifier" "$unshared" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier applied a sharing bound to independent threads" >&2
    exit 1
}

# A schema-10 record owes its new fields.
truncated="$temp_dir/truncated.log"
sed 's/ renderer_workers=1//' "$forward" >"$truncated"
reject_mutation "schema-10 evidence without the renderer-thread count" "$truncated" \
    "resource record is missing renderer_workers"

# Schema 11 reports the direct scanout path. Real schema-11 evidence needs a
# physical run with the path enabled; until that archive exists, this derives
# the shape from the same real record the other schema controls mutate.
direct="$temp_dir/direct.log"
sed -e 's/sophia_live_native_resources schema=10 status=complete/sophia_live_native_resources schema=12 status=complete/' \
    -e 's/ worker_max_service_skew=1/ worker_max_service_skew=1 direct_scanout_attempts=44 direct_scanout_flips=43 direct_scanout_tests=2 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_unsupported=0 direct_scanout_fallbacks=1/' \
    "$forward" >"$direct"
"$verifier" "$direct" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier rejected schema-12 evidence" >&2
    exit 1
}

# A session that ran with the path off reports zeros and is still a valid
# session -- most are. The verifier must not require the row to have fired.
quiet="$temp_dir/quiet.log"
sed 's/ direct_scanout_attempts=44 direct_scanout_flips=43 direct_scanout_tests=2 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_fallbacks=1/ direct_scanout_attempts=0 direct_scanout_flips=0 direct_scanout_tests=0 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_fallbacks=0/' \
    "$direct" >"$quiet"
"$verifier" "$quiet" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier required direct scanout of a session that never used it" >&2
    exit 1
}

# A refusal means Engine proved a frame its own lowered pixels contradict.
# There is no benign nonzero value: an ineligible frame never becomes an
# attempt, so this can only be the two halves of the proof disagreeing.
disagreed="$temp_dir/disagreed.log"
sed 's/ direct_scanout_refusals=0/ direct_scanout_refusals=1/' "$direct" >"$disagreed"
reject_mutation "a run whose eligibility proof disagreed with its pixels" "$disagreed" \
    "disagreed with the frame it lowered"

# A client buffer may not reach a plane without the driver having been asked.
untested="$temp_dir/untested.log"
sed 's/ direct_scanout_tests=2/ direct_scanout_tests=0/' "$direct" >"$untested"
reject_mutation "a direct flip with no validating commit" "$untested" \
    "no validating commit"

# Every attempt ends as a flip, a fallback, or is still outstanding. More
# settlements than attempts means the counters describe different frames.
overcounted="$temp_dir/overcounted.log"
sed 's/ direct_scanout_attempts=44/ direct_scanout_attempts=43/' "$direct" >"$overcounted"
reject_mutation "more settled direct attempts than attempts" "$overcounted" \
    "settled more attempts than it made"

# A schema-11 record owes its new fields.
partial="$temp_dir/partial.log"
sed 's/ direct_scanout_fallbacks=1//' "$direct" >"$partial"
reject_mutation "schema-12 evidence without the fallback count" "$partial" \
    "resource record is missing direct_scanout_fallbacks"

# The sampler ran for as long as the session did.
#
# This gate drives a ramp, not a steady state, so a growth comparison belongs to
# the soak verifier rather than here. What this owes is the difference between a
# short session and a lost sampler, which is the cadence against the clock.
#
# The evidence fixture is a real session of 41,802ms, so its five-second
# cadence owes eight samples.
{
    for seq in $(seq 1 8); do
        printf 'sophia_live_resource_sample schema=1 seq=%d uptime_msec=%d rss_kib=201344 cpu_registry_buffers=0 cpu_registry_bytes=0 cpu_cow_splits=0 frame_slots_leased=4 snapshot_live_entries=2 import_cache_live_entries=2\n' \
            "$seq" "$(( seq * 5000 ))"
    done
    echo 'sophia_live_resource_steady_state schema=1 status=complete samples=8 saturated=false interval_msec=5000'
} >>"$evidence"
"$verifier" "$evidence" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier rejected a session whose sampler kept its cadence" >&2
    exit 1
}

# A sampler that stopped partway leaves a session claiming a cadence it did not
# keep, which is what separates a short run from a lost one.
stalled="$temp_dir/stalled-sampler.log"
grep -v '^sophia_live_resource_sample schema=1 seq=[5-8] ' "$evidence" \
    | sed 's/samples=8 /samples=4 /' >"$stalled"
reject_mutation "a session whose sampler stopped partway" "$stalled" \
    "where its 5000ms cadence owes about"

# A count that disagrees with the samples actually present is a truncated
# record read as a complete one.
miscounted="$temp_dir/miscounted-samples.log"
sed 's/samples=8 /samples=12 /' "$evidence" >"$miscounted"
reject_mutation "a session claiming more samples than it recorded" "$miscounted" \
    "and recorded"

# Evidence with no samples at all stays verifiable: archives 0001 through 0003
# predate the sampler, and this file cannot tell those from a run that lost it.
unsampled="$temp_dir/unsampled.log"
grep -v '^sophia_live_resource_sample \|^sophia_live_resource_steady_state ' "$evidence" >"$unsampled"
"$verifier" "$unsampled" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier required resource samples, which orphans archives 0001 to 0003" >&2
    exit 1
}

# The shared-worker and one-in-flight rules were guarded by schema equality
# rather than by a lower bound, so they stopped running the moment the record
# moved past the schema they named -- which is how they went unasserted for
# every schema-11 and schema-12 session. These three controls mutate the
# newest fixture rather than the schema-10 one, so a guard that silently
# stops applying to current evidence fails here instead of in production.
current_misrouted="$temp_dir/current-misrouted.log"
sed 's/ worker_result_misroutes=0/ worker_result_misroutes=1/' "$direct" >"$current_misrouted"
reject_mutation "current-schema evidence whose renderer misrouted a result" \
    "$current_misrouted" "a renderer result reached an output that did not request it"

current_starved="$temp_dir/current-starved.log"
sed 's/ worker_max_service_skew=1/ worker_max_service_skew=2/' "$direct" >"$current_starved"
reject_mutation "current-schema evidence that starved an output behind its sibling" \
    "$current_starved" "was passed over"

# The cursor path a session took, against the one it asked for. Asking for the
# plane and taking the ioctl is a card refusing the probe, which is the
# fallback this row kept on purpose and must stay acceptable. The reverse is
# the preference being ignored.
refused_card="$temp_dir/refused-card.log"
sed 's/requested=atomic_plane path=atomic_plane/requested=atomic_plane path=legacy_ioctl/' \
    "$evidence" >"$refused_card"
"$verifier" "$refused_card" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier refused a card that kept the legacy cursor, which is the fallback working" >&2
    exit 1
}

ignored_preference="$temp_dir/ignored-preference.log"
sed 's/requested=atomic_plane path=atomic_plane/requested=legacy_ioctl path=atomic_plane/' \
    "$evidence" >"$ignored_preference"
reject_mutation "a session that asked for the legacy cursor and took the plane" \
    "$ignored_preference" "asked for the legacy cursor and took"

# A record that names only where the cursor ended up cannot say whether the
# card refused or the preference was ignored, which is the whole reason the
# request is recorded beside the path.
halved="$temp_dir/halved-cursor.log"
sed 's/ requested=atomic_plane path=atomic_plane/ path=atomic_plane/' \
    "$evidence" >"$halved"
reject_mutation "a cursor record that names no request" "$halved" \
    "does not name both a request and a path"

# Evidence predating the record must stay verifiable: archives 0001 through
# 0003 carry no cursor line at all, and this file cannot tell those from a run
# that lost it. The gate owns that requirement instead.
no_cursor="$temp_dir/no-cursor.log"
grep -v '^sophia_live_cursor_path ' "$evidence" >"$no_cursor"
"$verifier" "$no_cursor" "$proof_text" >/dev/null 2>&1 || {
    echo "the native verifier required a cursor record, which orphans archives 0001 to 0003" >&2
    exit 1
}

current_overdepth="$temp_dir/current-overdepth.log"
sed 's/max_in_flight_per_output=2/max_in_flight_per_output=3/' "$direct" >"$current_overdepth"
reject_mutation "current-schema evidence holding more submissions than heads" \
    "$current_overdepth" "concurrent KMS submissions across 2 presented heads"

# A restarted or degraded WM is a different run than the one being promoted.
restarted="$temp_dir/restarted.log"
sed 's/wm_restarts=0/wm_restarts=1/' "$evidence" >"$restarted"
reject_mutation "a session whose WM restarted" "$restarted" \
    "completion does not contain wm_restarts=0"

# The identity must describe the profile that ran.
misreported="$temp_dir/misreported.log"
sed "s/root_sha256=$profile_sha256/root_sha256=eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee/" \
    "$evidence" >"$misreported"
reject_mutation "an identity naming a profile the session did not load" "$misreported" \
    "the loaded desktop profile is not the one bound to this run"

emergency="$temp_dir/emergency.log"
sed 's/emergency=false session_shutdown=not_requested/emergency=true session_shutdown=graceful/' \
    "$evidence" >"$emergency"
reject_mutation "an emergency exit as a normal logout" "$emergency" \
    "missing exact TTY recovery"

protocol="$temp_dir/protocol.log"
sed 's/^sophia_live_session_protocol_errors schema=1 expected=0 unexpected=0$/sophia_live_session_protocol_errors schema=1 expected=0 unexpected=3/' \
    "$evidence" >"$protocol"
reject_mutation "unexpected X protocol errors" "$protocol" \
    "missing zero unexpected protocol errors"

# An extra keypress the guide never asked for means the session that ran is not
# the session that was specified.
extra="$temp_dir/extra.log"
cp "$evidence" "$extra"
printf '%s\n' 'sophia_live_wm schema=1 status=physical_action_committed action=38' >>"$extra"
reject_mutation "an action the guide never requested" "$extra" \
    "the guide never asked for it"

# Archive and re-verify, then prove the archive verifier refuses a manifest that
# no longer matches its own evidence.
sophia_bin="$temp_dir/sophia"
hagia_bin="$temp_dir/hagia"
hagia_shell_bin="$temp_dir/hagia-shell"
cp /usr/bin/true "$sophia_bin"
cp /usr/bin/false "$hagia_bin"
cp /usr/bin/true "$hagia_shell_bin"
sophia_commit="$(git -C "$root_dir" rev-parse HEAD)"
# Synthetic binary identities need real signed commit objects, not product
# checkouts. Use this repository for each fixture's signature verification.
hagia_root="$root_dir"
hagia_commit="$sophia_commit"
narthex_root="$root_dir"
narthex_commit="$sophia_commit"
sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
hagia_sha256="$(sha256sum "$hagia_bin" | awk '{ print $1 }')"
narthex_sha256="$(sha256sum "$hagia_shell_bin" | awk '{ print $1 }')"
archive_evidence="$temp_dir/archive-evidence.log"
sed \
    -e "s/sophia_commit=1111111111111111111111111111111111111111/sophia_commit=$sophia_commit/" \
    -e "s/hagia_commit=2222222222222222222222222222222222222222/hagia_commit=$hagia_commit/" \
    -e "s/narthex_commit=3333333333333333333333333333333333333333/narthex_commit=$narthex_commit/" \
    -e "s/sophia_sha256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/sophia_sha256=$sophia_sha256/" \
    -e "s/hagia_sha256=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/hagia_sha256=$hagia_sha256/" \
    -e "s/narthex_sha256=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc/narthex_sha256=$narthex_sha256/" \
    "$evidence" >"$archive_evidence"
archive_output="$(env \
    XDG_STATE_HOME="$temp_dir/state" \
    SOPHIA_HAGIA_ROOT="$hagia_root" \
    SOPHIA_NARTHEX_ROOT="$narthex_root" \
    SOPHIA_HAGIA_NATIVE_SOPHIA_BIN="$sophia_bin" \
    SOPHIA_HAGIA_BIN="$hagia_bin" \
    SOPHIA_HAGIA_SHELL_BIN="$hagia_shell_bin" \
    "$root_dir/tools/archive_hagia_native_session_run.sh" \
    "$archive_evidence" "$proof_text")"
run_dir="${archive_output##*: }"
SOPHIA_HAGIA_ROOT="$hagia_root" \
    SOPHIA_NARTHEX_ROOT="$narthex_root" \
    "$root_dir/tools/verify_hagia_native_session_archive.sh" "$run_dir" >/dev/null


# Recomputed checksums must not hide a changed Narthex identity. The positive
# archive above binds genuine signed objects to synthetic client identities; these
# copies change only the claim being tested, not the signature verifier.
archive_fixture_checksums() {
    candidate="$1"
    digest="$(sha256sum "$candidate/session.log" | awk '{ print $1 }')"
    sed -i "s/^evidence_sha256=.*/evidence_sha256=$digest/" "$candidate/manifest"
    (cd "$candidate" && sha256sum manifest result.kdl session.log >SHA256SUMS)
}
refuse_narthex_archive() {
    candidate="$1"
    expected="$2"
    selected_root="$3"
    if rejection="$(env SOPHIA_HAGIA_ROOT="$hagia_root" \
        SOPHIA_NARTHEX_ROOT="$selected_root" \
        "$root_dir/tools/verify_hagia_native_session_archive.sh" "$candidate" 2>&1)"; then
        echo "the native archive verifier accepted $expected" >&2
        exit 1
    fi
    printf '%s\n' "$rejection" | grep -Fq "$expected" || {
        echo "the native archive verifier refused a Narthex mutation for the wrong reason: $rejection" >&2
        exit 1
    }
}
changed_narthex="$temp_dir/changed-narthex-manifest"
cp -R "$run_dir" "$changed_narthex"
sed -i 's/^narthex_commit=.*/narthex_commit=ffffffffffffffffffffffffffffffffffffffff/' \
    "$changed_narthex/manifest"
archive_fixture_checksums "$changed_narthex"
refuse_narthex_archive "$changed_narthex" 'different Narthex identities' "$narthex_root"

unknown_narthex="$temp_dir/unknown-narthex"
cp -R "$changed_narthex" "$unknown_narthex"
sed -i 's/narthex_commit=[0-9a-f]\{40\}/narthex_commit=ffffffffffffffffffffffffffffffffffffffff/' \
    "$unknown_narthex/session.log"
archive_fixture_checksums "$unknown_narthex"
refuse_narthex_archive "$unknown_narthex" 'invalid Narthex source commit' "$narthex_root"
refuse_narthex_archive "$run_dir" 'Narthex checkout is unavailable' "$temp_dir/missing-narthex"

# An existing but unsigned object must fail separately from an absent object.
# This scratch repository is a negative only: no signature is forged or stubbed.
unsigned_narthex_repo="$temp_dir/unsigned-narthex-repo"
git -c init.templateDir= init -q "$unsigned_narthex_repo"
git -C "$unsigned_narthex_repo" -c user.name=fixture -c user.email=fixture@example.invalid \
    -c commit.gpgsign=false commit -q --allow-empty -m 'unsigned Narthex rejection fixture'
unsigned_commit="$(git -C "$unsigned_narthex_repo" rev-parse HEAD)"
unsigned_narthex="$temp_dir/unsigned-narthex-archive"
cp -R "$run_dir" "$unsigned_narthex"
sed -i "s/^narthex_commit=.*/narthex_commit=$unsigned_commit/" "$unsigned_narthex/manifest"
sed -i "s/narthex_commit=[0-9a-f]\{40\}/narthex_commit=$unsigned_commit/" "$unsigned_narthex/session.log"
archive_fixture_checksums "$unsigned_narthex"
refuse_narthex_archive "$unsigned_narthex" 'Narthex source commit lacks a valid signature' "$unsigned_narthex_repo"

# Historical schema 1 has no Narthex source binding. A nonexistent Narthex root
# must not prevent it from verifying against its original Sophia/Hagia inputs.
legacy="$temp_dir/schema1-archive"
cp -R "$run_dir" "$legacy"
sed -i -e 's/^record_schema=2$/record_schema=1/' -e '/^narthex_commit=/d' \
    -e 's/^narthex_binary_sha256=/hagia_shell_binary_sha256=/' "$legacy/manifest"
sed -i 's/schema=2/schema=1/' "$legacy/result.kdl"
sed -i -e 's/^sophia_hagia_native_identity schema=2 /sophia_hagia_native_identity schema=1 /' \
    -e 's/ narthex_commit=[0-9a-f]\{40\}//' \
    -e 's/ narthex_sha256=/ hagia_shell_sha256=/' "$legacy/session.log"
archive_fixture_checksums "$legacy"
env SOPHIA_HAGIA_ROOT="$hagia_root" SOPHIA_NARTHEX_ROOT="$temp_dir/missing-narthex" \
    "$root_dir/tools/verify_hagia_native_session_archive.sh" "$legacy" >/dev/null

# The same evidence must not become two proofs.
if env \
    XDG_STATE_HOME="$temp_dir/state" \
    SOPHIA_HAGIA_ROOT="$hagia_root" \
    SOPHIA_NARTHEX_ROOT="$narthex_root" \
    SOPHIA_HAGIA_NATIVE_SOPHIA_BIN="$sophia_bin" \
    SOPHIA_HAGIA_BIN="$hagia_bin" \
    SOPHIA_HAGIA_SHELL_BIN="$hagia_shell_bin" \
    "$root_dir/tools/archive_hagia_native_session_run.sh" \
    "$archive_evidence" "$proof_text" >/dev/null 2>&1; then
    echo "the native archiver recorded the same evidence twice" >&2
    exit 1
fi

sed -i \
    's/^hagia_commit=.*/hagia_commit=ffffffffffffffffffffffffffffffffffffffff/' \
    "$run_dir/manifest"
(
    cd "$run_dir"
    sha256sum manifest result.kdl session.log >SHA256SUMS
)
if SOPHIA_HAGIA_ROOT="$hagia_root" \
    SOPHIA_NARTHEX_ROOT="$narthex_root" \
    "$root_dir/tools/verify_hagia_native_session_archive.sh" \
    "$run_dir" >/dev/null 2>&1; then
    echo "the native archive verifier accepted an unknown Hagia commit" >&2
    exit 1
fi

printf '%s\n' 'Hagia native matchers accepted the bounded three-launch workflow.'

# The output-unplug scenario's host waits and its guest endpoint, sourced by
# tools/qemu_session_harness.sh. Kept apart so the waits and the endpoint can
# be driven with stand-in processes (crates/xtask/tests/output_unplug_endpoint.rs).
#
# Every wait has one bound and ends in exactly one outcome, and the endpoint
# names what actually happened to the guest's QEMU and to the serial logger:
# a clean exit of both, a non-zero status, or a process the host had to stop.
# Only the first is "guest_exited".

# unplug_wait_for EVIDENCE STEPS PID PROBE...: polls every 0.05 s, at most
# STEPS times, and prints one outcome. A failure record the guest or the host
# already wrote wins over everything; then the probe (PROBE... run as a
# command, success meaning ready); then the process: a marker that arrived
# with the process's exit still counts as ready.
#   guest_failed  a "sophia_qemu_guest" or "sophia_qemu_unplug" failed record
#   ready         the probe succeeded
#   guest_exited  PID is gone
#   timeout       STEPS polls passed
unplug_wait_for() {
    local evidence=$1 steps=$2 pid=$3 step
    shift 3
    for ((step = 0; step < steps; step++)); do
        if grep -qE '^sophia_qemu_(guest|unplug) schema=1 status=failed ' "$evidence"; then
            echo guest_failed
            return
        fi
        if "$@"; then
            echo ready
            return
        fi
        if ! kill -0 "$pid" 2>/dev/null; then
            echo guest_exited
            return
        fi
        sleep 0.05
    done
    echo timeout
}

# unplug_end_process PID DEADLINE: waits until PID is gone or SECONDS reaches
# DEADLINE; a process still alive then gets TERM, then KILL after
# UNPLUG_STOP_GRACE_S more seconds, then UNPLUG_KILL_GRACE_S (2) more seconds to
# be gone. Sets UNPLUG_END_STATUS and UNPLUG_END_SIGNAL (none, TERM or KILL).
# The status is the wait status of a process that is gone, or "unreaped" for
# one still present after the KILL grace: that process is never waited for, so
# nothing here blocks past its bounds, and its pid stays for the runner's own
# leftover check. It runs in the caller's shell, never a subshell, because only
# the shell that started PID can wait for it.
unplug_end_process() {
    local pid=$1 deadline=$2 signal=none stop_deadline
    # No process to end is reported as such (255), never as a clean exit.
    if [[ -z "$pid" ]]; then
        UNPLUG_END_STATUS=255
        UNPLUG_END_SIGNAL=none
        return
    fi
    while kill -0 "$pid" 2>/dev/null && ((SECONDS < deadline)); do
        sleep 0.1
    done
    if kill -0 "$pid" 2>/dev/null; then
        signal=TERM
        kill -TERM "$pid" 2>/dev/null || true
        stop_deadline=$((SECONDS + ${UNPLUG_STOP_GRACE_S:-5}))
        while kill -0 "$pid" 2>/dev/null && ((SECONDS < stop_deadline)); do
            sleep 0.1
        done
        if kill -0 "$pid" 2>/dev/null; then
            signal=KILL
            kill -KILL "$pid" 2>/dev/null || true
            stop_deadline=$((SECONDS + ${UNPLUG_KILL_GRACE_S:-2}))
            while kill -0 "$pid" 2>/dev/null && ((SECONDS < stop_deadline)); do
                sleep 0.1
            done
            if kill -0 "$pid" 2>/dev/null; then
                UNPLUG_END_STATUS=unreaped
                UNPLUG_END_SIGNAL=KILL
                return
            fi
        fi
    fi
    # The process is gone, so this wait only collects its status; under the
    # harness's errexit a non-zero status must be recorded, not fatal.
    UNPLUG_END_STATUS=0
    wait "$pid" 2>/dev/null || UNPLUG_END_STATUS=$?
    UNPLUG_END_SIGNAL=$signal
}

# unplug_collect_endpoint EVIDENCE QEMU_PID LOGGER_PID DEADLINE [STOP_REASON]:
# the guest's end, recorded once. LOGGER_PID is the last member (tee) of the
# harness's serial pipeline; its reader ends at the FIFO's EOF in a clean end,
# but a stopped or unreaped logger says nothing about the rest of that pipeline,
# which the runner's leftover check still has to cover. QEMU may run until DEADLINE (an absolute SECONDS value,
# never extended here); the logger, which ends when QEMU closes the serial
# FIFO, gets UNPLUG_LOGGER_GRACE_S seconds after that. Records:
#   status=guest_exited qemu_exit=0                         both exited 0
#   status=failed reason=guest_exit qemu_exit=N logger_exit=M   both ended
#                                                           by themselves, not 0/0
#   status=failed reason=STOP_REASON (host_timeout by default), then
#   status=guest_stopped qemu_exit=N logger_exit=M qemu_signal=S logger_signal=T
#                                                           QEMU outlived DEADLINE;
#                                                           an exit of "unreaped"
#                                                           adds qemu_pid=/logger_pid=
#   status=guest_stopped ... (qemu_signal=none)             only the logger had
#                                                           to be stopped
# Returns 0 only when guest_exited was written.
unplug_collect_endpoint() {
    local evidence=$1 qemu_pid=$2 logger_pid=$3 deadline=$4 stop_reason=${5:-host_timeout}
    local qemu_status qemu_signal logger_status logger_signal unreaped=""
    unplug_end_process "$qemu_pid" "$deadline"
    qemu_status=$UNPLUG_END_STATUS
    qemu_signal=$UNPLUG_END_SIGNAL
    if [[ "$qemu_signal" != none ]]; then
        echo "sophia_qemu_unplug schema=1 status=failed reason=$stop_reason" | tee -a "$evidence"
    fi
    unplug_end_process "$logger_pid" "$((SECONDS + ${UNPLUG_LOGGER_GRACE_S:-5}))"
    logger_status=$UNPLUG_END_STATUS
    logger_signal=$UNPLUG_END_SIGNAL
    [[ "$qemu_status" != unreaped ]] || unreaped+=" qemu_pid=$qemu_pid"
    [[ "$logger_status" != unreaped ]] || unreaped+=" logger_pid=$logger_pid"
    if [[ "$qemu_signal" != none || "$logger_signal" != none ]]; then
        echo "sophia_qemu_unplug schema=1 status=guest_stopped qemu_exit=$qemu_status logger_exit=$logger_status qemu_signal=$qemu_signal logger_signal=$logger_signal$unreaped" | tee -a "$evidence"
        return 1
    fi
    if [[ "$qemu_status" -ne 0 || "$logger_status" -ne 0 ]]; then
        echo "sophia_qemu_unplug schema=1 status=failed reason=guest_exit qemu_exit=$qemu_status logger_exit=$logger_status" | tee -a "$evidence"
        return 1
    fi
    echo "sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0" | tee -a "$evidence"
}

# The harness's own ends. They read the harness globals EVIDENCE_FILE,
# QEMU_PID, LOGGER_PID, DISPLAY_BUS_PID, ROOT_DIR, UNPLUG_MODE and the socket
# paths.
# UNPLUG_HOST_BOUND_S (150) is the normal path's bound on a session that never
# ends.

# unplug_failed REASON: the failure is recorded first and the host sends nothing
# more; the guest's end is then recorded once, QEMU allowed the same bound
# counted from the failure. The collector never re-enters this function, so no
# deadline is restarted. Exits 1, and no verifier runs.
unplug_failed() {
    echo "sophia_qemu_unplug schema=1 status=failed reason=$1" | tee -a "$EVIDENCE_FILE"
    if [[ -n "${QEMU_PID:-}" ]]; then
        unplug_collect_endpoint "$EVIDENCE_FILE" "$QEMU_PID" "${LOGGER_PID:-}" \
            "$((SECONDS + ${UNPLUG_HOST_BOUND_S:-150}))" || true
        QEMU_PID=""
        LOGGER_PID=""
    fi
    exit 1
}

# unplug_finish: the normal path's end, after every action was sent. Only a
# guest_exited endpoint and a cleanup that left nothing unreaped go on to the
# verifier.
unplug_finish() {
    local endpoint_status=0
    unplug_collect_endpoint "$EVIDENCE_FILE" "$QEMU_PID" "$LOGGER_PID" \
        "$((SECONDS + ${UNPLUG_HOST_BOUND_S:-150}))" || endpoint_status=$?
    QEMU_PID=""
    LOGGER_PID=""
    unplug_cleanup || endpoint_status=1
    if ((endpoint_status != 0)); then
        exit 1
    fi
    cd "$ROOT_DIR"
    exec cargo xtask conformance verify output-unplug "$UNPLUG_MODE" "$EVIDENCE_FILE"
}

# unplug_cleanup: the output-unplug scenario's EXIT trap, replacing the shared
# cleanup() there only. A guest still running here (an unexpected exit) is ended
# and recorded as stopped by the harness, never as guest_exited. The display bus
# is ended with the same bounds and recorded under its own name: TERM or KILL of
# this private bus is ordinary cleanup, but an unreaped bus keeps its pid in the
# record for the runner and makes this return 1. Each pid is cleared once
# recorded, so a second call (the EXIT trap after unplug_finish) restarts no
# deadline. Then the harness's four socket paths are removed, as cleanup() does.
unplug_cleanup() {
    local unreaped="" status=0
    if [[ -n "${QEMU_PID:-}" ]]; then
        unplug_collect_endpoint "$EVIDENCE_FILE" "$QEMU_PID" "${LOGGER_PID:-}" "$SECONDS" harness_exit || true
        QEMU_PID=""
        LOGGER_PID=""
    fi
    if [[ -n "${DISPLAY_BUS_PID:-}" ]]; then
        unplug_end_process "$DISPLAY_BUS_PID" "$SECONDS"
        if [[ "$UNPLUG_END_STATUS" == unreaped ]]; then
            unreaped=" display_bus_pid=$DISPLAY_BUS_PID"
            status=1
        fi
        echo "sophia_qemu_unplug schema=1 status=display_bus_ended display_bus_exit=$UNPLUG_END_STATUS display_bus_signal=$UNPLUG_END_SIGNAL$unreaped" | tee -a "$EVIDENCE_FILE"
        DISPLAY_BUS_PID=""
    fi
    rm -f -- "$VNC_SOCKET" "$QMP_SOCKET" "$SERIAL_FIFO" "$DISPLAY_BUS_SOCKET"
    return "$status"
}

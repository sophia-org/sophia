# Sourced by tools/qemu_session_harness.sh. A host barrier on the guest's
# evidence: whether a line matching an extended regex was written after a
# given line count. The count is taken before the host acts, so a record
# from before the action never satisfies the barrier, while one the guest
# writes before the host's own "sent" marker still does.

# evidence_after_line FILE LINES PATTERN
evidence_after_line() {
    awk -v n="$2" -v p="$3" 'NR > n && $0 ~ p { found = 1; exit } END { exit !found }' "$1"
}

# wait_for_after_line ANCHOR PATTERN REASON: polls $EVIDENCE_FILE while
# $QEMU_PID lives (at most 30 s), else records REASON and exits 1.
wait_for_after_line() {
    local anchor="$1" pattern="$2" reason="$3"
    for _ in $(seq 1 600); do
        if evidence_after_line "$EVIDENCE_FILE" "$anchor" "$pattern"; then
            return 0
        fi
        kill -0 "$QEMU_PID" 2>/dev/null || break
        sleep 0.05
    done
    echo "sophia_qemu_gtk schema=1 status=failed reason=$reason scenario=$SCENARIO" | tee -a "$EVIDENCE_FILE"
    exit 1
}

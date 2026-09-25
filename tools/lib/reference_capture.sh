# Validation shared by the t018 reference-capture gate, its archive and archive
# re-verification, so the three cannot drift. A capture claims only the bound
# identity and profile, an observed exit 0, validated TTY recovery and the
# retained evidence. It is not a native workflow acceptance and verifies no tab
# observation.
#
# Each required family is counted in full and must hold exactly one record,
# which must then have the exact form: a valid record beside a malformed
# duplicate of the same family still refuses. The forms are the native
# verifier's own (tools/verify_hagia_native_session.sh).

REFERENCE_CAPTURE_IDENTITY='^sophia_hagia_native_identity schema=2 status=bound sophia_commit=[0-9a-f]{40} hagia_commit=[0-9a-f]{40} narthex_commit=[0-9a-f]{40} sophia_sha256=[0-9a-f]{64} hagia_sha256=[0-9a-f]{64} narthex_sha256=[0-9a-f]{64} desktop_profile_sha256=[0-9a-f]{64}$'
REFERENCE_CAPTURE_RECOVERY='^sophia_tty_recovery schema=3 profile=hagia kd_mode_before=[^ ]+ kd_mode_after=[^ ]+ termios_restored=true emergency=false session_shutdown=not_requested session_exit_status=none$'
REFERENCE_CAPTURE_RECORD='^sophia_hagia_reference_capture schema=1 status=captured native_acceptance=false tab_observations=unverified sophia_commit=[0-9a-f]{40} hagia_commit=[0-9a-f]{40} narthex_commit=[0-9a-f]{40} desktop_profile_sha256=[0-9a-f]{64}$'

reference_capture_sole() {
    local evidence="$1" family="$2" exact="$3" label="$4" observed
    observed="$(grep -Ec -- "$family" "$evidence" || true)"
    if [[ "$observed" != 1 ]]; then
        echo "reference capture requires exactly one $label record, found $observed" >&2
        return 1
    fi
    grep -Eq -- "$exact" "$evidence" || {
        echo "reference capture $label record is malformed" >&2
        return 1
    }
}

# The records a capture rests on, before its capture record is written.
reference_capture_validate_session() {
    local evidence="$1" proof_text="$2"
    [[ -s "$evidence" && "$proof_text" =~ ^[a-z]{1,24}$ ]] || {
        echo "reference capture evidence or proof text is invalid" >&2
        return 1
    }
    reference_capture_sole "$evidence" '^sophia_hagia_native_identity ' \
        "$REFERENCE_CAPTURE_IDENTITY" identity \
        && reference_capture_sole "$evidence" '^sophia_tty_recovery ' \
            "$REFERENCE_CAPTURE_RECOVERY" "TTY recovery" \
        && reference_capture_sole "$evidence" \
            '^sophia_live_session_input (.* )?status=complete( |$)' \
            "^sophia_live_session_input schema=2 status=complete source=physical text=$proof_text expected_events=[1-9][0-9]* matched_events=[1-9][0-9]* pixel_change=true\$" \
            "proof-input completion"
}

# The capture record a validated session earns, derived from its identity.
reference_capture_record() {
    local identity field value record
    identity="$(grep -E "$REFERENCE_CAPTURE_IDENTITY" "$1")" || return 1
    record='sophia_hagia_reference_capture schema=1 status=captured native_acceptance=false tab_observations=unverified'
    for field in sophia_commit hagia_commit narthex_commit desktop_profile_sha256; do
        value="$(sed -n "s/.* $field=\\([0-9a-f]*\\)\\( .*\\)\\{0,1\\}\$/\\1/p" <<<"$identity")"
        record+=" $field=$value"
    done
    printf '%s\n' "$record"
}

# A complete capture: the session records plus exactly the capture record they
# earn.
reference_capture_validate() {
    local evidence="$1" proof_text="$2"
    reference_capture_validate_session "$evidence" "$proof_text" \
        && reference_capture_sole "$evidence" '^sophia_hagia_reference_capture ' \
            "$REFERENCE_CAPTURE_RECORD" capture || return 1
    [[ "$(grep -E '^sophia_hagia_reference_capture ' "$evidence")" \
        == "$(reference_capture_record "$evidence")" ]] || {
        echo "reference capture record does not match the bound identity" >&2
        return 1
    }
}

# Which files a runner log and its one retained generation are right now.
reference_capture_log_state() {
    local log="$1"
    printf '%s %s\n' \
        "$(stat -c %d:%i -- "$log" 2>/dev/null || echo absent)" \
        "$(stat -c %d:%i -- "$log.previous" 2>/dev/null || echo absent)"
}

# The runner rotates each log once per run (sophia_session_rotate_log): the
# current file moves to .previous and a fresh one is created. The log belongs
# to this invocation only if it was rotated exactly once since BEFORE: a fresh
# current file, and a .previous that is the file BEFORE named current (or, when
# there was none, the unchanged older .previous). A second run in between
# leaves this invocation's own file in .previous instead, and refuses.
reference_capture_rotated_once() {
    local log="$1" before="$2" was_log was_previous now_log now_previous
    read -r was_log was_previous <<<"$before"
    read -r now_log now_previous <<<"$(reference_capture_log_state "$log")"
    [[ "$now_log" != absent && "$now_log" != "$was_log" ]] || return 1
    if [[ "$was_log" == absent ]]; then
        [[ "$now_previous" == "$was_previous" ]]
    else
        [[ "$now_previous" == "$was_log" ]]
    fi
}

# A reference run root may never be, contain, or sit inside promotion storage.
reference_capture_root_allowed() {
    local root promotion native
    root="$(realpath -m -- "$1")"
    promotion="$(realpath -m -- "$2")"
    native="$(realpath -m -- "$3")"
    [[ "$root" != "$promotion" && "$root" != "$promotion"/* && "$promotion" != "$root"/* \
        && "$root" != "$native" && "$root" != "$native"/* && "$native" != "$root"/* ]]
}

# Operator observations are bounded: at most this many bytes are ever retained.
REFERENCE_CAPTURE_OBSERVATIONS_LIMIT=262144

# Installed Sophia owns profile parsing, private staging and bounded validation.
sophia_check_hagia_profile() {
    local sophia="$1" hagia="$2" profile="$3"
    [[ -x "$sophia" && "$profile" == /* && -f "$profile" ]] || {
        echo "Desktop preflight requires executable Sophia and an absolute desktop profile." >&2
        return 1
    }
    local output status
    if output="$(timeout 15s "$sophia" config check-session-profile \
        "--desktop-profile=$profile" "--default-wm=$hagia")"; then
        printf '%s\n' "$output"
        if ! grep -Eq '^sophia_session_profile_preflight schema=1 status=accepted policy=(validated|deferred)$' <<<"$output"; then
            echo "Sophia did not confirm session profile preflight; rebuild the matching binary." >&2
            return 1
        fi
    else
        status=$?
        printf '%s\n' "$output"
        return "$status"
    fi
}

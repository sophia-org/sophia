# Installed Sophia owns profile parsing, private staging and bounded validation.
sophia_check_session_profile() {
    local sophia="$1" profile="$2" checker="$3"
    [[ -x "$sophia" && "$profile" == /* && -f "$profile" ]] || {
        echo "Desktop preflight requires executable Sophia and an absolute desktop profile." >&2
        return 1
    }
    local output status verdict count
    local -a policy
    if [[ "$checker" == --deferred ]]; then
        policy=(--allow-deferred-policy)
        verdict='sophia_session_profile_preflight schema=1 status=accepted policy=deferred'
    elif [[ "$checker" == /* && -x "$checker" && -f "$checker" ]]; then
        policy=("--policy-checker=$checker")
        verdict='sophia_session_profile_preflight schema=1 status=accepted policy=validated'
    else
        echo 'Select an absolute policy checker or explicitly use --deferred.' >&2
        return 1
    fi
    if output="$(timeout --kill-after=2s 15s "$sophia" config check-session-profile \
        "--desktop-profile=$profile" "${policy[@]}")"; then
        printf '%s\n' "$output"
        count="$(grep -c '^sophia_session_profile_preflight ' <<<"$output" || true)"
        if [[ "$count" != 1 ]] || ! grep -Fxq "$verdict" <<<"$output"; then
            echo "Sophia did not confirm session profile preflight; rebuild the matching binary." >&2
            return 1
        fi
    else
        status=$?
        printf '%s\n' "$output"
        return "$status"
    fi
}

# Load a versioned vector from the installed binary without evaluating its text.
# A private temporary file preserves both NUL boundaries and the producer's
# exit status; process substitution alone would lose the latter.
sophia_load_preparation() {
    local expected="$1" command="$2" prepared_file
    shift 2
    prepared_file="$(mktemp "${TMPDIR:-/tmp}/sophia-preparation.XXXXXX")" || return 1
    if ! timeout --kill-after=2s 15s "$SOPHIA_BIN" session "$command" "$@" >"$prepared_file"; then
        rm -f -- "$prepared_file"
        echo "The installed binary refused $command." >&2
        return 1
    fi
    mapfile -d '' -t prepared_vector <"$prepared_file"
    rm -f -- "$prepared_file"
    if [[ "${prepared_vector[0]:-}" != "$expected" ]]; then
        echo "This binary does not support $command; rebuild it." >&2
        return 1
    fi
}

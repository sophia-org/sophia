#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == session && "${2:-}" == input-guard ]]; then
    shift 2
    set -- sophia-session-input-guard "$@"
fi
case "${1:-}" in
    session)
        if [[ "${2:-}" != run ]]; then
            exec "${SOPHIA_TEST_PREPARER_BIN:?real preparation binary required}" "$@"
        fi
        trap '' TERM
        while true; do sleep 60; done
        ;;
    sophia-session-input-guard)
        armed_file=""
        triggered_file=""
        for argument in "$@"; do
            case "$argument" in
                --armed-file=*)
                    armed_file="${argument#--armed-file=}"
                    ;;
                --triggered-file=*) triggered_file="${argument#--triggered-file=}" ;;
            esac
        done
        [[ -n "$armed_file" ]] || exit 2
        printf 'armed\n' >"$armed_file"
        case "${SOPHIA_TEST_GUARD_MODE:-ready}" in
            die) exit 1 ;;
            trigger) printf 'triggered\n' >"$triggered_file" ;;
        esac
        trap 'exit 0' INT TERM
        while true; do
            sleep 1
        done
        ;;
    sophia-live-session)
        trap '' TERM
        while true; do
            sleep 60
        done
        ;;
    *)
        exit 2
        ;;
esac

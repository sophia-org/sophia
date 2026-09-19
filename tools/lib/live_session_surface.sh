#!/usr/bin/env bash
# Shared installed command/session reconciliation for activation and rollback.

sophia_surface_select_entries() {
    local release="$1" hagia_included

    hagia_included="$(awk -F= '$1 == "hagia_included" { print $2; exit }' \
        "$release/manifest")"
    case "$hagia_included" in
        true|false) ;;
        *)
            echo "Release has an invalid hagia_included value." >&2
            return 1
            ;;
    esac

    SOPHIA_SURFACE_HAGIA_INCLUDED="$hagia_included"
    SOPHIA_SURFACE_HAGIA_COMMANDS=(
        sophia-hagia-session
        sophia-hagia-promotion-session
        sophia-firefox-proof
        sophia-xterm-proof
        sophia-truecolor-proof
        sophia-recovery-proof
        sophia-record-hagia-run
        sophia-record-watchdog-run
        sophia-record-firefox-run
        sophia-verify-hagia
        sophia-verify-hagia-promotion
        sophia-verify-watchdog
        sophia-verify-firefox-runs
        sophia-verify-xterm-runs
        sophia-verify-truecolor-runs
    )
    SOPHIA_SURFACE_HAGIA_DESKTOPS=(
        sophia-hagia
        sophia-hagia-promotion
        sophia-firefox-proof
        sophia-recovery-proof
    )
    SOPHIA_SURFACE_HAGIA_DESKTOP_COMMANDS=(
        sophia-hagia-session
        sophia-hagia-promotion-session
        sophia-firefox-proof
        sophia-recovery-proof
    )
    SOPHIA_SURFACE_COMMANDS=(
        sophia-session
        sophia-kitty-session
        sophia-native-chrome-proof
        sophia-status
        sophia-stop
        sophia-rollback
        sophia-setup-uinput
        sophia-record-fallback-run
        sophia-record-emergency-run
        sophia-record-native-chrome-run
        sophia-verify-login-cycle
        sophia-verify-emergency
        sophia-verify-fallback
        sophia-verify-native-chrome
    )
    SOPHIA_SURFACE_DESKTOPS=(
        sophia-kitty
    )
    # Proof and evidence workflows, kept out of the login menu unless asked for.
    #
    # operations.md calls these diagnostics and evidence rather than sessions:
    # Native Chrome is "a bounded Engine-chrome diagnostic", Hagia Promotion is
    # "immutable release evidence", and the other two are bounded proof gates.
    # Only "Sophia Hagia (Native Policy)" is "the ordinary user-profile
    # session". They ship in every release and keep their operator commands, so
    # nothing here decides whether a proof can be run -- only whether it also
    # takes a line in greetd, where four of them crowded out the two entries
    # someone logs in with. Set SOPHIA_INSTALL_PROOF_SESSIONS=1 to list them.
    SOPHIA_SURFACE_PROOF_DESKTOPS=(
        sophia-native-chrome-proof
    )
    SOPHIA_SURFACE_PROOF_DESKTOP_COMMANDS=(
        sophia-native-chrome-proof
    )
    SOPHIA_SURFACE_RETIRED_COMMANDS=(
        sophia-run-cycles
        sophia-record-run
        sophia-soak-progress
        sophia-verify-cycles
        sophia-verify-soak
        sophia-verify-xmobar-work-area
        sophia-verify-xmonad-run
    )
    SOPHIA_SURFACE_RETIRED_DESKTOPS=(
        sophia
        sophia-cycle-proof
    )
    SOPHIA_SURFACE_RETIRED_DESKTOP_COMMANDS=(
        sophia-session
        sophia-run-cycles
    )
    if [[ "$hagia_included" == true ]]; then
        SOPHIA_SURFACE_COMMANDS+=("${SOPHIA_SURFACE_HAGIA_COMMANDS[@]}")
        SOPHIA_SURFACE_DESKTOPS+=(sophia-hagia)
        SOPHIA_SURFACE_PROOF_DESKTOPS+=(
            sophia-hagia-promotion
            sophia-firefox-proof
            sophia-recovery-proof
        )
        SOPHIA_SURFACE_PROOF_DESKTOP_COMMANDS+=(
            sophia-hagia-promotion-session
            sophia-firefox-proof
            sophia-recovery-proof
        )
    fi
    if sophia_surface_proof_sessions_requested; then
        SOPHIA_SURFACE_DESKTOPS+=("${SOPHIA_SURFACE_PROOF_DESKTOPS[@]}")
    fi
}

# Whether proof and evidence entries should appear in the login menu. The
# release still carries them either way; this only selects what greetd lists.
sophia_surface_proof_sessions_requested() {
    case "${SOPHIA_INSTALL_PROOF_SESSIONS:-}" in
        1|true|yes) return 0 ;;
        *) return 1 ;;
    esac
}

sophia_surface_validate_entries() {
    local release="$1" command desktop

    sophia_surface_select_entries "$release" || return
    [[ -x "$release/target/release/sophia" ]] || {
        echo "Release is missing the Sophia CLI." >&2
        return 1
    }
    for command in "${SOPHIA_SURFACE_COMMANDS[@]}"; do
        [[ -x "$release/bin/$command" ]] || {
            echo "Release is missing operator command: $command" >&2
            return 1
        }
    done
    for desktop in "${SOPHIA_SURFACE_DESKTOPS[@]}"; do
        [[ -f "$release/share/wayland-sessions/$desktop.desktop" ]] || {
            echo "Release is missing session entry: $desktop.desktop" >&2
            return 1
        }
    done
}

sophia_surface_remove_absent_hagia() {
    local prefix="$1" session_dir="$2" command_dir="$3"
    local command link desktop entry expected index

    [[ "$SOPHIA_SURFACE_HAGIA_INCLUDED" == false ]] || return 0
    for command in "${SOPHIA_SURFACE_HAGIA_COMMANDS[@]}"; do
        link="$command_dir/$command"
        if [[ -L "$link" ]] \
            && [[ "$(readlink "$link")" == "$prefix/current/bin/$command" ]]; then
            rm -f -- "$link"
        fi
    done
    for index in "${!SOPHIA_SURFACE_HAGIA_DESKTOPS[@]}"; do
        desktop="${SOPHIA_SURFACE_HAGIA_DESKTOPS[$index]}"
        command="${SOPHIA_SURFACE_HAGIA_DESKTOP_COMMANDS[$index]}"
        entry="$session_dir/$desktop.desktop"
        expected="Exec=$prefix/current/bin/$command"
        if [[ -f "$entry" && ! -L "$entry" ]] \
            && grep -Fqx "$expected" "$entry"; then
            rm -f -- "$entry"
        fi
    done
}

# Retire proof entries a previous release listed, when this one does not.
# Matches the exact substituted Exec line, so an unrelated file that happens to
# share a name is preserved -- the same rule the hagia retirement uses.
sophia_surface_remove_absent_proofs() {
    local prefix="$1" session_dir="$2"
    local command desktop entry expected index

    ! sophia_surface_proof_sessions_requested || return 0
    for index in "${!SOPHIA_SURFACE_PROOF_DESKTOPS[@]}"; do
        desktop="${SOPHIA_SURFACE_PROOF_DESKTOPS[$index]}"
        command="${SOPHIA_SURFACE_PROOF_DESKTOP_COMMANDS[$index]}"
        entry="$session_dir/$desktop.desktop"
        expected="Exec=$prefix/current/bin/$command"
        if [[ -f "$entry" && ! -L "$entry" ]] \
            && grep -Fqx "$expected" "$entry"; then
            rm -f -- "$entry"
        fi
    done
}

sophia_surface_install() {
    local release="$1" prefix="$2" session_dir="$3" command_dir="$4"
    local command desktop desktop_temp entry expected index link sed_prefix

    # Validate the complete target before changing the installed surface.
    sophia_surface_validate_entries "$release" || return
    install -d -m 755 "$session_dir" "$command_dir"
    sophia_surface_remove_absent_hagia "$prefix" "$session_dir" "$command_dir"
    sophia_surface_remove_absent_proofs "$prefix" "$session_dir"
    for command in "${SOPHIA_SURFACE_RETIRED_COMMANDS[@]}"; do
        link="$command_dir/$command"
        if [[ -L "$link" ]] \
            && [[ "$(readlink "$link")" == "$prefix/current/bin/$command" ]]; then
            rm -f -- "$link"
        fi
    done
    for index in "${!SOPHIA_SURFACE_RETIRED_DESKTOPS[@]}"; do
        desktop="${SOPHIA_SURFACE_RETIRED_DESKTOPS[$index]}"
        command="${SOPHIA_SURFACE_RETIRED_DESKTOP_COMMANDS[$index]}"
        entry="$session_dir/$desktop.desktop"
        expected="Exec=$prefix/current/bin/$command"
        if [[ -f "$entry" && ! -L "$entry" ]] \
            && grep -Fqx "$expected" "$entry"; then
            rm -f -- "$entry"
        fi
    done
    for command in "${SOPHIA_SURFACE_COMMANDS[@]}"; do
        ln -sfn "$prefix/current/bin/$command" "$command_dir/$command"
    done
    # Every supported release already carries the CLI here. Point through
    # current so activation and rollback select the same binary as the session.
    ln -sfn "$prefix/current/target/release/sophia" "$command_dir/sophia"

    sed_prefix="${prefix//\\/\\\\}"
    sed_prefix="${sed_prefix//&/\\&}"
    sed_prefix="${sed_prefix//|/\\|}"
    for desktop in "${SOPHIA_SURFACE_DESKTOPS[@]}"; do
        desktop_temp="$(mktemp "$session_dir/.$desktop.desktop.XXXXXX")"
        if ! sed "s|@SOPHIA_INSTALL_PREFIX@|$sed_prefix|g" \
            "$release/share/wayland-sessions/$desktop.desktop" \
            >"$desktop_temp"; then
            rm -f -- "$desktop_temp"
            return 1
        fi
        chmod 644 "$desktop_temp"
        mv -f "$desktop_temp" "$session_dir/$desktop.desktop"
    done
}

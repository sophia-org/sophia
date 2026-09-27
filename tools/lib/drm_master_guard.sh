# Shared guard for read-only DRM checks that still need atomic validation.
#
# Atomic commits require DRM master even when they carry TEST_ONLY, so a check run
# while a compositor holds the card reports "not master" and concludes nothing. A
# run that proves nothing is worse than no run, because its output looks like a
# result. Refusing up front is the cheaper failure.

# Ambient display endpoints are additional refusal evidence. Process detection
# belongs to the required external checker, never a list of desktop products.
sophia_drm_master_blockers() {
    [[ -n "${DISPLAY:-}" ]] && echo "DISPLAY=${DISPLAY} is set"
    [[ -n "${WAYLAND_DISPLAY:-}" ]] && echo "WAYLAND_DISPLAY=${WAYLAND_DISPLAY} is set"
    return 0
}

# Refuses unless the card looks free. `$1` names the override variable a caller
# honors, so the message points at the right escape hatch.
sophia_require_drm_master_available() {
    local override="${1:-SOPHIA_DRM_MASTER_FORCE}"
    local blockers checker_bin tty_name allow_active=false
    blockers="$(sophia_drm_master_blockers)"
    if [[ "${!override:-0}" == "1" ]]; then
        allow_active=true
    fi
    if [[ -n "$blockers" ]]; then
        echo "A display endpoint is configured:" >&2
        sed 's/^/  - /' <<<"$blockers" >&2
        if [[ "$allow_active" != true ]]; then
            echo "Use a bare TTY, or explicitly set $override=1." >&2
            return 1
        fi
        echo "$override=1; overriding the ambient display refusal." >&2
    fi

    checker_bin="${SOPHIA_BIN:-}"
    if [[ "$checker_bin" != /* || ! -f "$checker_bin" || ! -x "$checker_bin" ]]; then
        echo "Set SOPHIA_BIN to an absolute built Sophia executable with session check-host." >&2
        return 1
    fi
    tty_name="$(tty 2>/dev/null)" || {
        echo "DRM validation requires a TTY for the external host preflight." >&2
        return 1
    }
    # Even force cannot bypass a missing checker, malformed verdict or timeout.
    "$checker_bin" session check-host "--tty=$tty_name" "--allow-active=$allow_active" || return 1
    if [[ ! -d /dev/dri ]]; then
        echo "/dev/dri is missing; no primary card node to use." >&2
        return 1
    fi
    # The check makes no reservation: the atomic ioctl can still refuse with
    # MasterUnavailable if ownership changes or detection misses an owner.
    return 0
}

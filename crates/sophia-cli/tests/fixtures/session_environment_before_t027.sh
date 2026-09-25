# Frozen environment preparation before t027; differential tests only.
session_environment=(
    SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1
    "SOPHIA_SESSION_TTY=$tty_name"
)
if [[ "$FIREFOX_M10_ANY_PROOF" == true ]]; then
    session_environment+=(
        "SOPHIA_FIREFOX_M10_KITTY_PROBE_DIR=$firefox_m10_probe_dir"
        "SOPHIA_FIREFOX_M10_PROOF_SLICE=$(
            if [[ "$FIREFOX_M10_SELECTION_PROOF" == true ]]; then
                echo selection
            elif [[ "$FIREFOX_M10_PRIMARY_PROOF" == true ]]; then
                echo primary
            elif [[ "$FIREFOX_M10_DIALOG_PROOF" == true ]]; then
                echo dialog
            elif [[ "$FIREFOX_M10_RENDERING_PROOF" == true ]]; then
                echo rendering
            elif [[ "$FIREFOX_M10_LIFECYCLE_PROOF" == true ]]; then
                echo lifecycle
            else
                echo promotion
            fi
        )"
        GDK_BACKEND=x11
        GTK_USE_PORTAL=0
        MOZ_ENABLE_WAYLAND=0
        MOZ_FORCE_DISABLE_E10S=1
        MOZ_USE_XINPUT2=1
    )
fi
if [[ "${SOPHIA_SESSION_VERBOSE_TRACE:-false}" == true ]]; then
    session_environment+=(
        "SOPHIA_LIVE_SESSION_DIAGNOSTIC=${SOPHIA_LIVE_SESSION_DIAGNOSTIC-1}"
        "SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE=${SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE-1}"
        "SOPHIA_X11_AUTHORITY_TRACE=${SOPHIA_X11_AUTHORITY_TRACE-1}"
    )
fi
if [[ "$FIREFOX_M10_RENDERING_PROOF" == true ]]; then
    session_environment+=(
        "SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE=${SOPHIA_NATIVE_COMPOSITION_PIXEL_TRACE-final-regions}"
        "SOPHIA_X11_PIXEL_TRACE=${SOPHIA_X11_PIXEL_TRACE-1}"
    )
fi
# The session bus every desktop application expects to find.
#
# A toolkit acquires the bus before it opens the display. A browser wants the
# secret service for its password store, a file manager wants xfconf for its
# settings, and portals and notifications are bus services too. This ran every
# profile with `unix:path=/dev/null`, an address that parses and refuses every
# connection, so all of that failed and said so once per attempt.
#
# That address is right for a measurement and only for a measurement: with no
# address at all a toolkit autolaunches a daemon of its own, which costs time
# the run then charges to the graphics path. So it stays available, by asking
# for it. A desktop gets a real bus scoped to the session, which is what the
# QEMU acceptance path already does, for the reason written there.
session_bus_launcher=()
if [[ "${SOPHIA_ISOLATE_SESSION_BUS:-0}" == 1 ]]; then
    session_environment+=(DBUS_SESSION_BUS_ADDRESS=unix:path=/dev/null)
    session_bus_mode=isolated
elif [[ -n "${DBUS_SESSION_BUS_ADDRESS:-}"
    && "${DBUS_SESSION_BUS_ADDRESS:-}" != unix:path=/dev/null ]]; then
    # One was provided already. Nesting a second would split this session's
    # applications across two buses that cannot see each other, which is the
    # failure the shared bus exists to prevent.
    session_bus_mode=inherited
elif command -v dbus-run-session >/dev/null 2>&1; then
    # The daemon is a child of the wrapper, and the wrapper leads the process
    # group `setsid` creates below, so the bus dies with the session on both
    # the normal exit and the watchdog's group kill. Nothing is left running
    # for the next login to inherit.
    session_bus_launcher=(dbus-run-session --)
    session_bus_mode=session_scoped
else
    # Leaving the address unset lets each application autolaunch its own bus.
    # That is worse than one shared bus, because those applications cannot
    # then talk to each other, and much better than an address that refuses
    # every connection.
    session_bus_mode=unavailable
fi

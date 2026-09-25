# Frozen pre-migration argument builder, for differential tests only.
session_args=(
    session
    run
    --session-mode=normal
    --display="$DISPLAY_NAME"
    --native-scanout
    "${input_source_args[@]}"
)
if [[ "$SESSION_PROFILE" != standalone && -n "${SOPHIA_CORE_CONFIG:-}" ]]; then
    [[ "$SOPHIA_CORE_CONFIG" == /* && -f "$SOPHIA_CORE_CONFIG" ]] || {
        echo "SOPHIA_CORE_CONFIG must be an absolute existing path." >&2
        exit 1
    }
    session_args+=("--config=$SOPHIA_CORE_CONFIG")
fi
# A user-composed desktop may start only panels or background applications.
# A focused application frame is a proof requirement, not a login requirement.
if [[ "$SESSION_STARTUP" != none && ( "$SESSION_PROFILE" != hagia
    || "$FIREFOX_M10_ANY_PROOF" == true || "$TRUECOLOR_PROOF" == true ) ]]; then
    session_args+=(--startup-ready-timeout-ms=8000)
fi
if [[ "$SESSION_PROFILE" == standalone ]]; then
    standalone_direct_scanout=0
    if [[ "${SOPHIA_ENABLE_DIRECT_SCANOUT:-0}" == 1 ]]; then
        standalone_direct_scanout=1
    fi
    # This profile runs no window manager.
    #
    # It cannot: `sophia-wm-demo` lost its serving mode in 83596bfc with the
    # experimental WM API v7, so the only subcommands left are proof clients
    # and every session naming it as `--wm-process` dies at startup with a
    # usage string. It does not need one either. A single-application proof
    # has nothing to arrange, a session without a WM honours the client's own
    # geometry, and no WM means no focus ring and no border over the frame --
    # which is what direct scanout requires anyway.
    #
    # There is no logout shortcut either. Shortcuts are resolved against a
    # policy client's configuration (`wm/public_policy.rs:2136-2145`), so a
    # session without one registers none at all. The ordinary exit is the
    # application exiting, which `--exit-when-startup-exits` turns into the
    # session exiting.
    if (( standalone_direct_scanout == 1 )); then
        # The compiled default's fallback chrome draws a focus ring, and a
        # session with no window manager uses the fallback. That ring lowers to
        # a Border command, which made thirty-nine of one run's frames
        # ineligible for a reason that has nothing to do with the client. This
        # config differs from the compiled default in that one line.
        standalone_core_template="$ROOT_DIR/tools/fixtures/direct_scanout_core.kdl"
        standalone_core_config="$STATE_DIR/standalone-core.kdl"
        if [[ ! -f "$standalone_core_template" ]]; then
            echo "The standalone core configuration is missing: $standalone_core_template" >&2
            exit 1
        fi
        install -m 600 "$standalone_core_template" "$standalone_core_config"
        # And a desktop profile, so the probe is hermetic. Without one the
        # session discovers whatever the operator has installed -- here
        # `~/.config/hagia/config.kdl`, which enables a shell and binds
        # spawn-terminal, neither of which a one-application proof can provide.
        # `--config` and `--no-config` are mutually exclusive, so a probe that
        # needs a core config cannot fall back to the compiled desktop profile.
        standalone_desktop_template="$ROOT_DIR/tools/fixtures/direct_scanout_desktop.kdl"
        standalone_desktop_profile="$STATE_DIR/standalone-desktop.kdl"
        if [[ ! -f "$standalone_desktop_template" ]]; then
            echo "The standalone desktop profile is missing: $standalone_desktop_template" >&2
            exit 1
        fi
        install -m 600 "$standalone_desktop_template" "$standalone_desktop_profile"
        session_args+=(
            "--config=$standalone_core_config"
            "--desktop-profile=$standalone_desktop_profile"
        )
        # The overlay proof, when the gate asked for it. The session opens an
        # overlay over a directly scanned frame itself, because the shell that
        # would open one in a product session is exactly what this session does
        # not run.
        # Moves the cursor over directly scanned frames, to test the claim
        # that the legacy ioctl keeps working there.
        if [[ "${SOPHIA_DIRECT_CURSOR_PROOF:-0}" == 1 ]]; then
            session_args+=(--direct-cursor-proof)
        fi
        if [[ "${SOPHIA_DIRECT_OVERLAY_PROOF:-0}" == 1 ]]; then
            session_args+=(--direct-overlay-proof)
            # A cost run holds the overlay far longer than a transition
            # proof does: what it needs from the composed phase is a
            # population, and this client repaints on a cursor blink.
            if [[ -n "${SOPHIA_DIRECT_OVERLAY_HOLD_TICKS:-}" ]]; then
                session_args+=("--direct-overlay-hold-ticks=$SOPHIA_DIRECT_OVERLAY_HOLD_TICKS")
            fi
        fi
    else
        session_args+=(--no-config)
    fi
    # Drive the cursor atomically rather than through the legacy ioctl.
    #
    # Outside the direct-scanout branch on purpose: the cursor path has
    # nothing to do with whether client buffers reach the plane directly.
    # It was inside it, so a benchmark that did not enable direct scanout
    # silently ran the legacy path while SOPHIA_ATOMIC_CURSOR=1 was set --
    # a whole physical run spent measuring the thing it was meant to replace.
    if [[ "${SOPHIA_ATOMIC_CURSOR:-0}" == 1 ]]; then
        session_args+=(--atomic-cursor)
    fi
    # The escape hatch, for a session that wants the ioctl a refused probe
    # would have given it anyway.
    if [[ "${SOPHIA_LEGACY_CURSOR:-0}" == 1 ]]; then
        session_args+=(--legacy-cursor)
    fi
    # A bounded measurement, not a daily session.
    #
    # This is what keeps the run's records where a report can read them.
    # `sophia` installs the reduced per-session evidence capture only for an
    # "ordinary" session -- `--session-mode=normal` with no `--proof` and no
    # `--max-runtime-ms` -- and a captured session's records go to that reduced
    # log instead of stdout, stripped of the very numbers a report is for:
    # `present_cadence` keeps `samples` and loses `mean_fps` and
    # `p95_frame_msec`. The benchmark reporter reads this profile's
    # `session.log`, so without this the cadence summary it needs is simply not
    # there, and it fails on the bounded-completion record it also cannot find.
    #
    # The value is a backstop, never the exit: the client's own duration ends
    # the run through `--exit-when-startup-exits`, and the independent watchdog
    # is sized by the caller. Sitting above the watchdog means this cap cannot
    # truncate a workload -- it marks the session bounded and keeps its full,
    # unreduced records on stdout, which is where this profile's report reads
    # them from.
    standalone_max_runtime_msec=$(( ( ${SESSION_WATCHDOG_SECONDS:-600} + 30 ) * 1000 ))
    session_args+=(
        "--session-app=standalone=$standalone_bin"
        --session-start=standalone
        --exit-when-startup-exits
        "--max-runtime-ms=$standalone_max_runtime_msec"
    )
    if [[ "$standalone_workload" == vkcube ]]; then
        session_args+=(
            --session-app-arg=standalone=--wsi
            --session-app-arg=standalone=xcb
        )
    fi
    if [[ "$standalone_workload" == kitty ]]; then
        standalone_width="${SOPHIA_STANDALONE_WIDTH:-2560}"
        standalone_height="${SOPHIA_STANDALONE_HEIGHT:-1440}"
        [[ "$standalone_width" =~ ^[1-9][0-9]*$
            && "$standalone_height" =~ ^[1-9][0-9]*$ ]] || {
            echo "SOPHIA_STANDALONE_WIDTH and SOPHIA_STANDALONE_HEIGHT must be positive integers." >&2
            exit 1
        }
        # No window manager means no one to fullscreen this, so it is sized to
        # the head it will land on. A bare number is already pixels here; a `c`
        # suffix would mean cells, and a `px` suffix is a parse error that
        # Kitty reports as "errors parsing configuration" inside its own
        # window, where a session log never sees it.
        #
        # Opaque, because a translucent background makes the client's alpha
        # part of the image and nothing behind it would be drawn on a plane.
        # Its own config is ignored so the probe does not depend on a dotfile.
        kitty_overrides=(
            linux_display_server=x11
            background_opacity=1
            remember_window_size=no
            "initial_window_width=$standalone_width"
            "initial_window_height=$standalone_height"
            confirm_os_window_close=0
        )
        session_args+=(
            --session-app-arg=standalone=--config
            --session-app-arg=standalone=NONE
        )
        for override in "${kitty_overrides[@]}"; do
            session_args+=(
                --session-app-arg=standalone=--override
                "--session-app-arg=standalone=$override"
            )
        done
        # Kitty reports a bad override as "errors parsing configuration" inside
        # its own window, which a session log never sees and which costs a
        # whole physical run to discover -- `initial_window_width=2560px` did
        # exactly that. Its own parser answers here, before anything takes DRM.
        kitty_override_check=()
        for override in "${kitty_overrides[@]}"; do
            kitty_override_check+=("${override/=/ }")
        done
        if ! "$standalone_bin" +runpy 'import sys
from kitty.config import parse_config
for spec in sys.argv[1:]:
    parse_config([spec])
' "${kitty_override_check[@]}" >/dev/null 2>"$STATE_DIR/kitty-override-check.log"; then
            echo "Kitty refused one of the probe's overrides:" >&2
            cat "$STATE_DIR/kitty-override-check.log" >&2
            exit 1
        fi
        # A bounded client, so the run needs no operator beyond starting it:
        # the shell exits and `--exit-when-startup-exits` ends the session.
        # Must come last -- everything after the command is the command's.
        session_args+=(
            --session-app-arg=standalone=sh
            --session-app-arg=standalone=-c
            "--session-app-arg=standalone=sleep ${SOPHIA_STANDALONE_HOLD_SECONDS:-20}"
        )
    fi
    if (( standalone_direct_scanout == 1 )) && [[ "$standalone_workload" == vkcube ]]; then
        # Sized to the head and bounded, so the probe needs no operator beyond
        # starting it. A client that is not exactly the head's size is not
        # eligible, and the verdict histogram says `layer_not_head_sized` when
        # these do not match the mode -- which is the answer, not a failure of
        # the run.
        : "${SOPHIA_STANDALONE_FRAME_COUNT:=600}"
        : "${SOPHIA_STANDALONE_WIDTH:=2560}"
        : "${SOPHIA_STANDALONE_HEIGHT:=1440}"
    fi
    if [[ -n "${SOPHIA_STANDALONE_FRAME_COUNT:-}" ]]; then
        [[ "$standalone_workload" == vkcube ]] || {
            echo "SOPHIA_STANDALONE_FRAME_COUNT is valid only for the vkcube workload." >&2
            exit 1
        }
        [[ "$SOPHIA_STANDALONE_FRAME_COUNT" =~ ^[1-9][0-9]*$ ]] || {
            echo "SOPHIA_STANDALONE_FRAME_COUNT must be a positive integer." >&2
            exit 1
        }
        standalone_width="${SOPHIA_STANDALONE_WIDTH:-500}"
        standalone_height="${SOPHIA_STANDALONE_HEIGHT:-500}"
        standalone_present_mode="${SOPHIA_STANDALONE_PRESENT_MODE:-2}"
        [[ "$standalone_width" =~ ^[1-9][0-9]*$
            && "$standalone_height" =~ ^[1-9][0-9]*$ ]] || {
            echo "SOPHIA_STANDALONE_WIDTH and SOPHIA_STANDALONE_HEIGHT must be positive integers." >&2
            exit 1
        }
        [[ "$standalone_present_mode" =~ ^[0-3]$ ]] || {
            echo "SOPHIA_STANDALONE_PRESENT_MODE must be a Vulkan present mode from 0 through 3." >&2
            exit 1
        }
        session_args+=(
            --session-app-arg=standalone=--c
            "--session-app-arg=standalone=$SOPHIA_STANDALONE_FRAME_COUNT"
            --session-app-arg=standalone=--width
            "--session-app-arg=standalone=$standalone_width"
            --session-app-arg=standalone=--height
            "--session-app-arg=standalone=$standalone_height"
            --session-app-arg=standalone=--present_mode
            "--session-app-arg=standalone=$standalone_present_mode"
        )
    fi
else
    if [[ "$normal_application_defaults" == true ]]; then
        sophia_append_session_application_default_args session_args terminal "$terminal_bin"
        # The ordinary desktop profile selects startup apps. Proofs retain an
        # explicit CLI selection; the normal terminal is only a fallback.
        if [[ "$SESSION_STARTUP" != none && -n "$terminal_bin" ]]; then
            session_args+=(--session-start-default=terminal)
        fi
    elif [[ "$SESSION_STARTUP" == none ]]; then
        sophia_append_session_terminal_registration_args \
            session_args "$terminal_kind" "$terminal_bin"
    else
        sophia_append_session_terminal_base_args \
            session_args "$terminal_kind" "$terminal_bin"
    fi
    if [[ "$TRUECOLOR_PROOF" == true ]]; then
        session_args+=(
            "--session-app-arg=terminal=$ROOT_DIR/tools/fixtures/truecolor_kitty_probe.sh"
        )
    elif [[ "$FIREFOX_M10_PRIMARY_PROOF" == true ]]; then
        session_args+=(
            "--session-app-arg=terminal=$ROOT_DIR/tools/fixtures/firefox_m10_primary_kitty_probe.sh"
        )
    elif [[ "$FIREFOX_M10_SELECTION_PROOF" == true ]]; then
        session_args+=(
            "--session-app-arg=terminal=$ROOT_DIR/tools/fixtures/firefox_m10_selection_kitty_probe.sh"
        )
    elif [[ "$FIREFOX_M10_PROOF" == true || "$FIREFOX_M10_LIFECYCLE_PROOF" == true ]]; then
        session_args+=(
            "--session-app-arg=terminal=$ROOT_DIR/tools/fixtures/firefox_m10_kitty_probe.sh"
        )
    elif [[ "$normal_application_defaults" != true ]]; then
        sophia_append_session_terminal_title_args \
            session_args "$terminal_kind" "Sophia ${SESSION_PROFILE^} TTY3"
    fi
fi
if [[ "$SESSION_PROFILE" == hagia ]]; then
    desktop_profile="${SOPHIA_DESKTOP_PROFILE:-}"
    [[ "$desktop_profile" == /* && -f "$desktop_profile" ]] || {
        echo "Sophia's Hagia desktop profile must be an absolute existing path: ${desktop_profile:-unset}" >&2
        exit 1
    }
    session_args+=(
        "--desktop-profile=$desktop_profile"
        --wm-interface=sophia_wm_v1
    )
    if [[ -n "$SOPHIA_HAGIA_BIN" ]]; then
        session_args+=(--wm-process-default="$SOPHIA_HAGIA_BIN")
    fi
    if [[ -n "${SOPHIA_HAGIA_SHELL_BIN:-}" ]]; then
        session_args+=("--shell-process-default=$SOPHIA_HAGIA_SHELL_BIN")
    fi
    if [[ "$TRUECOLOR_PROOF" == true ]]; then
        session_args+=(
            "--session-app=palette=$SOPHIA_BIN"
            --session-app-arg=palette=x-authority-truecolor-palette-client
            --session-start=palette
        )
    fi
    if [[ "$FIREFOX_M10_ANY_PROOF" == true ]]; then
        firefox_page="file://$ROOT_DIR/tools/fixtures/firefox_m8_local_page.html"
        if [[ "$FIREFOX_M10_DIALOG_PROOF" == true ]]; then
            firefox_page="${firefox_page}?dialog_only=1"
        elif [[ "$FIREFOX_M10_PRIMARY_PROOF" == true ]]; then
            firefox_page="${firefox_page}?primary_only=1"
        elif [[ "$FIREFOX_M10_RENDERING_PROOF" == true ]]; then
            firefox_page="${firefox_page}?rendering_only=1"
        elif [[ "$FIREFOX_M10_PROOF" == true ]]; then
            firefox_page="${firefox_page}?promotion_only=1"
        elif [[ "$FIREFOX_M10_SELECTION_PROOF" == true ]]; then
            firefox_page="${firefox_page}?selection_peer=kitty"
        elif [[ "$FIREFOX_M10_LIFECYCLE_PROOF" == true ]]; then
            firefox_page="${firefox_page}?lifecycle_only=1"
        fi
        session_args+=(
            "--session-app=browser=$hagia_browser_bin"
            --session-app-arg=browser=--no-remote
            --session-app-arg=browser=--new-instance
        )
        session_args+=(
            --session-app-arg=browser=--profile
            "--session-app-arg=browser=$firefox_m10_profile_dir"
            "--session-app-arg=browser=$firefox_page"
        )
    elif [[ "$normal_application_defaults" == true ]]; then
        sophia_append_session_application_default_args session_args browser "$hagia_browser_bin"
    else
        session_args+=(
            "--session-app=browser=$hagia_browser_bin"
            --session-app-arg=browser=--no-remote
            --session-app-arg=browser=--new-instance
        )
    fi
elif [[ "$SESSION_PROFILE" == native ]]; then
    # No `--wm-process` for the same reason the standalone profile has none:
    # `sophia-wm-demo` cannot serve a session since 83596bfc. The session
    # action mapping below is session-level and needs no policy client, so
    # Super+Enter still launches a terminal.
    session_args+=(
        --session-action-app=terminal=terminal
    )
else
    session_args+=(
        --exit-when-startup-exits
    )
fi
if [[ "${SOPHIA_ADMIT_XTEST:-0}" == 1 ]]; then
    session_args+=(--admit-xtest)
fi
session_args+=("$@")

# Every requested flag reached the vector.
#
# An environment variable that asks for a behaviour and is then dropped is
# worse than one that is not honoured at all: the session runs, the evidence
# looks healthy, and it describes the wrong thing. That happened -- a
# glxgears benchmark asked for the atomic cursor, the flag sat behind an
# unrelated guard, and a physical run measured the legacy path while
# reporting success. This refuses instead.
requested_flags=()
[[ "${SOPHIA_ATOMIC_CURSOR:-0}" == 1 ]] && requested_flags+=(--atomic-cursor)
[[ "${SOPHIA_ADMIT_XTEST:-0}" == 1 ]] && requested_flags+=(--admit-xtest)
[[ "${SOPHIA_LEGACY_CURSOR:-0}" == 1 ]] && requested_flags+=(--legacy-cursor)
[[ "${SOPHIA_DIRECT_CURSOR_PROOF:-0}" == 1 ]] && requested_flags+=(--direct-cursor-proof)
[[ "${SOPHIA_DIRECT_OVERLAY_PROOF:-0}" == 1 ]] && requested_flags+=(--direct-overlay-proof)
for requested in ${requested_flags[@]+"${requested_flags[@]}"}; do
    found=false
    for assembled in "${session_args[@]}"; do
        [[ "$assembled" == "$requested" ]] && found=true && break
    done
    if [[ "$found" != true ]]; then
        echo "The session was asked for $requested and did not receive it." >&2
        echo "A run that quietly drops a requested flag measures the wrong thing." >&2
        exit 1
    fi
done

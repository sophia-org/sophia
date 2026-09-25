# Retained discovery oracle; never used by the live launcher.
hagia_browser_bin=""
if [[ "$SESSION_PROFILE" == hagia ]]; then
    if [[ "$FIREFOX_M10_ANY_PROOF" == true ]]; then
        hagia_browser_bin="${SOPHIA_FIREFOX_BIN:-$(command -v firefox || true)}"
    else
        hagia_browser_bin="${SOPHIA_HAGIA_BROWSER_BIN:-$(command -v helium || command -v firefox || true)}"
    fi
    if [[ "$normal_application_defaults" == true ]]; then
        if [[ -n "$hagia_browser_bin" && ! -x "$hagia_browser_bin" ]]; then
            echo "The default browser is not executable: $hagia_browser_bin" >&2
            exit 1
        fi
    elif [[ -z "$hagia_browser_bin" || ! -x "$hagia_browser_bin" ]]; then
        echo "The Hagia profile requires Helium, Firefox, or SOPHIA_HAGIA_BROWSER_BIN." >&2
        exit 1
    fi
fi
terminal_bin=""
standalone_bin=""
standalone_workload=""
glxgears_duration=""
glxgears_width=""
glxgears_height=""
xterm_duration=""
xterm_width=""
xterm_height=""
xterm_lines=""
xterm_interval_msec=""
if [[ "$SESSION_PROFILE" == standalone ]]; then
    standalone_workload="${SOPHIA_STANDALONE_WORKLOAD:-vkcube}"
    case "$standalone_workload" in
        glxgears)
            standalone_default_bin="$(command -v glxgears || true)"
            standalone_requirement=glxgears
            glxgears_duration="${SOPHIA_GLXGEARS_DURATION_SECONDS:-20}"
            glxgears_width="${SOPHIA_GLXGEARS_WIDTH:-500}"
            glxgears_height="${SOPHIA_GLXGEARS_HEIGHT:-500}"
            [[ "$glxgears_duration" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_GLXGEARS_DURATION_SECONDS must be a positive integer." >&2
                exit 1
            }
            [[ "$glxgears_width" =~ ^[1-9][0-9]*$
                && "$glxgears_height" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_GLXGEARS_WIDTH and SOPHIA_GLXGEARS_HEIGHT must be positive integers." >&2
                exit 1
            }
            ;;
        kitty)
            # The client this stack is known to hand DMA-BUFs. vkcube
            # presents through the software path here -- 389 Presents,
            # every one a CPU layer -- while Kitty produced DMA-BUF
            # content in every promoted Hagia archive. Direct scanout
            # needs a client buffer, so the probe uses the one that
            # provides one.
            standalone_default_bin="$(command -v kitty || true)"
            standalone_requirement=kitty
            ;;
        vkcube)
            standalone_default_bin="$(command -v vkcube || true)"
            standalone_requirement=vkcube
            ;;
        xterm)
            standalone_default_bin="$(command -v xterm || true)"
            standalone_requirement=xterm
            xterm_duration="${SOPHIA_XTERM_DURATION_SECONDS:-20}"
            xterm_width="${SOPHIA_XTERM_WIDTH:-500}"
            xterm_height="${SOPHIA_XTERM_HEIGHT:-500}"
            xterm_lines="${SOPHIA_XTERM_LINES:-1}"
            xterm_interval_msec="${SOPHIA_XTERM_INTERVAL_MSEC:-16}"
            [[ "$xterm_duration" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_XTERM_DURATION_SECONDS must be a positive integer." >&2
                exit 1
            }
            [[ "$xterm_width" =~ ^[1-9][0-9]*$
                && "$xterm_height" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_XTERM_WIDTH and SOPHIA_XTERM_HEIGHT must be positive integers." >&2
                exit 1
            }
            [[ "$xterm_lines" =~ ^[1-9][0-9]*$ ]] || {
                echo "SOPHIA_XTERM_LINES must be a positive integer." >&2
                exit 1
            }
            [[ "$xterm_interval_msec" =~ ^[1-9][0-9]*$
                && "$xterm_interval_msec" -le 1000 ]] || {
                echo "SOPHIA_XTERM_INTERVAL_MSEC must be an integer from 1 through 1000." >&2
                exit 1
            }
            ;;
        *)
            echo "SOPHIA_STANDALONE_WORKLOAD must be glxgears, kitty, vkcube, or xterm." >&2
            exit 1
            ;;
    esac
    standalone_bin="${SOPHIA_STANDALONE_APP_BIN:-$standalone_default_bin}"
    if [[ -z "$standalone_bin" || ! -x "$standalone_bin" ]]; then
        echo "The standalone $standalone_workload proof requires $standalone_requirement; set SOPHIA_STANDALONE_APP_BIN to override it." >&2
        exit 1
    fi
else
    terminal_bin="${SOPHIA_TERMINAL_BIN:-$(command -v kitty || true)}"
    terminal_kind=""
    if [[ "$normal_application_defaults" == true ]]; then
        if [[ -n "$terminal_bin" && ! -x "$terminal_bin" ]]; then
            echo "The default terminal is not executable: $terminal_bin" >&2
            exit 1
        fi
    elif [[ -z "$terminal_bin" || ! -x "$terminal_bin" ]]; then
        echo "The graphical session requires Kitty or xterm; set SOPHIA_TERMINAL_BIN if it is installed elsewhere." >&2
        exit 1
    fi
    if [[ "$normal_application_defaults" != true ]]; then
        terminal_kind="$(
            sophia_resolve_session_terminal_kind \
                "$terminal_bin" "${SOPHIA_TERMINAL_KIND:-}"
        )"
    fi
    if [[ "$FIREFOX_M10_ANY_PROOF" == true && "$terminal_kind" != kitty ]]; then
        echo "The Firefox proof profiles require the Kitty terminal adapter." >&2
        exit 1
    fi
    if [[ "$TRUECOLOR_PROOF" == true && "$terminal_kind" != kitty ]]; then
        echo "The TrueColor proof requires the Kitty terminal adapter." >&2
        exit 1
    fi
fi

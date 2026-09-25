#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT_DIR/tools/lib/session_terminal.sh"

[[ "$(sophia_resolve_session_terminal_kind /usr/bin/kitty)" == kitty ]]
[[ "$(sophia_resolve_session_terminal_kind /usr/bin/xterm)" == xterm ]]
[[ "$(sophia_resolve_session_terminal_kind /opt/proof-wrapper xterm)" == xterm ]]
if sophia_resolve_session_terminal_kind /usr/bin/unknown >/dev/null 2>&1; then
    echo "terminal adapter accepted an unknown executable" >&2
    exit 1
fi

registered_args=()
sophia_append_session_terminal_registration_args registered_args kitty /usr/bin/kitty
[[ " ${registered_args[*]} " == *' --session-app=terminal=/usr/bin/kitty '* ]]
[[ " ${registered_args[*]} " != *' --session-start=terminal '* ]]

kitty_args=()
sophia_append_session_terminal_base_args kitty_args kitty /usr/bin/kitty
sophia_append_session_terminal_title_args kitty_args kitty 'Sophia Xmonad TTY3'
[[ " ${kitty_args[*]} " == *' --session-start=terminal '* ]]
[[ " ${kitty_args[*]} " == *' --session-app-arg=terminal=--config '* ]]
[[ " ${kitty_args[*]} " == *' --session-app-arg=terminal=linux_display_server=x11 '* ]]
[[ " ${kitty_args[*]} " == *' --session-app-arg=terminal=--title '* ]]

xterm_args=()
sophia_append_session_terminal_base_args xterm_args xterm /usr/bin/xterm
sophia_append_session_terminal_title_args xterm_args xterm 'Sophia Xmonad TTY3'
[[ " ${xterm_args[*]} " == *' --session-app-arg=terminal=-cm '* ]]
[[ " ${xterm_args[*]} " == *' --session-app-arg=terminal=-dc '* ]]
[[ " ${xterm_args[*]} " == *' --session-app-arg=terminal=-title '* ]]
for kitty_only in --config NONE --override linux_display_server=x11 background_opacity=1; do
    [[ " ${xterm_args[*]} " != *" --session-app-arg=terminal=$kitty_only "* ]] || {
        echo "xterm adapter inherited Kitty argument: $kitty_only" >&2
        exit 1
    }
done
if command -v xterm >/dev/null 2>&1; then
    xterm -cm -dc -title 'Sophia Xterm Adapter Check' -version 2>&1 |
        grep -Eq '^XTerm\([0-9]+\)$'
fi

cargo test --offline --manifest-path "$ROOT_DIR/Cargo.toml" -p sophia-cli \
    --test session_application_arguments --test session_prepare_arguments

echo "session terminal argument checks passed"

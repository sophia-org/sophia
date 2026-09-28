#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
narthex_root=${SOPHIA_NARTHEX_ROOT:-"$(dirname -- "$root")/narthex"}
content_client=${SOPHIA_CONTENT_LIFECYCLE_CLIENT:-}
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM

cd "$root"
sh tools/check_shell_c_wire.sh
cargo run --offline -q -p sophia-protocol --example shell_catalog_action_corpus >"$build_dir/catalog-actions.frames"
cmp "$build_dir/catalog-actions.frames" protocol/golden/sophia-shell-catalog-actions.frames
cargo run --offline -q -p sophia-protocol --example shell_catalog_action_corpus -- --mutations >"$build_dir/catalog-action-mutations.frames"
"${CC:-cc}" -std=c99 -Wall -Wextra -Werror -pedantic \
    vendor/c-desktop-sdk/source/src/shell_wire/frame.c vendor/c-desktop-sdk/source/src/shell_wire/catalog_actions.c \
    vendor/c-desktop-sdk/source/src/tests/sophia_shell_wire_catalog_actions_test.c -o "$build_dir/catalog-action-decoder"
"$build_dir/catalog-action-decoder" "$build_dir/catalog-actions.frames"
"$build_dir/catalog-action-decoder" "$build_dir/catalog-action-mutations.frames"
cargo test --offline -q -p sophia-protocol --test shell_catalog_actions
cargo run --offline -q -p sophia-protocol --example shell_native_launcher_corpus >"$build_dir/native-launcher.frames"
cmp "$build_dir/native-launcher.frames" protocol/golden/sophia-shell-native-launcher.frames
cargo run --offline -q -p sophia-protocol --example shell_native_launcher_corpus -- --mutations >"$build_dir/native-launcher-mutations.frames"
"${CC:-cc}" -std=c99 -Wall -Wextra -Werror -pedantic \
    vendor/c-desktop-sdk/source/src/shell_wire/frame.c vendor/c-desktop-sdk/source/src/shell_wire/native_launcher.c \
    vendor/c-desktop-sdk/source/src/tests/sophia_shell_wire_native_test.c -o "$build_dir/native-launcher-decoder"
"$build_dir/native-launcher-decoder" "$build_dir/native-launcher-mutations.frames"

cargo run --offline -q -p sophia-protocol --example shell_content_corpus \
    >"$build_dir/sophia-shell-content.frames"
cargo run --offline -q -p sophia-protocol --example shell_content_corpus -- --malformed \
    >"$build_dir/sophia-shell-content-malformed.frames"
cmp "$build_dir/sophia-shell-content.frames" protocol/golden/sophia-shell-content.frames
cmp "$build_dir/sophia-shell-content-malformed.frames" protocol/golden/sophia-shell-content-malformed.frames
cargo test --offline -q -p sophia-protocol --test shell_content_wire
cargo test --offline -q -p sophia-runtime --test shell_content_resources
cargo test --offline -q -p sophia-runtime --test shell_content_admission
cargo test --offline -q -p sophia-runtime --test shell_content_transport
cc -std=c11 -Wall -Wextra -Werror -pedantic \
    vendor/c-desktop-sdk/source/src/tests/sophia_shell_content_client.c -o "$build_dir/content-client"
"$build_dir/content-client" --valid protocol/golden/sophia-shell-content.frames
"$build_dir/content-client" --malformed protocol/golden/sophia-shell-content-malformed.frames
# These inverse expectations prove the independent reader rejects invalid bytes
# and does not implement a success-only corpus printer.
if "$build_dir/content-client" --valid protocol/golden/sophia-shell-content-malformed.frames; then
    echo 'content C decoder accepted malformed records' >&2
    exit 1
fi
if "$build_dir/content-client" --malformed protocol/golden/sophia-shell-content.frames; then
    echo 'content C decoder rejected every valid record' >&2
    exit 1
fi
cargo run --offline -q -p sophia-protocol --example shell_v1_corpus -- --valid \
    >"$build_dir/sophia-shell-v1.frames"
cargo run --offline -q -p sophia-protocol --example shell_v1_corpus -- --malformed \
    >"$build_dir/sophia-shell-v1-malformed.frames"
cargo run --offline -q -p sophia-protocol --example shell_tab_corpus >"$build_dir/sophia-shell-tabs.frames"
cmp "$build_dir/sophia-shell-tabs.frames" protocol/golden/sophia-shell-tabs.frames
cargo run --offline -q -p sophia-protocol --example shell_indicator_corpus >"$build_dir/sophia-shell-indicators.frames"
cmp "$build_dir/sophia-shell-indicators.frames" protocol/golden/sophia-shell-indicators.frames
cmp "$build_dir/sophia-shell-v1.frames" protocol/golden/sophia-shell-v1.frames
cmp "$build_dir/sophia-shell-v1-malformed.frames" \
    protocol/golden/sophia-shell-v1-malformed.frames
cargo run --offline -q -p sophia-protocol --example shell_launcher_corpus >"$build_dir/sophia-shell-launcher.frames"
cmp "$build_dir/sophia-shell-launcher.frames" protocol/golden/sophia-shell-launcher.frames
cargo test --offline -q -p sophia-protocol --test shell_launcher
cargo test --offline -q -p sophia-protocol --test shell_wire
cargo test --offline -q -p sophia-protocol --test shell_tabs
cargo test --offline -q -p sophia-protocol --test shell_indicators
# The reference codec had golden frames and a test target but no invocation
# here, so its coverage was retained without ever being run.
cargo test --offline -q -p sophia-protocol --test shell_reference
cargo test --offline -q -p sophia-runtime --test shell_transport

${CC:-cc} -std=c99 -Wall -Wextra -Werror -pedantic \
    vendor/c-desktop-sdk/source/src/tests/sophia_shell_v1_client.c \
    -o "$build_dir/sophia-shell-v1-c-client"
cargo run --offline -q -p sophia-runtime \
    --example shell_descriptor_conformance_host -- \
    "$build_dir/sophia-shell-v1-c-client"

# The content host serves only 9P2000.L. Its independent peer links the pinned
# C SDK, built by the SDK's own makefile without the IPC library, and no Rust.
make -s -C vendor/c-desktop-sdk/source BUILD="$build_dir/c-desktop-sdk" WITH_IPC=0 all
${CC:-cc} -std=c99 -Wall -Wextra -Werror -pedantic -Ivendor/c-desktop-sdk/source/src \
    crates/sophia-conformance/tests/support/shell_content_file_peer.c \
    -L"$build_dir/c-desktop-sdk" -lsophia-desktop -lsophia-9p \
    -o "$build_dir/shell-content-file-peer"
cargo run --offline -q -p sophia-runtime --example shell_content_conformance_host -- \
    "$build_dir/shell-content-file-peer"
# Red mutations, retired-selection refusals and the hand-encoded boundary
# controls against the production file export.
cargo test --offline -q -p sophia-conformance --test shell_content_files
# The protected popout lifecycle test still uses the socket wire (t252 item);
# it keeps the vendored IPC client in its content-lifecycle mode.
${CC:-cc} -std=c11 -Wall -Wextra -Werror -pedantic \
    vendor/c-desktop-sdk/source/src/tests/sophia_shell_content_live_client.c \
    -o "$build_dir/sophia-shell-content-live-c-client"
SOPHIA_CONTENT_LIFECYCLE_CLIENT="$build_dir/sophia-shell-content-live-c-client" \
    cargo test --offline -q -p sophia-backend-live --all-features --lib \
    protected_popout_client -- --ignored

${CC:-cc} -std=c11 -Wall -Wextra -Werror -pedantic \
    vendor/c-desktop-sdk/source/src/tests/sophia_shell_launcher_client.c -o "$build_dir/sophia-shell-launcher-c-client"
cargo run --offline -q -p sophia-runtime --example shell_launcher_conformance_host -- "$build_dir/sophia-shell-launcher-c-client"

# An independent decoder written from the schema, not from the Rust. It must
# also refuse malformed frames itself: a second implementation that accepts
# everything proves nothing about the format being described well enough.
${CC:-cc} -std=c11 -Wall -Wextra -Werror -pedantic \
    vendor/c-desktop-sdk/source/src/tests/sophia_shell_indicator_client.c -o "$build_dir/sophia-shell-indicator-c-client"
"$build_dir/sophia-shell-indicator-c-client" protocol/golden/sophia-shell-indicators.frames
for mutation in stale-active label-padding bad-count; do
    python3 tools/mutate_shell_indicator_corpus.py "$mutation" \
        protocol/golden/sophia-shell-indicators.frames "$build_dir/bad-$mutation.frames"
    if "$build_dir/sophia-shell-indicator-c-client" "$build_dir/bad-$mutation.frames" >/dev/null 2>&1; then
        echo "independent C decoder accepted a $mutation corpus" >&2
        exit 1
    fi
done

if [ ! -f "$narthex_root/src/narthex.nim" ]; then
    echo "Narthex checkout not found at $narthex_root" >&2
    exit 2
fi
cd "$narthex_root"
SOPHIA_ROOT="$root" nim c -r --hints:off --path:src \
    --nimcache:"$build_dir/nimcache-test" \
    -o:"$build_dir/tshell-v1" tests/tshell_v1.nim
SOPHIA_ROOT="$root" nim c -r --hints:off --path:src --nimcache:"$build_dir/nimcache-tabs" -o:"$build_dir/tshell-tabs" tests/tshell_tabs.nim
SOPHIA_ROOT="$root" nim c -r --hints:off --path:src --nimcache:"$build_dir/nimcache-launcher" -o:"$build_dir/tshell-launcher" tests/tshell_launcher.nim
nim c --hints:off --path:src --nimcache:"$build_dir/nimcache-client" \
    -o:"$build_dir/narthex" src/narthex.nim
cd "$root"
cargo run --offline -q -p sophia-runtime \
    --example shell_descriptor_conformance_host -- "$build_dir/narthex"
cargo run --offline -q -p sophia-runtime \
    --example shell_descriptor_conformance_host -- "$build_dir/narthex" --serve
# The reservation half: the real Nim shell claims a bottom strip, Engine's
# coordinator admits it, and the work area shrinks only once the bundle
# commits. Driving it here keeps the claim honest offline, where a wrong band
# costs seconds instead of a rig session.
cargo run --offline -q -p sophia-runtime \
    --example shell_descriptor_conformance_host -- "$build_dir/narthex" --bar-proof

cargo run --offline -q -p sophia-runtime --example shell_launcher_conformance_host -- "$build_dir/narthex"

content_lifecycle=unavailable
if [ -n "$content_client" ]; then
    case "$content_client" in
        /*) ;;
        *) echo 'SOPHIA_CONTENT_LIFECYCLE_CLIENT must be an absolute path' >&2; exit 2 ;;
    esac
    if [ ! -x "$content_client" ]; then
        echo "Content lifecycle client is not executable: $content_client" >&2
        exit 2
    fi
    # A supplied client speaks the 9P content-proof scenario; the socket-wire
    # popout lifecycle above keeps its own vendored client.
    cargo run --offline -q -p sophia-runtime \
        --example shell_content_conformance_host -- "$content_client"
    content_lifecycle=complete
else
    printf '%s\n' \
        'sophia_shell_content_lifecycle schema=1 status=unavailable reason=client_not_supplied native_presentation=false'
fi

printf '%s\n' \
    "sophia_shell_behavior_corpus schema=2 status=complete clients=rust,c,nim protected=true live_serve=true descriptors=2 activations=1 withdrawn=true reservations=1 content_host_wire=9p2000.L content_lifecycle=$content_lifecycle"

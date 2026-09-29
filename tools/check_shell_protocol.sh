#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
content_client=${SOPHIA_CONTENT_LIFECYCLE_CLIENT:-}
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM

cd "$root"
sh tools/check_shell_c_wire.sh
# Whole file records and neutral values retain the semantics of the retired
# socket corpora. Socket frame layout/order tests retired with their codecs.
cargo test --offline -q -p sophia-protocol \
    --test shell_catalog_files --test shell_indicator_files \
    --test shell_content_values --test shell_native_launcher_values \
    --test shell_descriptor_files --test shell_tabs --test shell_reference --test shell_launcher
cargo test --offline -q -p sophia-runtime --test shell_content_resources
cargo test --offline -q -p sophia-runtime --test shell_content_admission
cargo test --offline -q -p sophia-runtime --test shell_content_session_files
cargo test --offline -q -p sophia-runtime --test shell_negotiation_service
cargo test --offline -q -p sophia-runtime --test shell_file_descriptor_negotiation

# The descriptor, tabs, shortcuts and launcher hosts use an independent C
# SDK peer over 9P, including reservation commit/withdrawal and refusal cases.
cargo test --offline -q -p sophia-conformance --test shell_descriptor_modes

# The content host serves only 9P2000.L. Its independent peer links the pinned
# C SDK, built by the SDK's own makefile without the IPC library, and no Rust.
make -s -C vendor/c-desktop-sdk/source BUILD="$build_dir/c-desktop-sdk" all
${CC:-cc} -std=c99 -Wall -Wextra -Werror -pedantic -Ivendor/c-desktop-sdk/source/src \
    crates/sophia-conformance/tests/support/shell_content_file_peer.c \
    -L"$build_dir/c-desktop-sdk" -lsophia-desktop -lsophia-9p \
    -o "$build_dir/shell-content-file-peer"
cargo run --offline -q -p sophia-runtime --example shell_content_conformance_host -- \
    "$build_dir/shell-content-file-peer"
# Red mutations, retired-selection refusals and the hand-encoded boundary
# controls against the production file export.
cargo test --offline -q -p sophia-conformance --test shell_content_files
# The same composition, action-receipt and retirement assertions also run
# over files, with the independent SDK peer and no IPC library.
cargo test --offline -q -p sophia-backend-live --all-features --lib protected_popout_file_client

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
    # A supplied client speaks the 9P content-proof scenario.
    cargo run --offline -q -p sophia-runtime \
        --example shell_content_conformance_host -- "$content_client"
    content_lifecycle=complete
else
    printf '%s\n' \
        'sophia_shell_content_lifecycle schema=1 status=unavailable reason=client_not_supplied native_presentation=false'
fi

printf '%s\n' \
    "sophia_shell_behavior_corpus schema=2 status=complete clients=rust,c protected=true live_serve=true descriptors=2 activations=1 withdrawn=true reservations=1 descriptor_host_wire=9p2000.L launcher_host_wire=9p2000.L content_host_wire=9p2000.L content_lifecycle=$content_lifecycle"

#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build=$(mktemp -d)
trap 'rm -rf "$build"' EXIT HUP INT TERM
cd "$root"
ulimit -c 0
cargo run --offline --locked -q -p xtask -- check c-desktop-sdk
make -C vendor/c-desktop-sdk/source -j "${CARGO_BUILD_JOBS:-$(nproc)}" BUILD="$build" check
printf '%s\n' 'sophia_shell_c_wire status=pass native=false'

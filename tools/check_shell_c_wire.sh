#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build=$(mktemp -d)
trap 'rm -rf "$build"' EXIT HUP INT TERM
cd "$root"
ulimit -c 0
nice -n 19 cargo run --offline --locked -j 2 -q -p xtask -- check c-desktop-sdk
nice -n 19 make -C vendor/c-desktop-sdk/source -j 2 BUILD="$build" check
printf '%s\n' 'sophia_shell_c_wire status=pass native=false'

#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/c-shell-files}"
cargo run --offline --locked -q -p xtask -- check c-desktop-sdk
exec cargo test --offline --locked -p sophia-runtime --test shell_files_c --test shell_files_c_roles --test shell_native_sdk -- --test-threads=1

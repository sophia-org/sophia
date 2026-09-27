#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/c-shell-files}"
exec nice -n 19 cargo test --offline -j 2 -p sophia-runtime --test shell_files_c --test shell_files_c_roles -- --test-threads=1

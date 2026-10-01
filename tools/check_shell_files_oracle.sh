#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target/shell-oracle}"
export GOCACHE="${GOCACHE:-$CARGO_TARGET_DIR/go-cache}"
exec cargo test --offline -p sophia-runtime --test shell_files_oracle -- --test-threads=1

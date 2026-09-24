#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build_dir=$(mktemp -d)
trap 'rm -rf "$build_dir"' EXIT HUP INT TERM

cd "$root"
target_dir=${CARGO_TARGET_DIR:-"$root/target"}
case "$target_dir" in /*) ;; *) target_dir="$root/$target_dir" ;; esac
cargo run --offline -q -p sophia-policy-protocol-gen -- --check
cargo test --offline -q -p sophia-protocol --test policy_wire
${CC:-cc} -std=c99 -Wall -Wextra -Werror -pedantic \
    -Ibindings/c \
    bindings/c/sophia_wm_v1.c \
    bindings/c/tests/sophia_wm_v1_conformance.c \
    -o "$build_dir/sophia-wm-v1-conformance"
"$build_dir/sophia-wm-v1-conformance" \
    protocol/golden/sophia-wm-v1.frames \
    protocol/golden/sophia-wm-v1-malformed.frames \
    protocol/golden/sophia-wm-v1.records
${CC:-cc} -std=c99 -Wall -Wextra -Werror -pedantic \
    -Ibindings/c \
    bindings/c/sophia_wm_v1.c \
    bindings/c/tests/sophia_wm_v1_client.c \
    -o "$build_dir/sophia-wm-v1-client"
cargo build --offline -q -p sophia-wm-demo --bin sophia-wm-demo
cargo run --offline -q -p sophia-runtime --example policy_c_conformance_host -- \
    "$build_dir/sophia-wm-v1-client" "$build_dir/c" all
cargo run --offline -q -p sophia-runtime --example policy_c_conformance_host -- \
    "$target_dir/debug/sophia-wm-demo" "$build_dir/rust" all policy-v1-proof
cargo run --offline -q -p sophia-runtime --example policy_c_conformance_host -- \
    "$build_dir/sophia-wm-v1-client" "$build_dir/c-restart" restart
cargo run --offline -q -p sophia-runtime --example policy_c_conformance_host -- \
    "$target_dir/debug/sophia-wm-demo" "$build_dir/rust-restart" restart policy-v1-proof
tools/check_archived_policy_client.sh

printf '%s\n' \
    'sophia_policy_behavior_corpus schema=5 status=complete revision=3 clients=rust,c,archived-c-r3 scenarios=11 sequential=true action=true timeout_recovery=true stale_recovery=true invalid_recovery=true reconnect_restart=true preserved_commit=true archived_client=true'

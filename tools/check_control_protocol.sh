#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
case "${1-}" in
    "") ;;
    --live-owner) export SOPHIA_CONTROL_SANDBOX_PROOF=1 ;;
    *) echo "usage: tools/check_control_protocol.sh [--live-owner]" >&2; exit 2 ;;
esac
cargo run --offline -q -p sophia-policy-protocol-gen -- --check
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s bindings/python/tests -p 'test_control_v1.py' -v
cargo test --offline -q -p sophia-protocol --test control_wire
cargo test --offline -q -p sophia-linux-peer --test socket
cargo test --offline -q -p sophia-config --test control_access
cargo test --offline -q -p sophia-runtime --test control_service
cargo test --offline -q -p sophia-runtime --test admin_file_service --test admin_file_deadlines --test admin_file_recovery --test admin_client_refusals --test admin_files_oracle
cargo test --offline -q -p sophia-cli --all-features --test msg
if [ "${1-}" = --live-owner ]; then
    cargo test --offline -q -p sophia-session --features native-session --lib live_control_tests::real_owner -- --ignored
fi

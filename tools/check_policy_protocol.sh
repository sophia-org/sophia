#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# Complete file records, capability bounds, and neutral policy semantics.
cargo test --offline --locked -q -p sophia-protocol --test wm_file_admission \
    --test wm_file_arrays --test wm_file_controls --test wm_file_envelope
cargo test --offline --locked -q -p sophia-runtime --test policy_capabilities \
    --test policy_profile_handoff --test policy_profile_io --test policy_socket
cargo test --offline --locked -q -p sophia-engine --test policy_projection

# Production 9P export/reducer tests include an independently compiled C SDK
# peer. Protected recovery exercises the actual Session launch and owner.
cargo test --offline --locked -q -p sophia-session --features native-session \
    --lib policy_transport_worker
cargo test --offline --locked -q -p sophia-session --features native-session \
    --lib protected_c_sdk_recovers_after_stale_and_timed_out_projections
cargo test --offline --locked -q -p sophia-session --features native-session \
    --lib real_owner_commits_actions_and_confirms_replacement_commit -- --ignored

printf '%s\n' \
    'sophia_policy_behavior_corpus schema=6 status=complete wire=9p2000.L independent_peer=c-sdk protected_recovery=true protected_replacement=true native_presentation=false'

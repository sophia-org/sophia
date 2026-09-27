#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

cargo fmt --check
cargo xtask check layout
tools/check_github_language_visibility.sh
cargo check --offline -p sophia-cli --features atomic-scanout-smoke-live --quiet
cargo test --offline -p sophia-session --features native-session \
    --test backend_evidence --quiet
cargo test --offline -p sophia-session --features native-session \
    --test input_proof --quiet
cargo test --offline -p sophia-renderer-native-egl --features gbm-platform --quiet
cargo test --offline -p sophia-renderer-live --features "gbm-probe egl-probe" --quiet
cargo test --offline -p sophia-backend-live \
    --features "libdrm-events libinput-events gbm-probe egl-probe" --quiet

# Every maintained shell entry point must remain parseable. This replaces the
# hand-maintained list that kept calling retired compatibility scripts long
# after their owning product path disappeared.
while IFS= read -r -d '' script; do
    bash -n "$script"
done < <(find tools -type f -name '*.sh' -print0)

checks=(
    tools/check_atomic_scanout_verifiers.sh
    tools/check_buffer_age_equivalence.sh
    tools/check_direct_scanout_verifier.sh
    tools/check_direct_scanout_archive_verifier.sh
    tools/check_native_egl_mixed_verifier.sh
    tools/check_session_lifecycle_diagnostics.sh
    tools/check_sophia_input_latency_reporter.sh
    tools/check_live_session_milestone4_verifier.sh
    tools/check_live_record_schema_readers.sh
    tools/check_no_legacy_wm_bridge.sh
)
for check in "${checks[@]}"; do
    if "$check"; then
        continue
    else
        status=$?
    fi
    if [[ "$check" == tools/check_buffer_age_equivalence.sh && "$status" == 2 ]]; then
        echo "buffer-age pixel equivalence skipped: no writable render node"
        continue
    fi
    exit "$status"
done

python3 -c 'compile(open("tools/sophia_tty_mode.py", encoding="utf-8").read(), "tools/sophia_tty_mode.py", "exec")'

# The GLX probes require the modifier-negotiated DRI3 import, and the
# authority advertises layouts only from a measured inventory. The glxgears
# benchmark's preflight failed for eleven days after that inventory replaced
# a fixed list, because nothing here ran it.
cargo run --quiet --offline -p sophia-cli --features native-session \
    -- x-authority-glxgears-smoke

echo "atomic scanout local checks passed"

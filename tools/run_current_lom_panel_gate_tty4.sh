#!/usr/bin/env bash
set -euo pipefail

# Generated profiles must satisfy the configuration reader's ownership policy,
# independently of the operator's inherited (possibly group-writable) umask.
umask 077
GATE_MODE=panel
if [[ "${1:-}" == launcher ]]; then GATE_MODE=launcher; shift; fi
[[ $# -eq 0 ]] || { echo "usage: lom-test [launcher]" >&2; exit 2; }

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOM_SOURCE="${SOPHIA_LOM_SOURCE:-/home/niltempus/dev/lom}"
LOM_TARGET="${SOPHIA_LOM_TARGET_DIR:-$HOME/.cache/lom-target}"
LOM_CONFIG="${SOPHIA_LOM_CONFIG:-$LOM_SOURCE/examples/minimal/live-shell.kdl}"
BEMENU_SOURCE="${SOPHIA_BEMENU_SOURCE:-$HOME/src/bemenu}"
HAGIA_ROOT="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
core_fixture=lom_panel_core.kdl
[[ "$GATE_MODE" != launcher ]] || core_fixture=native_launcher_core.kdl
LOM_CORE_CONFIG="${SOPHIA_LOM_CORE_CONFIG:-$ROOT_DIR/tools/fixtures/$core_fixture}"
WORKLOAD_BUDGETS="$ROOT_DIR/tools/fixtures/lom_workload_budgets.json"
EVIDENCE_DIR="${SOPHIA_LOM_NATIVE_EVIDENCE_DIR:-$ROOT_DIR/.artifacts/lom-panel-native/$(date -u +%Y%m%dT%H%M%SZ)}"

[[ "$(tty)" == /dev/tty4 ]] || { echo "Run this from /dev/tty4 after ending the graphical session." >&2; exit 2; }
[[ "${SOPHIA_LOM_NATIVE_GATE_ARM:-0}" == 1 ]] || { echo "Set SOPHIA_LOM_NATIVE_GATE_ARM=1 to run the native panel gate." >&2; exit 2; }
[[ -z "$(git -C "$ROOT_DIR" status --short)" ]] || { echo "Sophia source must be clean" >&2; exit 2; }
[[ -z "$(git -C "$LOM_SOURCE" status --short)" ]] || { echo "Lom source must be clean" >&2; exit 2; }
[[ -z "$(git -C "$HAGIA_ROOT" status --short)" ]] || { echo "Hagia source must be clean" >&2; exit 2; }
git -C "$HAGIA_ROOT" verify-commit HEAD >/dev/null
git -C "$ROOT_DIR" verify-commit HEAD >/dev/null
git -C "$LOM_SOURCE" verify-commit HEAD >/dev/null
if [[ "$GATE_MODE" == launcher ]]; then
    [[ -z "$(git -C "$BEMENU_SOURCE" status --short)" ]] || { echo "Bemenu source must be clean" >&2; exit 2; }
    BEMENU_COMMIT="$(git -C "$BEMENU_SOURCE" rev-parse HEAD)"
    git -C "$BEMENU_SOURCE" verify-commit "$BEMENU_COMMIT" >/dev/null
fi
[[ ! -e "$EVIDENCE_DIR" ]] || { echo "Evidence directory already exists; refusing to overwrite it" >&2; exit 2; }
mkdir -p "$(dirname "$EVIDENCE_DIR")" "$LOM_TARGET"
mkdir -m 700 "$EVIDENCE_DIR"
cp "$WORKLOAD_BUDGETS" "$EVIDENCE_DIR/workload-budgets.json"
cp "$LOM_CONFIG" "$EVIDENCE_DIR/lom-config.kdl"
cp "$LOM_CORE_CONFIG" "$EVIDENCE_DIR/core.kdl"
cp "$ROOT_DIR/tools/fixtures/lom_panel_desktop.kdl" "$EVIDENCE_DIR/probe-overrides.kdl"
LOM_CONFIG="$EVIDENCE_DIR/lom-config.kdl"
LOM_CORE_CONFIG="$EVIDENCE_DIR/core.kdl"
python3 - "$ROOT_DIR/tools/probes/lom_workload" "$EVIDENCE_DIR/workload-budgets.json" <<'PY'
import json, sys
sys.path.insert(0, sys.argv[1])
from verify import budgets, unique_json_object
with open(sys.argv[2], encoding="utf-8") as source:
    budgets(json.load(source, object_pairs_hook=unique_json_object))
PY
CARGO_TARGET_DIR="$ROOT_DIR/target" cargo build --offline --release -p sophia-cli --features native-session --manifest-path "$ROOT_DIR/Cargo.toml"
CARGO_TARGET_DIR="$LOM_TARGET" cargo build --offline --release --manifest-path "$LOM_SOURCE/Cargo.toml"
LOM_BIN="$LOM_TARGET/release/lom"
SOPHIA_BIN="$ROOT_DIR/target/release/sophia"
HAGIA_BIN="$EVIDENCE_DIR/hagia"
BEMENU_BIN="$EVIDENCE_DIR/bemenu-sophia"
if [[ "$GATE_MODE" == launcher ]]; then
    mkdir -m 700 "$EVIDENCE_DIR/bemenu-source"
    git -C "$BEMENU_SOURCE" archive "$BEMENU_COMMIT" | tar -x -C "$EVIDENCE_DIR/bemenu-source"
    make -C "$EVIDENCE_DIR/bemenu-source" bemenu-sophia EXTRA_WARNINGS=-Werror \
        GIT_SHA1="$BEMENU_COMMIT" GIT_TAG="$BEMENU_COMMIT"
    cp "$EVIDENCE_DIR/bemenu-source/bemenu-sophia" "$BEMENU_BIN"
    chmod 700 "$BEMENU_BIN"
    python3 "$ROOT_DIR/tools/probes/native_launcher/profile.py" \
        --lom "$LOM_BIN" --config "$LOM_CONFIG" --bemenu "$BEMENU_BIN" \
        > "$EVIDENCE_DIR/probe-overrides.kdl"
fi
(cd "$HAGIA_ROOT" && nim c -d:release --hints:off --path:src     --nimcache:"$HOME/.cache/hagia-lom-gate" -o:"$HAGIA_BIN" src/hagia.nim)
wm_profile="${SOPHIA_DESKTOP_PROFILE:-}"
if [[ -z "$wm_profile" ]]; then
    config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
    for candidate in "$config_home/sophia/desktop.kdl" "$config_home/hagia/config.kdl" \
        /etc/sophia/desktop.kdl /etc/hagia/config.kdl "$HAGIA_ROOT/examples/config/default.kdl"; do
        if [[ -e "$candidate" || -L "$candidate" ]]; then
            wm_profile="$candidate"
            break
        fi
    done
fi
[[ "$wm_profile" == /* && -f "$wm_profile" ]] || {
    echo "Select an existing absolute WM profile with SOPHIA_DESKTOP_PROFILE." >&2
    exit 2
}
# Expand includes through the real parser and preserve all configured bindings
# and application declarations. Only the recorded probe overrides differ.
"$SOPHIA_BIN" config print-effective --desktop-profile="$wm_profile" \
    > "$EVIDENCE_DIR/wm-profile.kdl"
CARGO_TARGET_DIR="$ROOT_DIR/target" cargo build --offline --release -p sophia-config --example desktop_profile_probe \
    --manifest-path "$ROOT_DIR/Cargo.toml"
probe_args=()
[[ "$GATE_MODE" != launcher ]] || probe_args+=(--require-launcher-binding)
"$ROOT_DIR/target/release/examples/desktop_profile_probe" \
    "$EVIDENCE_DIR/wm-profile.kdl" "$EVIDENCE_DIR/probe-overrides.kdl" "${probe_args[@]}" \
    > "$EVIDENCE_DIR/desktop.kdl"
"$SOPHIA_BIN" config check --desktop-profile="$EVIDENCE_DIR/desktop.kdl"
{
    printf 'gate_mode=%s\n' "$GATE_MODE"
    if [[ "$GATE_MODE" == launcher ]]; then
        printf 'bemenu_commit=%s\n' "$BEMENU_COMMIT"
        printf 'bemenu_binary_sha256=%s\n' "$(sha256sum "$BEMENU_BIN" | cut -d' ' -f1)"
    fi
    printf 'sophia_commit=%s\n' "$(git -C "$ROOT_DIR" rev-parse HEAD)"
    printf 'sophia_binary_sha256=%s\n' "$(sha256sum "$SOPHIA_BIN" | cut -d' ' -f1)"
    printf 'lom_commit=%s\n' "$(git -C "$LOM_SOURCE" rev-parse HEAD)"
    printf 'lom_binary_sha256=%s\n' "$(sha256sum "$LOM_BIN" | cut -d' ' -f1)"
    printf 'lom_config_sha256=%s\n' "$(sha256sum "$LOM_CONFIG" | cut -d' ' -f1)"
    printf 'hagia_commit=%s\n' "$(git -C "$HAGIA_ROOT" rev-parse HEAD)"
    printf 'hagia_binary_sha256=%s\n' "$(sha256sum "$HAGIA_BIN" | cut -d' ' -f1)"
    printf 'workload_budgets_sha256=%s\n' "$(sha256sum "$EVIDENCE_DIR/workload-budgets.json" | cut -d' ' -f1)"
    printf 'core_config_sha256=%s\n' "$(sha256sum "$LOM_CORE_CONFIG" | cut -d' ' -f1)"
    printf 'desktop_profile_sha256=%s\n' "$(sha256sum "$EVIDENCE_DIR/desktop.kdl" | cut -d' ' -f1)"
    printf 'wm_profile_sha256=%s\n' "$(sha256sum "$EVIDENCE_DIR/wm-profile.kdl" | cut -d' ' -f1)"
    printf 'probe_overrides_sha256=%s\n' "$(sha256sum "$EVIDENCE_DIR/probe-overrides.kdl" | cut -d' ' -f1)"
    printf 'native_runtime_msec=90000\nwatchdog_seconds=110\n'
} > "$EVIDENCE_DIR/identity.manifest"
sha256sum "$SOPHIA_BIN" "$LOM_BIN" "$HAGIA_BIN" "$LOM_CONFIG" "$LOM_CORE_CONFIG" \
    "$EVIDENCE_DIR/desktop.kdl" "$EVIDENCE_DIR/wm-profile.kdl" \
    "$EVIDENCE_DIR/probe-overrides.kdl" "$EVIDENCE_DIR/workload-budgets.json" > "$EVIDENCE_DIR/inputs.sha256"
if [[ "$GATE_MODE" == launcher ]]; then sha256sum "$BEMENU_BIN" >> "$EVIDENCE_DIR/inputs.sha256"; fi
verify_candidate_inputs() {
    if [[ "$GATE_MODE" == launcher ]]; then
        [[ -z "$(git -C "$BEMENU_SOURCE" status --short)" ]]
        [[ "bemenu_commit=$(git -C "$BEMENU_SOURCE" rev-parse HEAD)" == "$(sed -n '/^bemenu_commit=/p' "$EVIDENCE_DIR/identity.manifest")" ]]
    fi
    sha256sum --check --status "$EVIDENCE_DIR/inputs.sha256"
    [[ "sophia_commit=$(git -C "$ROOT_DIR" rev-parse HEAD)" == "$(sed -n '/^sophia_commit=/p' "$EVIDENCE_DIR/identity.manifest")" ]]
    [[ "lom_commit=$(git -C "$LOM_SOURCE" rev-parse HEAD)" == "$(sed -n '/^lom_commit=/p' "$EVIDENCE_DIR/identity.manifest")" ]]
    [[ "hagia_commit=$(git -C "$HAGIA_ROOT" rev-parse HEAD)" == "$(sed -n '/^hagia_commit=/p' "$EVIDENCE_DIR/identity.manifest")" ]]
    [[ -z "$(git -C "$ROOT_DIR" status --short)" && -z "$(git -C "$LOM_SOURCE" status --short)" && -z "$(git -C "$HAGIA_ROOT" status --short)" ]]
}

echo "Evidence: $EVIDENCE_DIR"
echo "Checking Lom's protected GPU and content path before graphics takeover."
SOPHIA_LOM_GPU_PROOF_ARM=1 \
SOPHIA_LOM_GPU_EVIDENCE_DIR="$EVIDENCE_DIR/gpu-content" \
SOPHIA_LOM_SOURCE="$LOM_SOURCE" \
SOPHIA_LOM_TARGET_DIR="$LOM_TARGET" \
SOPHIA_LOM_CONFIG="$LOM_CONFIG" \
SOPHIA_LOM_GPU_RENDER_NODE="${SOPHIA_LOM_GPU_RENDER_NODE:-/dev/dri/renderD128}" \
    "$ROOT_DIR/tools/lom_gpu_content_hardware_proof.sh"

verify_candidate_inputs
if [[ "$GATE_MODE" == panel ]]; then
cat <<'INSTRUCTIONS'
The session ends normally after 90 seconds; the 110-second watchdog is failure recovery only.
Confirm a bar and moving clock on BOTH outputs, then wait ten clock ticks.
Click an INACTIVE workspace number 20 times on EACH bar (40 clicks total).
Alternate the two outputs and two INACTIVE numbers from each bar; finish the clicks within 60 seconds.
Do not click during warmup or after the 40 clicks; let the clocks run until automatic exit.
ACK limits: p95 50ms / maximum 100ms. Native limits: p95 150ms / maximum 300ms.
Missing actions, stale/no-op clicks, restarts, timeouts and retained shutdown credits fail.
INSTRUCTIONS
else
cat <<'INSTRUCTIONS'
Launcher smoke: the session ends after 90 seconds; the watchdog is failure recovery only.
Confirm Lom bars and clocks on both monitors. Use your WM application-launcher binding
(the selected operator profile binds Super+Space). Move the pointer to each monitor before opening its menu;
type a query, dismiss with Escape, reopen and confirm the query resets. Check that
the other bar keeps updating and workspace switching still works. Finally search
for terminal, select the entry named terminal, and press Enter once to launch it. Close the terminal, dismiss any menu,
and wait for automatic exit. Record placement, focus restoration, mouse dismissal,
query/reset and both-monitor observations separately. This is not the 40-action
panel latency workload or complete native acceptance.
INSTRUCTIONS
fi
shell_args=()
[[ "$GATE_MODE" != panel ]] || shell_args+=("--shell-process=$LOM_BIN")
set +e
SOPHIA_BIN="$SOPHIA_BIN" \
SOPHIA_HAGIA_BIN="$HAGIA_BIN" \
SOPHIA_HAGIA_SHELL_BIN="$LOM_BIN" \
SOPHIA_SHELL_CONFIG="$LOM_CONFIG" \
SOPHIA_BUILD_SESSION=false \
SOPHIA_MANAGE_KEYD=true \
SOPHIA_REQUIRE_LOCAL_VT=true \
SOPHIA_TTY_PROFILE=hagia \
SOPHIA_CORE_CONFIG="$LOM_CORE_CONFIG" \
SOPHIA_DESKTOP_PROFILE="$EVIDENCE_DIR/desktop.kdl" \
SOPHIA_SESSION_STARTUP=none \
SOPHIA_SESSION_WATCHDOG_SECONDS=110 \
SOPHIA_DIAGNOSTIC_DIR="$EVIDENCE_DIR/session" \
SOPHIA_UNTRUSTED_SESSION_OUTPUT_LOG="$EVIDENCE_DIR/session/untrusted-session-output.log" \
    "$ROOT_DIR/tools/run_sophia_session.sh" --max-runtime-ms=90000 "${shell_args[@]}" --wm-process="$HAGIA_BIN"
native_status=$?
set -e
printf 'native_exit_status=%s\n' "$native_status" > "$EVIDENCE_DIR/native-outcome.txt"
if [[ "$native_status" -ne 0 ]]; then
    echo "Native panel session ended unexpectedly with status $native_status" >&2
    exit 1
fi
verify_candidate_inputs
grep -q '^sophia_tty_recovery schema=3 .*termios_restored=true ' \
    "$EVIDENCE_DIR/session/recovery.log" \
    && grep -q '^sophia_tty_recovery_verification schema=1 .*keyd_restored=true$' \
        "$EVIDENCE_DIR/session/recovery.log" || {
    echo "Native panel session did not prove complete TTY and keyd recovery" >&2
    exit 1
}

if [[ "$GATE_MODE" == launcher ]]; then
    python3 "$ROOT_DIR/tools/probes/native_launcher/verify.py" "$EVIDENCE_DIR/session/events.0.log" \
        | tee "$EVIDENCE_DIR/launcher-verification.json"
    echo "Launcher transcript passed; visual/focus/placement acceptance remains operator evidence: $EVIDENCE_DIR"
    exit 0
fi

"$ROOT_DIR/tools/verify_lom_panel_native_gate.sh" "$EVIDENCE_DIR/session/events.0.log" \
    | tee "$EVIDENCE_DIR/verification.log"
python3 "$ROOT_DIR/tools/probes/lom_workload/verify.py" \
    --host "$EVIDENCE_DIR/session/events.0.log" \
    --client "$EVIDENCE_DIR/session/untrusted-session-output.log" \
    --budgets "$EVIDENCE_DIR/workload-budgets.json" \
    | tee "$EVIDENCE_DIR/workload-verification.json"
echo "Workload evidence passed. Record your visual/placement observations separately; see $EVIDENCE_DIR"

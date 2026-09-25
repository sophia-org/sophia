#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/lib/proof_checkout.sh
source "$ROOT_DIR/tools/lib/proof_checkout.sh"
hagia_bin="${SOPHIA_HAGIA_BIN:-$(command -v hagia || true)}"
hagia_shell_bin="${SOPHIA_HAGIA_SHELL_BIN:-$(command -v narthex || true)}"
kitty_bin="${SOPHIA_TERMINAL_BIN:-$(command -v kitty || true)}"
browser_bin="${SOPHIA_BROWSER_BIN:-${SOPHIA_FIREFOX_BIN:-}}"
if [[ -z "$browser_bin" ]]; then
    browser_bin="$(command -v helium || command -v firefox || true)"
fi
seat="${SOPHIA_HAGIA_PHYSICAL_SEAT:-}"
display="${SOPHIA_HAGIA_PHYSICAL_DISPLAY:-:291}"
runtime_msec="${SOPHIA_HAGIA_PHYSICAL_RUNTIME_MSEC:-660000}"
sequence_timeout_msec="${SOPHIA_HAGIA_PHYSICAL_SEQUENCE_TIMEOUT_MSEC:-600000}"
evidence="${SOPHIA_HAGIA_PHYSICAL_EVIDENCE:-/tmp/sophia-hagia-policy-physical.log}"
proof_text="${SOPHIA_HAGIA_PHYSICAL_TEXT:-hagiapolicyproof}"
guide="${SOPHIA_HAGIA_PHYSICAL_GUIDE:-$ROOT_DIR/tools/fixtures/hagia_physical_guide.sh}"
hagia_root="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
narthex_root="${SOPHIA_NARTHEX_ROOT:-$ROOT_DIR/../narthex}"
source_commit="${SOPHIA_HAGIA_PHYSICAL_SOURCE_COMMIT:-}"
hagia_commit="${SOPHIA_HAGIA_PHYSICAL_HAGIA_COMMIT:-}"
narthex_commit="${SOPHIA_HAGIA_PHYSICAL_NARTHEX_COMMIT:-}"
recorded_sophia_sha256="${SOPHIA_HAGIA_PHYSICAL_SOPHIA_SHA256:-}"
recorded_hagia_sha256="${SOPHIA_HAGIA_PHYSICAL_HAGIA_SHA256:-}"
recorded_narthex_sha256="${SOPHIA_HAGIA_PHYSICAL_NARTHEX_SHA256:-}"

if [[ ! "$proof_text" =~ ^[a-z]{1,24}$ ]]; then
    echo "SOPHIA_HAGIA_PHYSICAL_TEXT must contain 1-24 lowercase ASCII letters" >&2
    exit 2
fi
if [[ "${SOPHIA_HAGIA_PHYSICAL_ARM:-0}" != "1" ]]; then
    echo "set SOPHIA_HAGIA_PHYSICAL_ARM=1 to acknowledge exclusive DRM/input use" >&2
    exit 2
fi
if [[ -z "$seat" ]]; then
    echo "set SOPHIA_HAGIA_PHYSICAL_SEAT to the libinput seat (normally seat0)" >&2
    exit 2
fi
if [[ -z "$hagia_bin" || ! -x "$hagia_bin" ]]; then
    echo "set SOPHIA_HAGIA_BIN to a built Hagia executable" >&2
    exit 2
fi
if [[ -z "$hagia_shell_bin" || ! -x "$hagia_shell_bin" ]]; then
    echo "set SOPHIA_HAGIA_SHELL_BIN to a built Narthex executable" >&2
    exit 2
fi
if [[ -z "$kitty_bin" || ! -x "$kitty_bin" ]]; then
    echo "set SOPHIA_TERMINAL_BIN to real Kitty" >&2
    exit 2
fi
if [[ -z "$browser_bin" || ! -x "$browser_bin" ]]; then
    echo "set SOPHIA_BROWSER_BIN to an executable browser" >&2
    exit 2
fi
if [[ ! -x "$guide" ]]; then
    echo "set SOPHIA_HAGIA_PHYSICAL_GUIDE to the executable proof guide" >&2
    exit 2
fi
if [[ ! "$source_commit" =~ ^[0-9a-f]{40}$ \
    || ! "$hagia_commit" =~ ^[0-9a-f]{40}$ \
    || ! "$narthex_commit" =~ ^[0-9a-f]{40}$ \
    || ! "$recorded_sophia_sha256" =~ ^[0-9a-f]{64}$ \
    || ! "$recorded_hagia_sha256" =~ ^[0-9a-f]{64}$ \
    || ! "$recorded_narthex_sha256" =~ ^[0-9a-f]{64}$ ]]; then
    echo "run tools/run_current_hagia_policy_gate_tty4.sh to bind all three signed commits and all three binary identities" >&2
    exit 2
fi
if ! proof_checkout_root "$hagia_root"; then
    echo "Hagia checkout not found at $hagia_root" >&2
    exit 2
fi
if [[ ! "$runtime_msec" =~ ^[0-9]+$ ]] || (( runtime_msec < 30000 )); then
    echo "SOPHIA_HAGIA_PHYSICAL_RUNTIME_MSEC must be at least 30000" >&2
    exit 2
fi
if [[ ! "$sequence_timeout_msec" =~ ^[0-9]+$ ]] \
    || (( sequence_timeout_msec < 1000 || sequence_timeout_msec > 600000 )); then
    echo "SOPHIA_HAGIA_PHYSICAL_SEQUENCE_TIMEOUT_MSEC must be 1000-600000" >&2
    exit 2
fi

verify_bound_identity() {
    if [[ -n "$(git -C "$ROOT_DIR" status --short)" \
        || -n "$(git -C "$hagia_root" status --short)" \
        || -n "$(git -C "$narthex_root" status --short)" \
        || "$(git -C "$ROOT_DIR" rev-parse HEAD)" != "$source_commit" \
        || "$(git -C "$hagia_root" rev-parse HEAD)" != "$hagia_commit" \
        || "$(git -C "$narthex_root" rev-parse HEAD)" != "$narthex_commit" ]]; then
        echo "Sophia, Hagia, or Narthex source identity changed during the physical proof." >&2
        exit 1
    fi
    git -C "$ROOT_DIR" verify-commit "$source_commit" >/dev/null 2>&1 || {
        echo "Sophia physical-proof commit does not have a valid signature." >&2
        exit 1
    }
    git -C "$hagia_root" verify-commit "$hagia_commit" >/dev/null 2>&1 || {
        echo "Hagia physical-proof commit does not have a valid signature." >&2
        exit 1
    }
    git -C "$narthex_root" verify-commit "$narthex_commit" >/dev/null 2>&1 || {
        echo "Narthex physical-proof commit does not have a valid signature." >&2
        exit 1
    }
    sophia_sha256="$(sha256sum "$ROOT_DIR/target/release/sophia" | awk '{ print $1 }')"
    hagia_sha256="$(sha256sum "$hagia_bin" | awk '{ print $1 }')"
    narthex_sha256="$(sha256sum "$hagia_shell_bin" | awk '{ print $1 }')"
    if [[ "$sophia_sha256" != "$recorded_sophia_sha256" \
        || "$hagia_sha256" != "$recorded_hagia_sha256" \
        || "$narthex_sha256" != "$recorded_narthex_sha256" ]]; then
        echo "Sophia, Hagia, or Narthex does not match its bound physical-proof identity." >&2
        exit 1
    fi
}

verify_bound_identity

echo "Hagia installed physical policy gate"
echo "This takes exclusive DRM/KMS and seat input. Evidence: $evidence"
echo "Use two connected outputs. After Kitty appears:"
echo "  1. Press Super+Shift+F once; confirm fullscreen."
echo "  2. Press Super+N once; Hagia will checkpoint the new layout and restart."
echo "  3. After the scene returns, confirm fullscreen and the layout survived."
echo "  4. Press Super+Shift+F and Super+N."
echo "  5. Confirm the indicator remains above fullscreen; click 2, then click 1."
echo "  6. Press Super+M twice, Super+Shift+B, then Super+Alt+B."
echo "  7. Move the window with Super+Shift+Right, then Super+Shift+Left."
echo "  8. Press Super+Left, then Super+Right."
echo "  9. Follow the guide through the Hagia Shell activation, restart, inert-pixel, and repeat-activation steps."
echo " 10. Type '$proof_text' and press Enter only when the guide asks."
echo "     The phrase is the final signal and ends the session immediately."

SOPHIA_LIVE_SESSION_DISPLAY="$display" \
SOPHIA_LIVE_SESSION_RUNTIME_MSEC="$runtime_msec" \
SOPHIA_LIVE_SESSION_PERSISTENT_EVIDENCE="$evidence" \
SOPHIA_LIVE_SESSION_VERIFY_MODE=caller \
SOPHIA_HAGIA_PHYSICAL_TEXT="$proof_text" \
    "$ROOT_DIR/tools/live_session_persistent_hardware_proof.sh" \
    --no-config \
    --session-mode=normal \
    "--session-app=terminal=$kitty_bin" \
    --session-start=terminal \
    --session-action-app=terminal=terminal \
    "--session-app=browser=$browser_bin" \
    --session-action-app=browser=browser \
    --session-app-arg=terminal=--config \
    --session-app-arg=terminal=NONE \
    --session-app-arg=terminal=--override \
    --session-app-arg=terminal=linux_display_server=x11 \
    --session-app-arg=terminal=--override \
    --session-app-arg=terminal=remember_window_size=no \
    "--session-app-arg=terminal=$guide" \
    "--wm-process=$hagia_bin" \
    "--shell-process=$hagia_shell_bin" \
    --shell-proof-restart-after-visible=2 \
    --wm-interface=sophia_wm_v1 \
    --wm-proof-restart-after-action=66 \
    "--input-seat=$seat" \
    "--expect-physical-text=$proof_text" \
    "--physical-sequence-timeout-ms=$sequence_timeout_msec" \
    --exit-after-input-proof

verify_bound_identity
printf 'sophia_hagia_policy_identity schema=3 status=bound sophia_commit=%s hagia_commit=%s narthex_commit=%s sophia_sha256=%s hagia_sha256=%s narthex_sha256=%s\n' \
    "$source_commit" "$hagia_commit" "$narthex_commit" "$sophia_sha256" "$hagia_sha256" "$narthex_sha256" \
    | tee -a "$evidence"

"$ROOT_DIR/tools/verify_hagia_policy_physical.sh" "$evidence" "$proof_text"
SOPHIA_HAGIA_BIN="$hagia_bin" \
SOPHIA_HAGIA_SHELL_BIN="$hagia_shell_bin" \
SOPHIA_HAGIA_ROOT="$hagia_root" \
    "$ROOT_DIR/tools/archive_hagia_policy_physical_run.sh" \
    "$evidence" "$proof_text"
echo "Hagia physical policy gate passed"

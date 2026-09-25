#!/usr/bin/env bash
set -euo pipefail
# Release metadata and generated session entries must be readable at login.
umask 022

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ARTIFACT_ROOT="${SOPHIA_ARTIFACT_ROOT:-$ROOT_DIR/.artifacts}"
hagia_bin="${SOPHIA_HAGIA_BIN:-}"
hagia_shell_bin="${SOPHIA_HAGIA_SHELL_BIN:-}"
hagia_root="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
hagia_default_profile=""
hagia_default_profile_sha256=""
hagia_source_commit=""

cd "$ROOT_DIR"
[[ -z "$(git status --short)" ]] || {
    echo "Refusing to package a dirty worktree; commit the exact release first." >&2
    exit 1
}
commit="$(git rev-parse HEAD)"
version="$(awk -F'"' '$1 ~ /^version = / { print $2; exit }' Cargo.toml)"
[[ -n "$version" ]] || {
    echo "Could not resolve workspace version." >&2
    exit 1
}
release_id="${version}-${commit:0:12}"
artifact="$ARTIFACT_ROOT/sophia-$release_id"
[[ ! -e "$artifact" ]] || {
    echo "Release artifact already exists: $artifact" >&2
    exit 1
}

cargo build --offline --release -p sophia-cli --features native-session
cargo build --offline --release -p sophia-wm-demo
if [[ -n "$hagia_bin" && ! -x "$hagia_bin" ]]; then
    echo "SOPHIA_HAGIA_BIN is not executable: $hagia_bin" >&2
    exit 1
fi
if [[ -n "$hagia_bin" && -z "$hagia_shell_bin" ]]; then
    hagia_shell_bin="$(dirname "$hagia_bin")/narthex"
    if [[ ! -x "$hagia_shell_bin" ]]; then
        for candidate in hagia-shell hagia_shell; do
            if [[ -x "$(dirname "$hagia_bin")/$candidate" ]]; then
                hagia_shell_bin="$(dirname "$hagia_bin")/$candidate"
                break
            fi
        done
    fi
fi
if [[ -n "$hagia_bin" && ! -x "$hagia_shell_bin" ]]; then
    echo "SOPHIA_HAGIA_SHELL_BIN is not executable: $hagia_shell_bin" >&2
    exit 1
fi
if [[ -n "$hagia_bin" ]]; then
    [[ -d "$hagia_root/.git" ]] || {
        echo "SOPHIA_HAGIA_ROOT must name the canonical Hagia checkout: $hagia_root" >&2
        exit 1
    }
    hagia_default_profile="$hagia_root/examples/config/default.kdl"
    [[ -f "$hagia_default_profile" && ! -L "$hagia_default_profile" ]] || {
        echo "Hagia's canonical default profile is missing: $hagia_default_profile" >&2
        exit 1
    }
    git -C "$hagia_root" diff --quiet HEAD -- examples/config/default.kdl || {
        echo "Hagia's canonical default profile differs from its commit." >&2
        exit 1
    }
    hagia_source_commit="$(git -C "$hagia_root" rev-parse HEAD)"
    [[ "$hagia_source_commit" =~ ^[0-9a-f]{40}$ ]] || {
        echo "Hagia has no exact source commit." >&2
        exit 1
    }
    git -C "$hagia_root" verify-commit "$hagia_source_commit" >/dev/null 2>&1 || {
        echo "Hagia's source commit does not have a valid signature." >&2
        exit 1
    }
    # Local signed commits are installable. Release identity comes from the
    # exact commit and packaged hashes, independently of remote publication.
    "$hagia_bin" config check --config="$hagia_default_profile" >/dev/null
    target/release/sophia config check \
        --desktop-profile="$hagia_default_profile" >/dev/null
    hagia_default_profile_sha256="$(sha256sum "$hagia_default_profile" | awk '{ print $1 }')"
fi

install -d -m 755 \
    "$artifact/bin" \
    "$artifact/target/release" \
    "$artifact/tools/fixtures" \
    "$artifact/tools/lib" \
    "$artifact/tools/probes" \
    "$artifact/share/doc/sophia" \
    "$artifact/share/sophia-policy/hagia" \
    "$artifact/share/wayland-sessions"
install -m 755 target/release/sophia "$artifact/target/release/sophia"
install -m 755 target/release/sophia-wm-demo \
    "$artifact/target/release/sophia-wm-demo"
install -m 755 tools/installed/sophia-session "$artifact/bin/sophia-session"
if [[ -n "$hagia_bin" ]]; then
    install -m 755 "$hagia_bin" "$artifact/target/release/hagia"
    install -m 755 "$hagia_shell_bin" "$artifact/target/release/narthex"
    install -m 755 tools/installed/sophia-hagia-session \
        "$artifact/bin/sophia-hagia-session"
    install -m 755 tools/installed/sophia-hagia-xtest-session \
        "$artifact/bin/sophia-hagia-xtest-session"
    install -m 755 tools/installed/sophia-hagia-promotion-session \
        "$artifact/bin/sophia-hagia-promotion-session"
    install -m 644 "$hagia_default_profile" \
        "$artifact/share/sophia-policy/hagia/default.kdl"
fi
install -m 755 tools/installed/sophia-kitty-session \
    "$artifact/bin/sophia-kitty-session"
install -m 755 tools/installed/sophia-firefox-proof \
    "$artifact/bin/sophia-firefox-proof"
install -m 755 tools/installed/sophia-xterm-proof \
    "$artifact/bin/sophia-xterm-proof"
install -m 755 tools/installed/sophia-truecolor-proof \
    "$artifact/bin/sophia-truecolor-proof"
install -m 755 tools/installed/sophia-recovery-proof \
    "$artifact/bin/sophia-recovery-proof"
install -m 755 tools/installed/sophia-native-chrome-proof \
    "$artifact/bin/sophia-native-chrome-proof"
install -m 755 tools/installed/capture-runtime-identity.sh \
    "$artifact/bin/capture-runtime-identity"
install -m 755 tools/setup_sophia_uinput.sh \
    "$artifact/bin/sophia-setup-uinput"
install -m 755 tools/status_live_session.sh "$artifact/bin/sophia-status"
install -m 755 tools/installed/sophia-stop "$artifact/bin/sophia-stop"
install -m 755 tools/rollback_live_session.sh "$artifact/bin/sophia-rollback"
install -m 755 tools/record_installed_firefox_attempt.sh \
    "$artifact/bin/sophia-record-firefox-attempt"
install -m 755 tools/record_installed_xterm_run.sh \
    "$artifact/bin/sophia-record-xterm-run"
install -m 755 tools/record_installed_truecolor_run.sh \
    "$artifact/bin/sophia-record-truecolor-run"
install -m 755 tools/record_installed_fallback_run.sh \
    "$artifact/bin/sophia-record-fallback-run"
install -m 755 tools/record_installed_emergency_run.sh \
    "$artifact/bin/sophia-record-emergency-run"
install -m 755 tools/record_installed_watchdog_run.sh \
    "$artifact/bin/sophia-record-watchdog-run"
install -m 755 tools/record_installed_native_chrome_run.sh \
    "$artifact/bin/sophia-record-native-chrome-run"
install -m 755 tools/record_installed_hagia_run.sh \
    "$artifact/bin/sophia-record-hagia-run"
install -m 755 tools/verify_installed_login_cycle.sh \
    "$artifact/bin/sophia-verify-login-cycle"
install -m 755 tools/verify_installed_xterm_session.sh \
    "$artifact/bin/sophia-verify-xterm-run"
install -m 755 tools/verify_installed_xterm_runs.sh \
    "$artifact/bin/sophia-verify-xterm-runs"
install -m 755 tools/verify_installed_truecolor_session.sh \
    "$artifact/bin/sophia-verify-truecolor-run"
install -m 755 tools/verify_installed_truecolor_runs.sh \
    "$artifact/bin/sophia-verify-truecolor-runs"
install -m 755 tools/verify_installed_fallback_session.sh \
    "$artifact/bin/sophia-verify-fallback-session"
install -m 755 tools/verify_installed_fallback_run.sh \
    "$artifact/bin/sophia-verify-fallback"
install -m 755 tools/verify_installed_emergency_archive.sh \
    "$artifact/bin/sophia-verify-emergency"
install -m 755 tools/verify_installed_runtime_identity.sh \
    "$artifact/bin/sophia-verify-runtime-identity"
install -m 755 tools/verify_installed_hagia_session.sh \
    "$artifact/bin/sophia-verify-hagia-session"
install -m 755 tools/verify_installed_hagia_recovery.sh \
    "$artifact/bin/sophia-verify-hagia-recovery"
install -m 755 tools/verify_installed_hagia_archive.sh \
    "$artifact/bin/sophia-verify-hagia"
install -m 755 tools/verify_installed_hagia_archive.sh \
    "$artifact/bin/sophia-verify-hagia-promotion"
install -m 755 tools/verify_installed_session_lifecycle.sh \
    "$artifact/bin/sophia-verify-lifecycle"
install -m 755 tools/verify_installed_watchdog_recovery.sh \
    "$artifact/bin/sophia-verify-watchdog-run"
install -m 755 tools/verify_installed_watchdog_archive.sh \
    "$artifact/bin/sophia-verify-watchdog"
install -m 755 tools/verify_sophia_native_chrome.sh \
    "$artifact/bin/sophia-verify-native-chrome-core"
install -m 755 tools/verify_installed_native_chrome_session.sh \
    "$artifact/bin/sophia-verify-native-chrome-session"
install -m 755 tools/verify_installed_native_chrome_archive.sh \
    "$artifact/bin/sophia-verify-native-chrome"
install -m 755 tools/verify_sophia_firefox_physical.sh \
    "$artifact/bin/sophia-verify-firefox-run"
install -m 755 tools/record_sophia_firefox_physical_run.sh \
    "$artifact/bin/sophia-record-firefox-run"
install -m 755 tools/verify_sophia_firefox_physical_runs.sh \
    "$artifact/bin/sophia-verify-firefox-runs"
install -m 755 tools/run_sophia_session.sh \
    tools/stop_sophia_session.sh \
    tools/start_sophia_native_hot_reload_tty3.sh "$artifact/tools/"
install -m 755 tools/verify_packaged_policy.sh \
    "$artifact/tools/verify_packaged_policy.sh"
install -d -m 755 "$artifact/tools/config"
install -m 755 tools/probes/uinput_text_injector.py \
    "$artifact/tools/probes/uinput_text_injector.py"
install -m 644 tools/config/proof_helpers.sh \
    "$artifact/tools/config/proof_helpers.sh"
install -m 644 tools/config/99-sophia-uinput.rules \
    "$artifact/tools/config/99-sophia-uinput.rules"
install -m 644 tools/config/sophia-uinput.conf \
    "$artifact/tools/config/sophia-uinput.conf"
install -m 644 tools/lib/session_lifecycle.sh \
    "$artifact/tools/lib/session_lifecycle.sh"
install -m 644 tools/lib/session_terminal.sh \
    "$artifact/tools/lib/session_terminal.sh"
install -m 644 tools/lib/session_preparation.sh \
    "$artifact/tools/lib/session_preparation.sh"
install -m 644 tools/lib/installed_attempt_ledger.sh \
    "$artifact/tools/lib/installed_attempt_ledger.sh"
install -m 644 tools/lib/installed_hagia_evidence.sh \
    "$artifact/tools/lib/installed_hagia_evidence.sh"
install -m 644 tools/lib/live_session_surface.sh \
    "$artifact/tools/lib/live_session_surface.sh"
install -m 644 tools/lib/verify_firefox_rendering.awk \
    "$artifact/tools/lib/verify_firefox_rendering.awk"
install -m 755 tools/verify_sophia_firefox_rendering_physical.sh \
    "$artifact/tools/verify_sophia_firefox_rendering_physical.sh"
install -m 755 tools/sophia_tty_mode.py "$artifact/tools/sophia_tty_mode.py"
install -m 644 tools/fixtures/firefox_m8_local_page.html \
    "$artifact/tools/fixtures/firefox_m8_local_page.html"
install -m 755 tools/fixtures/firefox_m10_kitty_probe.sh \
    "$artifact/tools/fixtures/firefox_m10_kitty_probe.sh"
install -m 755 tools/fixtures/firefox_m10_selection_kitty_probe.sh \
    "$artifact/tools/fixtures/firefox_m10_selection_kitty_probe.sh"
install -m 755 tools/fixtures/truecolor_kitty_probe.sh \
    "$artifact/tools/fixtures/truecolor_kitty_probe.sh"
install -m 644 docs/operations.md "$artifact/share/doc/sophia/operations.md"

if [[ -n "$hagia_bin" ]]; then
    printf '%s\n' \
        '[Desktop Entry]' \
        'Name=Sophia Hagia (Native Policy)' \
        'Comment=Bounded Sophia native public-policy profile' \
        'Exec=@SOPHIA_INSTALL_PREFIX@/current/bin/sophia-hagia-session' \
        'Type=Application' \
        'DesktopNames=Sophia' \
        >"$artifact/share/wayland-sessions/sophia-hagia.desktop"
    printf '%s\n' \
        '[Desktop Entry]' \
        'Name=Sophia Hagia Promotion (Packaged Default)' \
        'Comment=Immutable Hagia packaged-default promotion profile' \
        'Exec=@SOPHIA_INSTALL_PREFIX@/current/bin/sophia-hagia-promotion-session' \
        'Type=Application' \
        'DesktopNames=Sophia' \
        >"$artifact/share/wayland-sessions/sophia-hagia-promotion.desktop"
    printf '%s\n' \
        '[Desktop Entry]' \
        'Name=Sophia Hagia (XTEST automation)' \
        'Comment=Hagia with synthetic input admitted; drives scenarios, accepts none' \
        'Exec=@SOPHIA_INSTALL_PREFIX@/current/bin/sophia-hagia-xtest-session' \
        'Type=Application' \
        'DesktopNames=Sophia' \
        >"$artifact/share/wayland-sessions/sophia-hagia-xtest.desktop"
fi
printf '%s\n' \
    '[Desktop Entry]' \
    'Name=Sophia Kitty (Baseline)' \
    'Comment=Sophia proven Kitty-only physical input baseline' \
    'Exec=@SOPHIA_INSTALL_PREFIX@/current/bin/sophia-kitty-session' \
    'Type=Application' \
    'DesktopNames=Sophia' \
    >"$artifact/share/wayland-sessions/sophia-kitty.desktop"
printf '%s\n' \
    '[Desktop Entry]' \
    'Name=Sophia Firefox Proof' \
    'Comment=Sophia installed physical Firefox promotion workflow' \
    'Exec=@SOPHIA_INSTALL_PREFIX@/current/bin/sophia-firefox-proof' \
    'Type=Application' \
    'DesktopNames=Sophia' \
    >"$artifact/share/wayland-sessions/sophia-firefox-proof.desktop"
printf '%s\n' \
    '[Desktop Entry]' \
    'Name=Sophia Recovery Proof' \
    'Comment=Bounded installed session and automatic display-manager recovery' \
    'Exec=@SOPHIA_INSTALL_PREFIX@/current/bin/sophia-recovery-proof' \
    'Type=Application' \
    'DesktopNames=Sophia' \
    >"$artifact/share/wayland-sessions/sophia-recovery-proof.desktop"
printf '%s\n' \
    '[Desktop Entry]' \
    'Name=Sophia Native Chrome Proof' \
    'Comment=Installed ring, frame, and combined chrome proof' \
    'Exec=@SOPHIA_INSTALL_PREFIX@/current/bin/sophia-native-chrome-proof' \
    'Type=Application' \
    'DesktopNames=Sophia' \
    >"$artifact/share/wayland-sessions/sophia-native-chrome-proof.desktop"
printf 'schema=6\nversion=%s\ncommit=%s\nrelease_id=%s\nbuilt_at_utc=%s\n' \
    "$version" "$commit" "$release_id" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    >"$artifact/manifest"
if [[ -n "$hagia_bin" ]]; then
    printf 'hagia_included=true\nhagia_source_commit=%s\nhagia_default_profile_sha256=%s\nhagia_binary_sha256=%s\nhagia_shell_binary_sha256=%s\n' \
        "$hagia_source_commit" "$hagia_default_profile_sha256" \
        "$(sha256sum "$hagia_bin" | awk '{print $1}')" \
        "$(sha256sum "$hagia_shell_bin" | awk '{print $1}')" \
        >>"$artifact/manifest"
else
    printf 'hagia_included=false\n' >>"$artifact/manifest"
fi
"$artifact/tools/verify_packaged_policy.sh" "$artifact"
(
    cd "$artifact"
    find bin target tools share -type f -print0 |
        sort -z |
        xargs -0 sha256sum >SHA256SUMS
)

echo "Packaged immutable Sophia release: $artifact"
echo "Install with: tools/install_live_session.sh"

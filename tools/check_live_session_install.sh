#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMP_DIR="$(mktemp -d)"
stop_pid=""
cleanup() {
    [[ -z "$stop_pid" ]] || kill "$stop_pid" 2>/dev/null || true
    rm -rf -- "$TEMP_DIR"
}
trap cleanup EXIT

# shellcheck source=tools/lib/live_session_surface.sh
source "$ROOT_DIR/tools/lib/live_session_surface.sh"

make_executable() {
    local path="$1"
    printf '#!/usr/bin/env bash\nexit 0\n' >"$path"
    chmod 755 "$path"
}

make_artifact() {
    local release_id="$1" with_hagia="$2" artifact command desktop
    local hagia_digest narthex_digest profile_digest
    artifact="$TEMP_DIR/artifact-$release_id"
    install -d -m 755 \
        "$artifact/bin" \
        "$artifact/share/doc/sophia" \
        "$artifact/share/wayland-sessions" \
        "$artifact/target/release" \
        "$artifact/tools/lib"
    install -m 644 "$ROOT_DIR/docs/operations.md" \
        "$artifact/share/doc/sophia/operations.md"
    install -m 644 "$ROOT_DIR/tools/lib/live_session_surface.sh" \
        "$artifact/tools/lib/live_session_surface.sh"
    install -m 755 "$ROOT_DIR/tools/verify_packaged_policy.sh" \
        "$artifact/tools/verify_packaged_policy.sh"

    make_executable "$artifact/target/release/sophia"
    make_executable "$artifact/target/release/sophia-wm-demo"
    if [[ "$with_hagia" == true ]]; then
        install -d -m 755 "$artifact/share/sophia-policy/hagia"
        make_executable "$artifact/target/release/hagia"
        make_executable "$artifact/target/release/narthex"
        printf 'schema 1\nshell { enabled #true; panel 28; }\n' \
            >"$artifact/share/sophia-policy/hagia/default.kdl"
        hagia_digest="$(sha256sum "$artifact/target/release/hagia" | awk '{print $1}')"
        narthex_digest="$(sha256sum "$artifact/target/release/narthex" | awk '{print $1}')"
        profile_digest="$(sha256sum "$artifact/share/sophia-policy/hagia/default.kdl" | awk '{print $1}')"
        printf 'schema=6\nversion=0.1.0\ncommit=%040d\nrelease_id=%s\nbuilt_at_utc=2026-09-04T00:00:00Z\nhagia_included=true\nhagia_source_commit=%040d\nhagia_default_profile_sha256=%s\nhagia_binary_sha256=%s\nhagia_shell_binary_sha256=%s\n' \
            "$release_id" "$release_id" 1 "$profile_digest" "$hagia_digest" \
            "$narthex_digest" >"$artifact/manifest"
    else
        printf 'schema=6\nversion=0.1.0\ncommit=%040d\nrelease_id=%s\nbuilt_at_utc=2026-09-04T00:00:00Z\nhagia_included=false\n' \
            "$release_id" "$release_id" >"$artifact/manifest"
    fi

    sophia_surface_select_entries "$artifact"
    for command in "${SOPHIA_SURFACE_COMMANDS[@]}"; do
        case "$command" in
            sophia-status)
                install -m 755 "$ROOT_DIR/tools/status_live_session.sh" \
                    "$artifact/bin/$command"
                ;;
            sophia-rollback)
                install -m 755 "$ROOT_DIR/tools/rollback_live_session.sh" \
                    "$artifact/bin/$command"
                ;;
            sophia-stop)
                install -m 755 "$ROOT_DIR/tools/installed/sophia-stop" \
                    "$artifact/bin/$command"
                ;;
            *) make_executable "$artifact/bin/$command" ;;
        esac
    done
    install -m 755 "$ROOT_DIR/tools/stop_sophia_session.sh" \
        "$artifact/tools/stop_sophia_session.sh"
    # A packaged release carries every entry unconditionally; only activation
    # selects which reach the login menu. Build the artifact the same way, so
    # the proof entries exist to be installed or withheld.
    for desktop in "${SOPHIA_SURFACE_DESKTOPS[@]}" \
        "${SOPHIA_SURFACE_PROOF_DESKTOPS[@]}"; do
        case "$desktop" in
            sophia-hagia) command=sophia-hagia-session ;;
            sophia-hagia-promotion) command=sophia-hagia-promotion-session ;;
            sophia-kitty) command=sophia-kitty-session ;;
            sophia-firefox-proof) command=sophia-firefox-proof ;;
            sophia-recovery-proof) command=sophia-recovery-proof ;;
            sophia-native-chrome-proof) command=sophia-native-chrome-proof ;;
            *) echo "test does not map desktop $desktop" >&2; return 1 ;;
        esac
        printf '[Desktop Entry]\nExec=@SOPHIA_INSTALL_PREFIX@/current/bin/%s\n' \
            "$command" >"$artifact/share/wayland-sessions/$desktop.desktop"
    done
    (
        cd "$artifact"
        find bin share target tools -type f -print0 | sort -z | \
            xargs -0 sha256sum >SHA256SUMS
    )
    "$ROOT_DIR/tools/verify_packaged_policy.sh" "$artifact" >/dev/null
    printf '%s\n' "$artifact"
}

expect_policy_rejection() {
    local artifact="$1" label="$2"
    if "$ROOT_DIR/tools/verify_packaged_policy.sh" "$artifact" >/dev/null 2>&1; then
        echo "packaged policy verifier accepted $label" >&2
        exit 1
    fi
}

first="$(umask 077; make_artifact 0001 false)"
second="$(make_artifact 0002 false)"
hagia_artifact="$(make_artifact 0003 true)"

invalid_bridge="$TEMP_DIR/invalid-bridge"
cp -a "$first" "$invalid_bridge"
make_executable "$invalid_bridge/target/release/sophia-x11-wm-bridge"
expect_policy_rejection "$invalid_bridge" "a removed legacy-WM bridge"

invalid_legacy_field="$TEMP_DIR/invalid-legacy-field"
cp -a "$first" "$invalid_legacy_field"
printf 'xmonad_binary_sha256=%064d\n' 0 >>"$invalid_legacy_field/manifest"
expect_policy_rejection "$invalid_legacy_field" "a legacy policy manifest field"

invalid_narthex="$TEMP_DIR/invalid-narthex"
cp -a "$hagia_artifact" "$invalid_narthex"
chmod 644 "$invalid_narthex/target/release/narthex"
expect_policy_rejection "$invalid_narthex" "a non-executable Narthex binary"

PREFIX="$TEMP_DIR/install/prefix"
SESSION_DIR="$TEMP_DIR/share/wayland-sessions"
COMMAND_DIR="$TEMP_DIR/commands"
install_env=(
    SOPHIA_INSTALL_PREFIX="$PREFIX"
    SOPHIA_SESSION_DIR="$SESSION_DIR"
    SOPHIA_COMMAND_DIR="$COMMAND_DIR"
)

env "${install_env[@]}" "$ROOT_DIR/tools/install_live_session.sh" "$first"
[[ "$(readlink "$PREFIX/current")" == releases/0001 ]]
for public_file in "$PREFIX/current/manifest" "$PREFIX/current/SHA256SUMS" \
    "$PREFIX/current"/share/wayland-sessions/*.desktop; do
    [[ "$(stat -c %a "$public_file")" == 644 ]] || {
        echo "Installed public release metadata retained a private umask: $public_file" >&2
        exit 1
    }
done
[[ ! -e "$PREFIX/previous" ]]
[[ "$(readlink "$COMMAND_DIR/sophia")" == "$PREFIX/current/target/release/sophia" ]]
[[ "$(readlink -f "$COMMAND_DIR/sophia")" == "$PREFIX/releases/0001/target/release/sophia" ]]
"$COMMAND_DIR/sophia" session list
[[ -f "$SESSION_DIR/sophia-kitty.desktop" ]]
# The diagnostic ships in the release and keeps its operator command, but a
# default install leaves it out of the login menu.
[[ ! -e "$SESSION_DIR/sophia-native-chrome-proof.desktop" ]]
[[ -x "$PREFIX/current/bin/sophia-native-chrome-proof" ]]
[[ ! -e "$SESSION_DIR/sophia.desktop" ]]
[[ ! -e "$COMMAND_DIR/sophia-verify-xmonad-run" ]]

# Activation removes only stale Sophia-owned compatibility entries. Foreign
# files with the same names remain operator-owned.
printf '[Desktop Entry]\nExec=%s/current/bin/sophia-session\n' "$PREFIX" \
    >"$SESSION_DIR/sophia.desktop"
ln -s "$PREFIX/current/bin/sophia-verify-xmonad-run" \
    "$COMMAND_DIR/sophia-verify-xmonad-run"
printf '[Desktop Entry]\nExec=/foreign-hagia-session\n' \
    >"$SESSION_DIR/sophia-hagia.desktop"
ln -s /foreign-hagia-command "$COMMAND_DIR/sophia-hagia-session"

env "${install_env[@]}" "$ROOT_DIR/tools/install_live_session.sh" "$second"
[[ "$(readlink "$PREFIX/current")" == releases/0002 ]]
[[ "$(readlink -f "$COMMAND_DIR/sophia")" == "$PREFIX/releases/0002/target/release/sophia" ]]
[[ "$(readlink "$PREFIX/previous")" == releases/0001 ]]
[[ ! -e "$SESSION_DIR/sophia.desktop" ]]
[[ ! -e "$COMMAND_DIR/sophia-verify-xmonad-run" ]]
grep -Fqx 'Exec=/foreign-hagia-session' "$SESSION_DIR/sophia-hagia.desktop"
[[ "$(readlink "$COMMAND_DIR/sophia-hagia-session")" == /foreign-hagia-command ]]

env "${install_env[@]}" "$COMMAND_DIR/sophia-rollback"
[[ "$(readlink "$PREFIX/current")" == releases/0001 ]]
[[ "$(readlink "$PREFIX/previous")" == releases/0002 ]]
[[ "$(readlink -f "$COMMAND_DIR/sophia")" == "$PREFIX/releases/0001/target/release/sophia" ]]

hagia_prefix="$TEMP_DIR/hagia/prefix"
hagia_sessions="$TEMP_DIR/hagia/sessions"
hagia_commands="$TEMP_DIR/hagia/commands"
hagia_env=(
    SOPHIA_INSTALL_PREFIX="$hagia_prefix"
    SOPHIA_SESSION_DIR="$hagia_sessions"
    SOPHIA_COMMAND_DIR="$hagia_commands"
)
env "${hagia_env[@]}" "$ROOT_DIR/tools/install_live_session.sh" "$first"
env "${hagia_env[@]}" "$ROOT_DIR/tools/install_live_session.sh" "$hagia_artifact"
[[ "$(readlink "$hagia_prefix/current")" == releases/0003 ]]
# The ordinary user-profile session is listed; the evidence and proof profiles
# are not, though their commands are installed either way.
[[ -f "$hagia_sessions/sophia-hagia.desktop" ]]
for desktop in sophia-hagia-promotion sophia-firefox-proof sophia-recovery-proof; do
    [[ ! -e "$hagia_sessions/$desktop.desktop" ]]
done
for command in sophia-hagia-session sophia-hagia-promotion-session \
    sophia-record-hagia-run sophia-verify-hagia sophia-verify-hagia-promotion; do
    [[ "$(readlink "$hagia_commands/$command")" == \
        "$hagia_prefix/current/bin/$command" ]]
done

env "${hagia_env[@]}" "$hagia_commands/sophia-rollback"
[[ "$(readlink "$hagia_prefix/current")" == releases/0001 ]]
for desktop in sophia-hagia sophia-hagia-promotion sophia-firefox-proof sophia-recovery-proof; do
    [[ ! -e "$hagia_sessions/$desktop.desktop" ]]
done
[[ ! -e "$hagia_commands/sophia-hagia-session" ]]

env "${hagia_env[@]}" "$hagia_commands/sophia-rollback"
[[ "$(readlink "$hagia_prefix/current")" == releases/0003 ]]
[[ -f "$hagia_sessions/sophia-hagia.desktop" ]]

# Proof entries are opt-in, and withdrawing the request retires them again.
proof_prefix="$TEMP_DIR/proof/prefix"
proof_sessions="$TEMP_DIR/proof/sessions"
proof_commands="$TEMP_DIR/proof/commands"
proof_env=(
    SOPHIA_INSTALL_PREFIX="$proof_prefix"
    SOPHIA_SESSION_DIR="$proof_sessions"
    SOPHIA_COMMAND_DIR="$proof_commands"
)
env "${proof_env[@]}" SOPHIA_INSTALL_PROOF_SESSIONS=1 \
    "$ROOT_DIR/tools/install_live_session.sh" "$hagia_artifact"
for desktop in sophia-kitty sophia-hagia sophia-hagia-promotion \
    sophia-firefox-proof sophia-recovery-proof sophia-native-chrome-proof; do
    [[ -f "$proof_sessions/$desktop.desktop" ]]
done
# A foreign file at a proof entry's path is operator-owned and must survive
# retirement, exactly as it does for the compatibility entries above.
printf '[Desktop Entry]\nExec=/foreign-recovery\n' \
    >"$proof_sessions/sophia-recovery-proof.desktop"
env "${proof_env[@]}" "$ROOT_DIR/tools/activate_live_session_release.sh" \
    "$proof_prefix/releases/0003"
for desktop in sophia-hagia-promotion sophia-firefox-proof \
    sophia-native-chrome-proof; do
    [[ ! -e "$proof_sessions/$desktop.desktop" ]]
done
grep -Fqx 'Exec=/foreign-recovery' "$proof_sessions/sophia-recovery-proof.desktop"
[[ -f "$proof_sessions/sophia-hagia.desktop" ]]
[[ -f "$proof_sessions/sophia-kitty.desktop" ]]

if env "${hagia_env[@]}" "$ROOT_DIR/tools/activate_live_session_release.sh" \
    "$hagia_artifact" >/dev/null 2>&1; then
    echo "activation accepted an artifact outside the immutable install prefix" >&2
    exit 1
fi

stop_runtime="$TEMP_DIR/stop-runtime"
stop_state="$stop_runtime/sophia-hagia-session-$UID"
install -d -m 700 "$stop_state"
sleep 60 &
stop_pid=$!
printf '%s\n' "$stop_pid" >"$stop_state/wrapper.pid"
env XDG_RUNTIME_DIR="$stop_runtime" "$hagia_commands/sophia-stop"
if kill -0 "$stop_pid" 2>/dev/null; then
    echo "installed Sophia stop command left the Hagia wrapper running" >&2
    exit 1
fi
wait "$stop_pid" 2>/dev/null || true
stop_pid=""

# Ordinary logins default to automatic arming; proofs retain a physical
# handshake even when the environment requests automatic arming.
arming_release="$TEMP_DIR/arming-release"
arming_capture="$TEMP_DIR/arming-observed"
install -d -m 755 "$arming_release/bin" "$arming_release/target/release" \
    "$arming_release/tools/lib"
install -m 644 "$ROOT_DIR/tools/lib/session_lifecycle.sh" \
    "$arming_release/tools/lib/session_lifecycle.sh"
install -m 755 "$ROOT_DIR/tools/installed/sophia-session" \
    "$arming_release/bin/sophia-session"
make_executable "$arming_release/target/release/sophia-wm-demo"
make_executable "$arming_release/target/release/hagia"
make_executable "$arming_release/target/release/narthex"
make_executable "$arming_release/bin/capture-runtime-identity"
printf 'version=0.1.0\ncommit=%040d\n' 0 >"$arming_release/manifest"
# Capture both exits: the ordinary supervisor and the direct proof runner.
for entry in target/release/sophia tools/run_sophia_session.sh; do
    printf '#!/usr/bin/env bash\nprintf "%%s\\n" "${SOPHIA_INPUT_GUARD_ARMING-unset}" >"$SOPHIA_ARMING_CAPTURE"\nexit 0\n' \
        >"$arming_release/$entry"
    chmod 755 "$arming_release/$entry"
done
printf '#!/usr/bin/env bash\nif [[ "${1:-}" == begin ]]; then mktemp -d "${SOPHIA_ARMING_CAPTURE}.attempt.XXXXXX"; fi\nexit 0\n' \
    >"$arming_release/bin/sophia-record-hagia-run"
chmod 755 "$arming_release/bin/sophia-record-hagia-run"

arming_profile_mode=explicit
arming_attempt_mode=hagia
arming_extra_env=()
observed_arming() {
    local ambient="$1"
    shift
    rm -f "$arming_capture"
    local -a fixture_env=(
        PATH="$PATH"
        HOME="$TEMP_DIR/arming-home"
        XDG_STATE_HOME="$TEMP_DIR/arming-state"
        SOPHIA_ARMING_CAPTURE="$arming_capture"
        SOPHIA_TTY_PROFILE=hagia
        SOPHIA_INSTALLED_ATTEMPT_MODE="$arming_attempt_mode"
        SOPHIA_HAGIA_PROFILE_MODE="$arming_profile_mode"
        SOPHIA_DESKTOP_PROFILE_SHA256=0000000000000000000000000000000000000000000000000000000000000000
    )
    fixture_env+=("${arming_extra_env[@]}")
    if [[ "$ambient" != - ]]; then
        fixture_env+=(SOPHIA_INPUT_GUARD_ARMING="$ambient")
    fi
    env -i "${fixture_env[@]}" "$arming_release/bin/sophia-session" "$@" \
        >"$TEMP_DIR/arming-launch.log" 2>&1 || true
    if [[ -s "$arming_capture" ]]; then
        cat "$arming_capture"
    else
        printf 'never-reached-the-session\n'
    fi
}

arming_expect() {
    local expected="$1" observed="$2" description="$3"
    if [[ "$observed" != "$expected" ]]; then
        echo "installed launcher arming: $description expected $expected, observed $observed" >&2
        exit 1
    fi
}

# An ordinary desktop arms itself.
arming_expect automatic "$(observed_arming -)" "ordinary session with no ambient arming"
# An explicit request survives the default, so a development session on an
# ordinary profile can still ask for the handshake.
arming_expect manual "$(observed_arming manual)" "ordinary session with manual requested"
# A proof run keeps the handshake even when the environment asks for
# automatic, so ambient automatic cannot skip the rehearsal.
arming_expect manual "$(observed_arming automatic --proof)" "proof run under ambient automatic"
arming_expect manual "$(observed_arming automatic --max-runtime-ms=1000)" \
    "bounded run under ambient automatic"
# A promotion session is a proof by profile rather than by argument.
arming_profile_mode=packaged-promotion
arming_expect manual "$(observed_arming automatic)" "packaged promotion under ambient automatic"
arming_profile_mode=explicit
arming_attempt_mode=hagia-promotion
arming_expect manual "$(observed_arming automatic)" "promotion attempt under ambient automatic"
arming_attempt_mode=hagia
# Some runs select the proof path by environment rather than by argument.
# Diagnostics and arming have to agree on which path that is, or a run would
# collect proof evidence while arming itself.
arming_extra_env=(SOPHIA_TRUECOLOR_PROOF=true)
arming_expect manual "$(observed_arming automatic)" \
    "truecolor proof by environment under ambient automatic"
arming_extra_env=(SOPHIA_SESSION_WATCHDOG_SECONDS=30)
arming_expect manual "$(observed_arming automatic)" \
    "watchdog-bounded run by environment under ambient automatic"
# A false value is not a proof request and must not cost a desktop its default.
arming_extra_env=(SOPHIA_TRUECOLOR_PROOF=false)
arming_expect automatic "$(observed_arming -)" \
    "ordinary session with truecolor proof explicitly disabled"
arming_extra_env=()

echo "native-only live-session install, activation, and rollback checks passed"

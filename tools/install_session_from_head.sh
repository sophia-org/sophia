#!/usr/bin/env bash
# Package the current commit and install it as the live session.
#
# This exists because the alternative is a throwaway script with a commit
# hash typed into it, which is correct for exactly as long as that commit is
# HEAD. Everything here is derived: the release id comes from the worktree,
# and the only thing an operator supplies is a password.
#
# It is idempotent. Installing what is already installed reports that and
# stops; packaging what is already packaged reuses the artifact. Rerunning
# after a failed step resumes rather than starting over.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

hagia_root="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
narthex_root="${SOPHIA_NARTHEX_ROOT:-$ROOT_DIR/../narthex}"
hagia_bin="${SOPHIA_HAGIA_BIN:-$hagia_root/hagia}"
narthex_bin="${SOPHIA_HAGIA_SHELL_BIN:-$narthex_root/narthex}"

commit="$(git rev-parse HEAD)"
version="$(awk -F'"' '$1 ~ /^version = / { print $2; exit }' Cargo.toml)"
release_id="${version}-${commit:0:12}"
artifact="$ROOT_DIR/.artifacts/sophia-$release_id"
installed="$(sed -n 's/^commit=//p' /opt/sophia/current/manifest 2>/dev/null || true)"

if [[ "$installed" == "$commit" ]]; then
    printf 'Already installed: %s\n' "$release_id"
    exit 0
fi

# Packaging refuses a dirty worktree, and it is right to: a release named
# after a commit must contain that commit and nothing else. Say so before
# spending a build on it.
if [[ -n "$(git status --short)" ]]; then
    echo "Refusing to package a dirty worktree. Commit or stash first:" >&2
    git status --short >&2
    exit 1
fi

if [[ ! -d "$artifact" ]]; then
    # The policy clients are built from their own repositories, so a stale
    # binary here would be packaged silently under this commit's name.
    for pair in "hagia:$hagia_root:$hagia_bin" "narthex:$narthex_root:$narthex_bin"; do
        name="${pair%%:*}"
        rest="${pair#*:}"
        root="${rest%%:*}"
        binary="${rest#*:}"
        if [[ ! -x "$binary" ]]; then
            [[ -d "$root" ]] || {
                echo "Cannot build $name: $root is not a directory." >&2
                echo "Set SOPHIA_${name^^}_ROOT or build $binary yourself." >&2
                exit 1
            }
            printf 'Building %s from %s\n' "$name" "$root"
            (cd "$root" && nimble build -d:release)
        fi
        [[ -x "$binary" ]] || {
            echo "$name did not produce an executable at $binary" >&2
            exit 1
        }
    done
    SOPHIA_HAGIA_BIN="$hagia_bin" SOPHIA_HAGIA_SHELL_BIN="$narthex_bin" \
        "$ROOT_DIR/tools/package_live_session.sh"
fi

(cd "$artifact" && sha256sum --check SHA256SUMS) >/dev/null
"$artifact/tools/verify_packaged_policy.sh" "$artifact"

# Activation is the cheaper path when the release directory already exists
# under /opt; installation is what creates it. Both retain the release they
# replace as `previous`.
release_dir="/opt/sophia/releases/$release_id"
echo "Installing $release_id (sudo required)"
# sudo drops the environment, so the proof-session opt-in has to be handed
# across the privilege boundary by name rather than inherited.
proof_sessions="SOPHIA_INSTALL_PROOF_SESSIONS=${SOPHIA_INSTALL_PROOF_SESSIONS:-}"
if [[ -d "$release_dir" ]]; then
    sudo "$proof_sessions" \
        "$ROOT_DIR/tools/activate_live_session_release.sh" "$release_dir"
else
    sudo "$proof_sessions" \
        "$ROOT_DIR/tools/install_live_session.sh" "$artifact"
fi

# Verify what is selected rather than trusting that the installer said so.
[[ "$(sed -n 's/^commit=//p' /opt/sophia/current/manifest)" == "$commit" ]]
(cd /opt/sophia/current && sha256sum --check SHA256SUMS) >/dev/null

# Put the policy client where its owner can replace it. It is a blind client
# the Engine validates, so it does not need the release's immutability, and
# keeping it here is what lets `just reload-wm` swap it without a logout. It
# comes from the release that was just verified, so this is the same binary
# either way.
policy_bin="${XDG_STATE_HOME:-$HOME/.local/state}/sophia/bin/hagia"
mkdir -p "$(dirname "$policy_bin")"
install -m 700 /opt/sophia/current/target/release/hagia "$policy_bin"
printf 'Policy client installed at %s\n' "$policy_bin"

hagia_commit="$(sed -n 's/^hagia_source_commit=//p' /opt/sophia/current/manifest)"
printf '\n%s\n' "Installed Sophia ${commit:0:8} with Hagia ${hagia_commit:0:7}."
printf '%s\n' \
    'Start from greetd: Sophia Hagia (Native Policy).' \
    'Logout: Ctrl+Alt+Delete. Emergency: Ctrl+Alt+Backspace.' \
    'From another TTY: sophia-stop hagia.' \
    'Rollback after logout: sudo sophia-rollback.'
printf 'Previous release: %s\n' "$(readlink /opt/sophia/previous 2>/dev/null || echo none)"

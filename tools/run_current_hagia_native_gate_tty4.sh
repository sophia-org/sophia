#!/usr/bin/env bash
set -euo pipefail

# Binds the exact source and binary identity of a native Hagia session proof
# before any DRM takeover, then hands off to the gate. Both repositories must be
# clean and signed: the archive this run produces is only as good as the
# identity bound here.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=tools/lib/proof_checkout.sh
source "$ROOT_DIR/tools/lib/proof_checkout.sh"
HAGIA_ROOT="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
NARTHEX_ROOT="${SOPHIA_NARTHEX_ROOT:-$ROOT_DIR/../narthex}"

if [[ ! -t 0 || "$(tty)" != /dev/tty4 ]]; then
    echo "Switch to tty4 with Ctrl+Alt+F4, log in, and run:" >&2
    echo "  $ROOT_DIR/tools/run_current_hagia_native_gate_tty4.sh" >&2
    exit 1
fi
if ! proof_checkout_root "$HAGIA_ROOT"; then
    echo "Hagia checkout not found at $HAGIA_ROOT" >&2
    echo "Set SOPHIA_HAGIA_ROOT to its checkout path." >&2
    exit 1
fi
if ! proof_checkout_root "$NARTHEX_ROOT"; then
    echo "Narthex checkout not found at $NARTHEX_ROOT" >&2
    echo "Set SOPHIA_NARTHEX_ROOT to its checkout path." >&2
    exit 1
fi
if [[ -n "$(git -C "$ROOT_DIR" status --short)" ]]; then
    echo "Sophia worktree must be clean before the physical proof." >&2
    exit 1
fi
if [[ -n "$(git -C "$NARTHEX_ROOT" status --short)" ]]; then
    echo "Narthex worktree must be clean before the physical proof." >&2
    exit 1
fi
if [[ -n "$(git -C "$HAGIA_ROOT" status --short)" ]]; then
    echo "Hagia worktree must be clean before the physical proof." >&2
    exit 1
fi

sophia_commit="$(git -C "$ROOT_DIR" rev-parse HEAD)"
hagia_commit="$(git -C "$HAGIA_ROOT" rev-parse HEAD)"
narthex_commit="$(git -C "$NARTHEX_ROOT" rev-parse HEAD)"
for repo_and_commit in "$ROOT_DIR:$sophia_commit" "$HAGIA_ROOT:$hagia_commit" "$NARTHEX_ROOT:$narthex_commit"; do
    repo="${repo_and_commit%:*}"
    commit="${repo_and_commit##*:}"
    git -C "$repo" verify-commit "$commit" >/dev/null 2>&1 || {
        echo "Physical-proof HEAD lacks a valid signature: $repo" >&2
        echo "  Run tools/check_proof_preconditions.sh first to see all three." >&2
        exit 1
    }
done
# A signed commit on a clean tree is the whole identity requirement, which is
# the rule the direct-scanout gate already moved to. Demanding HEAD equal the
# locally known origin/master added a push to every commit-gate-run cycle
# without adding anything the archive binds to: the archive names the commit,
# the commit is signed, and re-verification checks both against these
# repositories, none of which involves a remote. Where a commit has been pushed
# is a publishing question, not an evidence one.
# Hagia's canonical default profile, unless a reference run names another one
# (t018). Either way it is chosen and checked before anything is built.
default_profile="$HAGIA_ROOT/examples/config/default.kdl"
desktop_profile="${SOPHIA_HAGIA_NATIVE_PROFILE:-$default_profile}"
proof_tracked_file "$desktop_profile" "$ROOT_DIR" "$HAGIA_ROOT" || {
    echo "The desktop profile must be an absolute, tracked, unmodified file of the Sophia or Hagia checkout: $desktop_profile" >&2
    exit 1
}
# A reference capture exists to observe a named reference profile; one that
# fell back to the canonical default would retain a capture of the wrong policy.
case "${SOPHIA_HAGIA_NATIVE_CAPTURE:-}" in
    '') ;;
    reference)
        if [[ -z "${SOPHIA_HAGIA_NATIVE_PROFILE:-}" \
            || "$(cd -- "$(dirname -- "$desktop_profile")" && pwd -P)/$(basename -- "$desktop_profile")" \
                == "$(cd -- "$(dirname -- "$default_profile")" && pwd -P)/$(basename -- "$default_profile")" ]]; then
            echo "A reference capture requires SOPHIA_HAGIA_NATIVE_PROFILE naming a non-default profile." >&2
            exit 1
        fi
        ;;
    *)
        echo "SOPHIA_HAGIA_NATIVE_CAPTURE must be unset or reference" >&2
        exit 1
        ;;
esac
hagia_bin="${TMPDIR:-/tmp}/hagia-native-${hagia_commit:0:12}"
hagia_shell_bin="${TMPDIR:-/tmp}/narthex-native-${narthex_commit:0:12}"
hagia_nimcache="${TMPDIR:-/tmp}/hagia-native-nimcache-${hagia_commit:0:12}"
hagia_shell_nimcache="${TMPDIR:-/tmp}/narthex-native-nimcache-${narthex_commit:0:12}"

echo "Building exact physical-proof binaries before DRM takeover..."
echo "Sophia: $sophia_commit"
echo "Hagia:  $hagia_commit"
echo "Narthex: $narthex_commit"
(
    cd "$HAGIA_ROOT"
    nim c -d:release --path:src --nimcache:"$hagia_nimcache" \
        -o:"$hagia_bin" src/hagia.nim
)
(
    cd "$NARTHEX_ROOT"
    nim c -d:release --path:src --nimcache:"$hagia_shell_nimcache" \
        -o:"$hagia_shell_bin" src/narthex.nim
)
(
    cd "$ROOT_DIR"
    cargo build --quiet --release --offline -p sophia-cli \
        --features native-session
)
"$hagia_bin" config check --config="$desktop_profile"
"$ROOT_DIR/target/release/sophia" config check \
    --desktop-profile="$desktop_profile"

if [[ -n "$(git -C "$ROOT_DIR" status --short)" \
    || -n "$(git -C "$HAGIA_ROOT" status --short)" \
    || -n "$(git -C "$NARTHEX_ROOT" status --short)" \
    || "$(git -C "$ROOT_DIR" rev-parse HEAD)" != "$sophia_commit" \
    || "$(git -C "$HAGIA_ROOT" rev-parse HEAD)" != "$hagia_commit" \
    || "$(git -C "$NARTHEX_ROOT" rev-parse HEAD)" != "$narthex_commit" ]]; then
    echo "Sophia, Hagia, or Narthex source identity changed during the physical-proof build." >&2
    exit 1
fi
git -C "$ROOT_DIR" verify-commit "$sophia_commit" >/dev/null 2>&1 || {
    echo "Sophia signature no longer verifies after the build." >&2
    exit 1
}
git -C "$HAGIA_ROOT" verify-commit "$hagia_commit" >/dev/null 2>&1 || {
    echo "Hagia signature no longer verifies after the build." >&2
    exit 1
}
git -C "$NARTHEX_ROOT" verify-commit "$narthex_commit" >/dev/null 2>&1 || {
    echo "Narthex signature no longer verifies after the build." >&2
    exit 1
}

sophia_bin="$ROOT_DIR/target/release/sophia"
sophia_sha256="$(sha256sum "$sophia_bin" | awk '{ print $1 }')"
hagia_sha256="$(sha256sum "$hagia_bin" | awk '{ print $1 }')"
hagia_shell_sha256="$(sha256sum "$hagia_shell_bin" | awk '{ print $1 }')"
profile_sha256="$(sha256sum "$desktop_profile" | awk '{ print $1 }')"
echo "Sophia binary:  $sophia_sha256"
echo "Hagia binary:   $hagia_sha256"
echo "Hagia Shell:    $hagia_shell_sha256"
echo "Desktop profile: $profile_sha256"

export SOPHIA_TTY_NUMBER=4
export SOPHIA_HAGIA_NATIVE_ARM=1
export SOPHIA_HAGIA_NATIVE_SEAT="${SOPHIA_HAGIA_NATIVE_SEAT:-seat0}"
export SOPHIA_HAGIA_BIN="$hagia_bin"
export SOPHIA_HAGIA_SHELL_BIN="$hagia_shell_bin"
# The profile is handed to the session explicitly and its digest is bound here,
# so the identity Sophia prints names the profile that actually ran.
export SOPHIA_DESKTOP_PROFILE="$desktop_profile"
export SOPHIA_DESKTOP_PROFILE_SHA256="$profile_sha256"
export SOPHIA_HAGIA_PROFILE_MODE=packaged-promotion
export SOPHIA_HAGIA_ROOT="$HAGIA_ROOT"
export SOPHIA_HAGIA_NATIVE_SOURCE_COMMIT="$sophia_commit"
export SOPHIA_HAGIA_NATIVE_HAGIA_COMMIT="$hagia_commit"
export SOPHIA_HAGIA_NATIVE_SOPHIA_SHA256="$sophia_sha256"
export SOPHIA_HAGIA_NATIVE_HAGIA_SHA256="$hagia_sha256"
export SOPHIA_HAGIA_NATIVE_NARTHEX_SHA256="$hagia_shell_sha256"
export SOPHIA_HAGIA_NATIVE_NARTHEX_COMMIT="$narthex_commit"
export SOPHIA_NARTHEX_ROOT="$NARTHEX_ROOT"
exec "$ROOT_DIR/tools/hagia_native_session_gate.sh"

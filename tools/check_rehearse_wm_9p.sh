#!/usr/bin/env bash
# Offline checks for tools/rehearse_wm_9p.sh: evidence preparation and every
# refusal. The live phases need a tty4 session and are rehearsal evidence.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
script="$ROOT_DIR/tools/rehearse_wm_9p.sh"
fixture="$(mktemp -d)"
trap 'rm -rf -- "$fixture"' EXIT
release="$fixture/release"
install -d -m 755 "$release/target/release" "$release/bin" \
    "$release/share/sophia-niltempus-desktop"
for binary in target/release/sophia target/release/hagia bin/sophia-hagia-session; do
    printf '#!/bin/sh\nexit 0\n' >"$release/$binary"
    chmod 755 "$release/$binary"
done
printf 'policy {\n    view-count 3\n}\nsession {\n    control host-admin\n}\n' \
    >"$release/share/sophia-niltempus-desktop/desktop.kdl"
printf '{"plan":{}}\n' >"$release/desktop-manifest.json"
(cd "$release" && find . -type f ! -name SHA256SUMS -printf '%P\n' | sort | xargs sha256sum) \
    >"$release/SHA256SUMS"
export XDG_STATE_HOME="$fixture/state"

# Dry run: evidence prepared from the sealed release; nothing launched.
evidence="$("$script" --release="$release" --dry-run </dev/null)"
[[ -d "$evidence" && "$(stat -c %a "$evidence")" == 700 ]]
cmp "$evidence/desktop-valid.kdl" "$release/share/sophia-niltempus-desktop/desktop.kdl"
cmp "$evidence/desktop.kdl" "$evidence/desktop-valid.kdl"
grep -qx '    view-count 10' "$evidence/desktop-rejected.kdl"
[[ "$(diff "$evidence/desktop-valid.kdl" "$evidence/desktop-rejected.kdl" | grep -c '^[<>]')" == 2 ]]
grep -q 'target/release/hagia' "$evidence/binaries.sha256"
grep -qx 'wire=9p2000.L' "$evidence/candidate.txt"
[[ "$evidence" == *-9p2000.L-* ]]

# Refusals: an unknown wire, a relative release, a live run off tty4, and a
# release that no longer matches its seal.
set +e
"$script" --wire=9p --release="$release" --dry-run </dev/null >/dev/null 2>&1
[[ $? == 2 ]] || { echo "unknown wire accepted" >&2; exit 1; }
"$script" --release=relative --dry-run </dev/null >/dev/null 2>&1
[[ $? == 2 ]] || { echo "relative release accepted" >&2; exit 1; }
"$script" --release="$release" </dev/null >/dev/null 2>&1
[[ $? == 1 ]] || { echo "ran off tty4" >&2; exit 1; }
printf 'tampered\n' >>"$release/target/release/hagia"
"$script" --release="$release" --dry-run </dev/null >/dev/null 2>&1
[[ $? == 1 ]] || { echo "tampered release accepted" >&2; exit 1; }
set -e
echo "rehearse_wm_9p offline checks passed"

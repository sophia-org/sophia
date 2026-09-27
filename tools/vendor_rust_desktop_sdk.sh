#!/bin/sh
# Replace vendor/rust-desktop-sdk with the exact signed SDK revision given:
# the source tree from `git archive`, a sorted SHA-256 manifest, and the raw
# commit object that binds the revision to that tree. Offline; it never
# fetches. Then run `cargo xtask check rust-desktop-sdk`.
set -eu
if [ "$#" -ne 2 ]; then
    echo "usage: $0 <sdk checkout> <revision>" >&2
    exit 2
fi
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
sdk=$1
revision=$(git -C "$sdk" rev-parse --verify "$2^{commit}")
git -C "$sdk" verify-commit "$revision"
snapshot="$root/vendor/rust-desktop-sdk"
rm -rf "$snapshot/source"
mkdir -p "$snapshot/source"
git -C "$sdk" archive "$revision" | tar -x -C "$snapshot/source"
git -C "$sdk" cat-file commit "$revision" > "$snapshot/upstream.commit"
python3 -B - "$snapshot" "$revision" <<'PY'
import hashlib, json, os, sys
snapshot, revision = sys.argv[1], sys.argv[2]
source = os.path.join(snapshot, "source")
files = {}
for directory, _, names in os.walk(source):
    for name in names:
        path = os.path.join(directory, name)
        with open(path, "rb") as handle:
            files[os.path.relpath(path, source)] = hashlib.sha256(handle.read()).hexdigest()
manifest = {
    "schema": 1,
    "repository": "https://github.com/sophia-org/sophia-desktop-sdk-rs",
    "revision": revision,
    "files": dict(sorted(files.items())),
}
with open(os.path.join(snapshot, "manifest.json"), "w") as handle:
    handle.write(json.dumps(manifest, indent=2) + "\n")
PY
printf '%s\n' "rust_desktop_sdk vendored revision=$revision"

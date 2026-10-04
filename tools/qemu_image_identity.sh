#!/usr/bin/env bash
# Asserts that a guest image holds exactly the expected bytes at each expected
# path. EXPECTED is a sha256sum-style file of "<sha256>  <guest path>" lines
# (guest paths relative to the image root, e.g. usr/bin/sophia). The listed
# paths are unpacked from IMAGE in one decompression into a private
# directory; each must exist as a regular file and match. The image's own
# digests are written to OUT, and any missing or different file fails.
#
#   tools/qemu_image_identity.sh IMAGE EXPECTED OUT
set -euo pipefail

IMAGE="$(realpath "${1:?image}")"
EXPECTED="$(realpath "${2:?expected digests}")"
OUT="${3:?output digests}"

mapfile -t entries < "$EXPECTED"
[[ ${#entries[@]} -gt 0 ]] || { echo "no expected image paths" >&2; exit 1; }
paths=()
for entry in "${entries[@]}"; do
    [[ "$entry" =~ ^([0-9a-f]{64})\ \ ([A-Za-z0-9._/-]+)$ ]] \
        || { echo "malformed expected entry: $entry" >&2; exit 1; }
    [[ "${BASH_REMATCH[2]}" != /* && "${BASH_REMATCH[2]}" != *..* ]] \
        || { echo "unsafe guest path: ${BASH_REMATCH[2]}" >&2; exit 1; }
    paths+=("${BASH_REMATCH[2]}")
done

unpacked="$(mktemp -d)"
trap 'rm -rf "$unpacked"' EXIT
# lsinitrd skips a requested path the image lacks, so every path is checked below.
(cd "$unpacked" && lsinitrd --unpack "$IMAGE" "${paths[@]}")

status=0
: > "$OUT"
for entry in "${entries[@]}"; do
    digest="${entry%%  *}"
    path="${entry#*  }"
    if [[ ! -f "$unpacked/$path" || -L "$unpacked/$path" ]]; then
        echo "image lacks regular file $path" >&2
        echo "missing  $path" >> "$OUT"
        status=1
        continue
    fi
    actual="$(sha256sum "$unpacked/$path" | cut -d' ' -f1)"
    echo "$actual  $path" >> "$OUT"
    if [[ "$actual" != "$digest" ]]; then
        echo "image $path is $actual, expected $digest" >&2
        status=1
    fi
done
exit "$status"

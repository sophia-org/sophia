#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
build=$(mktemp -d)
trap 'rm -rf "$build"' EXIT HUP INT TERM
cd "$root"
ulimit -c 0
python3 -B tools/check_shell_c_wire_inventory.py
for test in test corpus budget_test catalog_test native_test native_codec_test resource_test; do
    if [ "$test" = budget_test ]; then
        set -- -Wl,--wrap=recv -Wl,--wrap=send
    else
        set --
    fi
    "${CC:-cc}" -std=c99 -Wall -Wextra -Werror -pedantic \
        bindings/c/shell_wire/frame.c bindings/c/shell_wire/io.c \
        bindings/c/shell_wire/negotiation.c bindings/c/shell_wire/catalog.c \
        bindings/c/shell_wire/native_launcher.c bindings/c/shell_wire/native_launcher_codec.c \
        bindings/c/shell_wire/native_launcher_content.c bindings/c/shell_wire/content_resource.c "bindings/c/tests/sophia_shell_wire_$test.c" \
        "$@" -o "$build/$test"
done
"$build/test"
"$build/budget_test"
"$build/catalog_test" protocol/golden/sophia-shell-launcher.frames
"$build/native_test" protocol/golden/sophia-shell-native-launcher.frames
"$build/native_codec_test" protocol/golden/sophia-shell-native-launcher.frames
"$build/resource_test" protocol/golden/sophia-shell-content.frames
for corpus in sophia-shell-v1 sophia-shell-tabs sophia-shell-reference \
    sophia-shell-launcher sophia-shell-content sophia-shell-indicators sophia-shell-native-launcher; do
    "$build/corpus" "protocol/golden/$corpus.frames"
done
# One valid envelope with a corrupt magic must not survive the independent reader.
sed 's/|534f5048/|004f5048/' protocol/golden/sophia-shell-v1.frames > "$build/corrupt.frames"
if "$build/corpus" "$build/corrupt.frames" > "$build/corrupt.log" 2>&1; then
    echo 'shell C envelope reader accepted corrupt magic' >&2
    exit 1
fi
printf '%s\n' 'sophia_shell_c_wire status=pass native=false'

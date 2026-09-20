#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
ledger="$root/docs/source-layout-exceptions.txt"
# Accumulated across every pass. Each pass below runs inside an `if`, which
# is what stops `set -e` ending the script at the first one that fails: until
# now the production pass failed on this repository's standing debt every
# time, and the two passes after it -- the test report and the stale-exception
# check -- never ran at all.
overall=0
status=0

is_recorded() {
    category=$1
    path=$2
    grep -Fqx "$category $path" "$ledger"
}

if find "$root/crates" -path '*/src/*.rs' -type f -print | sort | {
while IFS= read -r file; do
    relative=${file#"$root/"}
    lines=$(wc -l < "$file")
    if [ "$lines" -ge 800 ]; then
        printf '%s %s %s\n' source-lines "$lines" "$relative"
    fi
    if [ "$lines" -gt 1000 ] && ! is_recorded large-source "$relative"; then
        printf '%s\n' "error: $relative has $lines lines and no reviewed cohesion exception" >&2
        status=1
    fi
    case "$relative" in
        */src/tests.rs|*/src/tests/*.rs|*/src/*/tests.rs|*/src/*/tests/*.rs|\
        */src/*/*/tests.rs|*/src/*/*/tests/*.rs|*/src/*/*/*/tests.rs|\
        */src/*/*/*/tests/*.rs|*/src/*/*/*/*/tests.rs|*/src/*/*/*/*/tests/*.rs)
            ;;
        *)
            if rg -q '#\[cfg\(test\)\]|#\[test\]' "$file" &&
                ! is_recorded inline-tests "$relative"; then
                printf '%s\n' "error: inline tests in $relative" >&2
                status=1
            fi
            ;;
    esac
    case "$relative" in
        crates/sophia-cli/*|*/src/main.rs) ;;
        *)
            if rg -q '(^|[^[:alnum:]_])(eprintln!|println!)' "$file" &&
                ! is_recorded direct-printing "$relative"; then
                printf '%s\n' "error: direct library printing in $relative" >&2
                status=1
            fi
            ;;
    esac
done
exit "$status"
}
then :; else overall=1; fi

# Tests answer to a ceiling of their own, and a higher one than production.
# A wire fixture or a table of refusals is one dense table with one owner,
# which is the cohesion argument the production rule already makes for a
# dense parser -- so the bar is looser, not absent. It was absent until now,
# and test files grew to five thousand lines with nothing to say so.
#
# The sentence is deliberately the one the production rule prints, because
# the xtask ledger normalizes errors by that exact shape and a second wording
# would need a second normalizer to recognise the same kind of debt.
status=0
if find "$root/crates" -path '*/tests/*.rs' -type f -print | sort | {
while IFS= read -r file; do
    relative=${file#"$root/"}
    lines=$(wc -l < "$file")
    if [ "$lines" -ge 800 ]; then
        printf '%s %s %s\n' test-lines "$lines" "$relative"
    fi
    if [ "$lines" -gt 1500 ] && ! is_recorded large-source "$relative"; then
        printf '%s\n' "error: $relative has $lines lines and no reviewed cohesion exception" >&2
        status=1
    fi
done
exit "$status"
}
then :; else overall=1; fi

status=0
while IFS=' ' read -r category relative; do
    [ -n "${category:-}" ] || continue
    case "$category" in \#*) continue ;; esac
    [ -f "$root/$relative" ] || {
        printf '%s\n' "error: stale source-layout exception: $category $relative" >&2
        status=1
    }
done < "$ledger"
[ "$status" -eq 0 ] || overall=1

exit "$overall"

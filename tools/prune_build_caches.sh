#!/bin/sh
# Reclaim abandoned cargo target directories.
#
# Discovery is by `.rustc_info.json`, which cargo writes at the root of every
# target directory it owns. This finds bare `--target-dir` locations that
# cargo-sweep cannot see, because sweep only looks beside a Cargo.toml and
# skips dot-directories unless told otherwise.
#
# Dry run unless --apply is given.

set -eu

DAYS=14
APPLY=0
ROOTS=""
KEEPFILE=""
QUIET=0

usage() {
    cat <<'USAGE'
usage: prune_build_caches.sh [options]

  --days N       remove targets untouched for more than N days (default 14)
  --root DIR     directory to scan; repeatable (default $HOME/dev)
  --keep FILE    file of substrings; a target whose path contains one is kept
  --apply        actually delete (default is a dry run)
  --quiet        only print the summary
  -h, --help     this text
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --days)  DAYS="$2"; shift 2 ;;
        --root)  ROOTS="$ROOTS $2"; shift 2 ;;
        --keep)  KEEPFILE="$2"; shift 2 ;;
        --apply) APPLY=1; shift ;;
        --quiet) QUIET=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

[ -n "$ROOTS" ] || ROOTS="$HOME/dev"

case "$DAYS" in
    ''|*[!0-9]*) echo "--days needs a non-negative integer, got: $DAYS" >&2; exit 2 ;;
esac

say() { [ "$QUIET" -eq 1 ] || printf '%s\n' "$*"; }

# Newest mtime anywhere in the target's top two levels, as an epoch second.
# Cargo refreshes .rustc_info.json and the profile dirs on every build, so this
# tracks real use without walking the whole tree.
freshness() {
    find "$1" -maxdepth 2 -printf '%T@\n' 2>/dev/null \
        | sort -rn | head -1 | cut -d. -f1
}

kept_by_list() {
    [ -n "$KEEPFILE" ] && [ -f "$KEEPFILE" ] || return 1
    while IFS= read -r pat; do
        case "$pat" in ''|\#*) continue ;; esac
        case "$1" in *"$pat"*) return 0 ;; esac
    done < "$KEEPFILE"
    return 1
}

now=$(date +%s)
cutoff=$(( now - DAYS * 86400 ))
total_kb=0
removed=0
scanned=0

say "scanning:${ROOTS} (threshold: ${DAYS}d)"
[ "$APPLY" -eq 1 ] || say "DRY RUN - nothing will be deleted; pass --apply to act"
say ""

# NUL-delimited so paths with spaces survive.
targets=$(mktemp)
trap 'rm -f "$targets"' EXIT
for root in $ROOTS; do
    [ -d "$root" ] || { echo "skipping missing root: $root" >&2; continue; }
    find "$root" -name .rustc_info.json -type f -printf '%h\n' 2>/dev/null
done | sort -u > "$targets"

while IFS= read -r dir; do
    [ -d "$dir" ] || continue
    scanned=$(( scanned + 1 ))

    if kept_by_list "$dir"; then
        say "keep (listed)  $dir"
        continue
    fi

    touched=$(freshness "$dir")
    [ -n "$touched" ] || touched=$now
    if [ "$touched" -gt "$cutoff" ]; then
        age_d=$(( (now - touched) / 86400 ))
        say "keep (${age_d}d)     $dir"
        continue
    fi

    size_kb=$(du -sk "$dir" 2>/dev/null | cut -f1)
    [ -n "$size_kb" ] || size_kb=0
    age_d=$(( (now - touched) / 86400 ))
    size_h=$(du -sh "$dir" 2>/dev/null | cut -f1)

    if [ "$APPLY" -eq 1 ]; then
        rm -rf -- "$dir"
        say "removed ${age_d}d ${size_h}	$dir"
    else
        say "would remove ${age_d}d ${size_h}	$dir"
    fi
    total_kb=$(( total_kb + size_kb ))
    removed=$(( removed + 1 ))
done < "$targets"

say ""
verb=$([ "$APPLY" -eq 1 ] && echo reclaimed || echo reclaimable)
printf '%d target dirs scanned, %d %s, %s\n' \
    "$scanned" "$removed" "$verb" \
    "$(echo "$total_kb" | awk '{printf "%.2f GiB", $1/1048576}')"

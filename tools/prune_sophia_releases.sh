#!/usr/bin/env bash
# Reclaim installed Sophia releases that nothing selects any more.
#
# Installation mints a release per commit and never removes one, so a machine
# that has been installed from for a few months accumulates every build it ever
# ran. Only two are ever reachable: the `current` symlink and the `previous`
# one that `sophia-rollback` swaps to.
#
# Those two are never candidates here, whatever the arguments say. Beyond them
# a number of the most recent releases are retained, because rolling back more
# than one step means reinstalling from a commit rather than switching a link,
# and that is a worse position to be in than holding some disk.
#
# Dry run unless --apply is given.
set -euo pipefail

PREFIX="${SOPHIA_INSTALL_PREFIX:-/opt/sophia}"
KEEP=5
APPLY=0

usage() {
    cat <<'USAGE'
usage: prune_sophia_releases.sh [options]

  --keep N       retain this many recent releases beyond current and
                 previous, newest first (default 5)
  --prefix DIR   install prefix (default /opt/sophia, or
                 $SOPHIA_INSTALL_PREFIX)
  --apply        actually delete (default is a dry run)
  -h, --help     this text
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --keep)   KEEP="$2"; shift 2 ;;
        --prefix) PREFIX="$2"; shift 2 ;;
        --apply)  APPLY=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

[[ "$KEEP" =~ ^[0-9]+$ ]] || { echo "--keep needs a non-negative integer" >&2; exit 2; }
releases="$PREFIX/releases"
[[ -d "$releases" ]] || { echo "No release directory: $releases" >&2; exit 1; }

# Resolve to bare release ids. A missing `previous` is normal on a first
# install and simply contributes no protected name.
protected=()
for link in current previous; do
    target="$(readlink "$PREFIX/$link" 2>/dev/null || true)"
    [[ -n "$target" ]] && protected+=("${target#releases/}")
done
(( ${#protected[@]} > 0 )) || {
    echo "Refusing to prune: $PREFIX/current does not resolve to a release." >&2
    exit 1
}

# Newest first by mtime, so --keep retains the most recently installed.
mapfile -t ordered < <(ls -1dt "$releases"/*/ 2>/dev/null | sed 's|/$||;s|.*/||')
(( ${#ordered[@]} > 0 )) || { echo "No releases under $releases"; exit 0; }

is_protected() {
    local name="$1" p
    for p in "${protected[@]}"; do
        [[ "$name" == "$p" ]] && return 0
    done
    return 1
}

kept=0
removed=0
total_kb=0
candidates=()
for name in "${ordered[@]}"; do
    if is_protected "$name"; then
        printf '  keep (selected)  %s\n' "$name"
        continue
    fi
    if (( kept < KEEP )); then
        kept=$((kept + 1))
        printf '  keep (recent %d)  %s\n' "$kept" "$name"
        continue
    fi
    candidates+=("$name")
done

printf '\n'
for name in "${candidates[@]}"; do
    dir="$releases/$name"
    size_kb="$(du -sk "$dir" 2>/dev/null | cut -f1)"
    size_kb="${size_kb:-0}"
    size_h="$(du -sh "$dir" 2>/dev/null | cut -f1)"
    if (( APPLY )); then
        rm -rf -- "$dir"
        printf '  removed %8s  %s\n' "$size_h" "$name"
    else
        printf '  would remove %8s  %s\n' "$size_h" "$name"
    fi
    total_kb=$((total_kb + size_kb))
    removed=$((removed + 1))
done

if (( APPLY )); then verb=reclaimed; else verb=reclaimable; fi
printf '\n%d releases present, %d protected, %d retained, %d %s, %s\n' \
    "${#ordered[@]}" "${#protected[@]}" "$kept" "$removed" "$verb" \
    "$(awk -v k="$total_kb" 'BEGIN{printf "%.2f GiB", k/1048576}')"
(( APPLY )) || printf 'Dry run. Pass --apply to delete.\n'

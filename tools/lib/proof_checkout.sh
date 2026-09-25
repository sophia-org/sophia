# The checkout predicate the Hagia proof gates and archives share. It asks git
# rather than testing for a .git directory, because a linked worktree's .git is
# a file. The path's real location must be the repository's top level, so a
# plain directory inside some other repository is refused.
proof_checkout_root() {
    local real top
    real="$(cd -- "$1" 2>/dev/null && pwd -P)" || return 1
    top="$(git -C "$real" rev-parse --show-toplevel 2>/dev/null)" || return 1
    [[ "$(cd -- "$top" 2>/dev/null && pwd -P)" == "$real" ]]
}

# A profile a proof run names must be a tracked, unmodified file of one of the
# given checkouts, so the signed commit the run binds also fixes its bytes.
proof_tracked_file() {
    local path="$1" root real_root relative
    shift
    [[ "$path" == /* && -f "$path" ]] || return 1
    path="$(cd -- "$(dirname -- "$path")" && pwd -P)/$(basename -- "$path")"
    for root in "$@"; do
        real_root="$(cd -- "$root" 2>/dev/null && pwd -P)" || continue
        [[ "$path" == "$real_root"/* ]] || continue
        relative="${path#"$real_root"/}"
        git -C "$real_root" ls-files --error-unmatch -- "$relative" >/dev/null 2>&1 || return 1
        [[ -z "$(git -C "$real_root" status --porcelain -- "$relative")" ]] || return 1
        return 0
    done
    return 1
}

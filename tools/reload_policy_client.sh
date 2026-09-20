#!/usr/bin/env bash
# Rebuild the policy client and restart it inside the running session.
#
# The window manager is a supervised child whose state is checkpointed, so
# replacing the binary and ending the process is a reload: the windows, the
# workspaces and the scroller camera come back with it. It lives where its
# owner can write it, so none of this needs privileges.
#
# A running session can do this from a keybinding now -- session:restart-wm,
# Ctrl+Alt+f5 in the shipped profile -- which does not rebuild anything. This
# script is the developer's version: build first, then restart.
#
# A reload that does not come back is rolled back to the binary it replaced,
# because the alternative is a desktop with no window manager.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
hagia_root="${SOPHIA_HAGIA_ROOT:-$ROOT_DIR/../hagia}"
built="${SOPHIA_HAGIA_BIN_SOURCE:-$hagia_root/hagia}"
policy_bin="${SOPHIA_HAGIA_BIN:-${XDG_STATE_HOME:-$HOME/.local/state}/sophia/bin/hagia}"
log="${XDG_STATE_HOME:-$HOME/.local/state}/sophia/hagia-session/session.log"

note() { printf 'reload: %s\n' "$1"; }
die() { printf 'reload: %s\n' "$1" >&2; exit 1; }

[[ -d "$hagia_root" ]] || die "no Hagia worktree at $hagia_root (set SOPHIA_HAGIA_ROOT)"
note "building Hagia from $hagia_root"
(cd "$hagia_root" && nimble build -d:release >/dev/null) || die "Hagia build failed"
[[ -x "$built" ]] || die "the build produced no executable at $built"

# Ask which binary the session is running BEFORE replacing it. Installing
# unlinks the old inode, after which /proc/PID/exe reads as "... (deleted)"
# and no comparison against the path can match -- the check would fail
# against the very install it just performed.
running="$(pgrep -x hagia || true)"
running_exe=""
if [[ -n "$running" ]]; then
    running_exe="$(readlink -f "/proc/$running/exe" 2>/dev/null || true)"
    # A client whose binary has already been replaced reads as "... (deleted)".
    # That is the kernel saying this path's file was swapped underneath it,
    # which is exactly the client a reload wants to replace -- so the suffix is
    # stripped rather than treated as a different program.
    running_exe="${running_exe% (deleted)}"
fi

mkdir -p "$(dirname "$policy_bin")"
backup=""
if [[ -e "$policy_bin" ]]; then
    backup="$policy_bin.previous"
    cp -p "$policy_bin" "$backup"
fi
# Replace by rename so no session can observe a half-written binary, and so a
# client already running keeps the inode it started from.
staged="$policy_bin.staged.$$"
install -m 700 "$built" "$staged"
mv -f "$staged" "$policy_bin"
note "installed $(cd "$hagia_root" && git rev-parse --short HEAD) to $policy_bin"

if [[ -z "$running" ]]; then
    note "no session is running; the next one starts on this build"
    exit 0
fi
if [[ "$running_exe" != "$(readlink -f "$policy_bin")" ]]; then
    note "the running session started from another policy client:"
    note "  ${running_exe:-unknown}"
    note "it predates this one being installed here; the next session start uses it"
    exit 0
fi

before="$(wc -l < "$log" 2>/dev/null || echo 1)"
note "restarting policy client $running"
# HUP rather than TERM: Hagia answers it by writing its checkpoint at the next
# committed cycle and exiting cleanly, so the windows are recorded before the
# process goes. TERM ends it wherever it happens to be.
kill -HUP "$running"
hup_deadline=$((SECONDS + 5))

settled=""
for _ in $(seq 1 100); do
    sleep 0.1
    replacement="$(pgrep -x hagia || true)"
    if [[ "$replacement" == "$running" && $SECONDS -ge $hup_deadline ]]; then
        note "no response to HUP after 5s; ending it"
        kill -TERM "$running" 2>/dev/null || true
        hup_deadline=$((SECONDS + 3600))
    fi
    [[ -n "$replacement" && "$replacement" != "$running" ]] || continue
    # Started is not working. A committed layout is the session's own word
    # that the replacement negotiated the protocol and produced a projection
    # it accepted, which a binary that merely launches cannot fake.
    # Captured rather than `grep -q`: pipefail plus a `tail` over a long
    # session log turns the first match into a SIGPIPE and the match into a
    # miss, which would read as the replacement never settling.
    committed="$(tail -n "+$before" "$log" 2>/dev/null \
        | grep -m1 -E "sophia_live_wm schema=[0-9]+ status=(layout_committed|focus_committed)" || true)"
    if [[ -n "$committed" ]]; then
        settled="$replacement"
        break
    fi
done

if [[ -n "$settled" ]]; then
    note "reloaded; the policy client is now $settled"
    exit 0
fi

[[ -n "$backup" ]] || die "the new policy client never settled and there is nothing to roll back to"
note "the new policy client never settled; rolling back"
cp -p "$backup" "$policy_bin"
survivor="$(pgrep -x hagia || true)"
[[ -n "$survivor" ]] && kill -TERM "$survivor" 2>/dev/null || true
for _ in $(seq 1 100); do
    sleep 0.1
    if [[ -n "$(pgrep -x hagia || true)" ]]; then
        note "rolled back to the previous policy client"
        exit 1
    fi
done
die "rolled back but nothing is running; the session needs a restart"

#!/usr/bin/env bash
# Enable the daily cargo build-cache pruner as a runit service.
#
# Idempotent: enabling what is already enabled reports that and stops, and a
# rerun after a failed step resumes rather than starting over. Nothing here
# deletes a build cache -- it installs the timer that will, and the pruner it
# schedules is the same one you can run by hand first.
#
# Run as your normal user; it calls sudo for the three steps that need it.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVICE_SRC="$ROOT_DIR/tools/svc/cargo-prune"
SERVICE_DST=/etc/sv/cargo-prune
SERVICE_LINK=/var/service/cargo-prune
LOG_DIR=/var/log/cargo-prune

fail() { printf '%s\n' "$*" >&2; exit 1; }

[[ -d "$SERVICE_SRC" ]] || fail "Service definition missing: $SERVICE_SRC"
[[ -x "$ROOT_DIR/tools/prune_build_caches.sh" ]] \
    || fail "Pruner missing or not executable: $ROOT_DIR/tools/prune_build_caches.sh"

# The run script names the account the pruner runs as. If that is not the
# person invoking this, the service would prune someone else's home, so say so
# rather than installing it and finding out at 03:00.
svc_user="$(sed -n 's/.*chpst -u \([A-Za-z0-9_-]*\).*/\1/p' "$SERVICE_SRC/run" | head -1)"
[[ -n "$svc_user" ]] || fail "Could not read the service user from $SERVICE_SRC/run"
if [[ "$svc_user" != "${USER:-$(id -un)}" ]]; then
    fail "Service runs as '$svc_user' but you are '${USER:-$(id -un)}'.
Edit $SERVICE_SRC/run (and its log/run) before enabling."
fi

if [[ -L "$SERVICE_LINK" || -e "$SERVICE_LINK" ]]; then
    printf 'Already enabled: %s\n' "$SERVICE_LINK"
    sudo sv status cargo-prune || true
    exit 0
fi

if ! command -v snooze >/dev/null 2>&1; then
    echo "Installing snooze (sudo required)"
    sudo xbps-install -Sy snooze
fi
command -v snooze >/dev/null 2>&1 || fail "snooze is still not on PATH after install"

echo "Installing the service (sudo required)"
sudo cp -r "$SERVICE_SRC" "$SERVICE_DST"
sudo chmod +x "$SERVICE_DST/run" "$SERVICE_DST/log/run"
sudo mkdir -p "$LOG_DIR"
sudo chown "$svc_user:$svc_user" "$LOG_DIR"

# runsvdir polls /var/service every few seconds; the link is what starts it.
sudo ln -s "$SERVICE_DST" "$SERVICE_LINK"

# Verify runit actually picked it up rather than trusting that the link exists.
for _ in $(seq 1 15); do
    if sudo sv status cargo-prune >/dev/null 2>&1; then
        break
    fi
    sleep 1
done
sudo sv status cargo-prune || fail "runit did not start cargo-prune; check $LOG_DIR"

cat <<INFO

Enabled. The pruner runs daily at 03:00 (spread over the following hour),
removing cargo target directories untouched for more than 14 days, except
those matched by tools/build-cache-keep.txt.

  Logs      sudo svlogd-friendly tail: sudo tail -f $LOG_DIR/current
  Status    sudo sv status cargo-prune
  Disable   sudo rm $SERVICE_LINK && sudo sv down cargo-prune
  Dry run   $ROOT_DIR/tools/prune_build_caches.sh --root ~/dev \\
                --days 14 --keep $ROOT_DIR/tools/build-cache-keep.txt
INFO

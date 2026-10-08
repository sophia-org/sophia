"""Render reviewable host text only. This module installs or starts nothing."""
from pathlib import Path
import shlex
from config import parse
from policy import deny_rule


def service_run(config):
    parse(config)
    quote = shlex.quote
    tools = config["tools"]
    # After exec, timeout retains this script's PID ($$). Its child verifies
    # that identity before running owner.py, closing parent death before exec.
    return ("#!/bin/sh\nset -eu\n"
            "exec /usr/bin/env -i PATH=/usr/bin:/bin LANG=C.UTF-8 "
            f"{quote(tools['timeout']['path'])} -s KILL {config['outer_seconds']} "
            f"{quote(tools['custody']['path'])} \"$$\" -- "
            f"{quote(tools['python']['path'])} -I -B "
            f"{quote(str(Path(config['bundle']) / 'owner.py'))} --run < /dev/null\n")


def proposed_files(config):
    return {
        # Register down; a reviewed operator invocation uses `sv once`, never
        # `up`, so an exit cannot silently start another development Session.
        "/etc/sv/sophia-development/down": "",
        "/etc/sv/sophia-development/run": service_run(config),
        "/etc/polkit-1/rules.d/00-000-sophia-development.rules": deny_rule(config["user"]),
        "/etc/pam.d/sophia-development": pam_service(config),
    }


def pam_service(config):
    # Absolute, pinned module paths prevent implicit PAM module search.
    paths = [config["tools"][name]["path"] for name in ("pam_permit", "pam_elogind")]
    if any(any(character.isspace() for character in path) for path in paths):
        raise ValueError("PAM module paths cannot contain whitespace")
    return ("# Root-owned fixed service authorizes this dedicated login.\n"
            f"account required {paths[0]}\n"
            f"session required {paths[1]}\n")

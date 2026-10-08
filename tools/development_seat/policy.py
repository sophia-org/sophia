"""Read-only preflight for the dedicated account's host polkit restriction.

Installing/reloading this policy is a separate reviewed host transaction. File
identity is not evidence that a running polkit daemon has consumed it: the
privileged qualification must check negative authorization before a GPU launch.
"""
from pathlib import Path
from config import digest, trusted


DIRECTORIES = (Path("/etc/polkit-1/rules.d"), Path("/usr/share/polkit-1/rules.d"))
DENY_PATH = Path("/etc/polkit-1/rules.d/00-000-sophia-development.rules")


def deny_rule(user):
    # user is already a restricted ASCII account name, never arbitrary JS.
    return ('// Fixed development account: no host policy grants.\n'
            'polkit.addRule(function(action, subject) {\n'
            f'    if (subject.user === "{user}") return polkit.Result.NO;\n'
            '});\n')


def verify(config):
    found = []
    for root in DIRECTORIES:
        trusted(root)
        found.extend(root.glob("*.rules"))
    if not found or min(path.name for path in found) != DENY_PATH.name:
        raise ValueError("development polkit deny rule is not first")
    if sum(path.name == DENY_PATH.name for path in found) != 1 or DENY_PATH not in found:
        raise ValueError("ambiguous development polkit deny rule")
    pins = {pin["path"]: pin["sha256"] for pin in config["files"]}
    for path in found:
        trusted(path, regular=True)
        if pins.get(str(path)) != digest(path):
            raise ValueError("host polkit rules changed since the reviewed bundle")
    if DENY_PATH.read_text() != deny_rule(config["user"]):
        raise ValueError("development polkit deny rule differs from the fixed rule")
    return {str(path): pins[str(path)] for path in sorted(found)}

#!/usr/bin/env python3
"""Generate explicit probe overrides; WM policy and bindings remain WM-owned."""
import argparse
import json
from pathlib import Path


def profile(lom: str, config: str, bemenu: str) -> str:
    def quoted(value):
        path = Path(value)
        if not path.is_absolute() or any(ord(c) < 32 for c in value):
            raise ValueError("component paths must be absolute without control characters")
        return json.dumps(value, ensure_ascii=False)
    return f'''schema 1
shell {{ enabled #true; content #true; content-input #true; panel 24; gpu "denied"; }}
session {{
    shell-component "panel" "bar" {{ executable {quoted(lom)}; config {quoted(config)}; gpu "direct"; }}
    shell-component "menu" "application-launcher" {{ executable {quoted(bemenu)}; gpu "denied"; }}
    application-catalog "native-launcher-gate"
    startup
}}
input {{ inherit-sophia #true; }}
output {{ inherit-sophia #true; }}
broker {{ enabled #false; }}
'''


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('lom', 'config', 'bemenu'):
        parser.add_argument('--' + name, required=True)
    args = parser.parse_args()
    print(profile(args.lom, args.config, args.bemenu), end='')

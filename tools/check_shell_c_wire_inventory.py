#!/usr/bin/env python3
"""Check independent C vocabulary and bounded source layout against the schema."""
from pathlib import Path
import re

root = Path(__file__).resolve().parent.parent
schema = (root / "protocol/sophia-shell-v1.kdl").read_text()
records = re.findall(
    r'message "[^"]+" kind=(\d+) direction="([^"]+)" transaction="([^"]+)"', schema
)
assert len(records) == len({kind for kind, _, _ in records}), "duplicate shell kind"
fields = (root / "bindings/c/shell_wire/fields.h").read_text()
direction_body = fields.split("switch (kind)", 1)[1].split("default:", 1)[0]
actual = {}
for cases, direction in re.findall(r"((?:\s*case \d+:)+)\s*return ([01]);", direction_body):
    for kind in re.findall(r"case (\d+):", cases):
        assert kind not in actual, "duplicate C kind"
        actual[kind] = "shell-to-session" if direction == "1" else "session-to-shell"
assert actual == {kind: direction for kind, direction, _ in records}, "C/schema direction drift"
transaction_body = fields.split("shell_transaction_valid", 1)[1]
zero = set(re.findall(r"kind == (\d+)", transaction_body))
assert zero == {kind for kind, _, tx in records if tx == "zero"}, "C/schema transaction drift"
header = (root / "bindings/c/sophia_shell_wire.h").read_text()
caps = re.findall(r"#define SOPHIA_SHELL_CAP_(\w+) \(UINT64_C\(1\) << (\d+)\)", header)
assert {name.lower(): bit for name, bit in caps} == dict(
    re.findall(r'capability "([^"]+)" bit=(\d+)', schema)
), "C/schema capability drift"
revision = re.search(r"SOPHIA_SHELL_WIRE_MAX_REVISION (\d+)u", header).group(1)
assert revision == re.search(r"interface-revision=(\d+)", schema).group(1), "C/schema revision drift"

paths = [root / "bindings/c/sophia_shell_wire.h", root / "bindings/c/sophia_shell_catalog.h", root / "bindings/c/sophia_shell_native_launcher.h", root / "bindings/c/sophia_shell_content_resource.h", root / "bindings/c/sophia_shell_content_types.h"]
paths += list((root / "bindings/c/shell_wire").glob("*.[ch]"))
paths += list((root / "bindings/c/tests").glob("sophia_shell_wire_*.c"))
for path in paths:
    size = len(path.read_text().splitlines())
    assert size <= 1000, f"source length exceeds 1000: {path} ({size})"
    if size >= 800:
        print(f"review cohesion: {path.relative_to(root)} ({size} lines)")
print(f"shell C inventory: {len(records)} message kinds; {len(paths)} source files")

#!/usr/bin/env python3
"""Generate neutral fixed WM rows from the pinned file contract."""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCHEMA = "spec/sophia-wm-files-v1.kdl"
HEADER = "src/sophia_wm_records.h"
SOURCE = "src/wm_files/rows.c"
ORDINARY_ROWS = 8
EXTENSION_ROWS = 14
EXTENSION_FLOOR = 0xFF00
HEADER_PROPS = ("interface-major", "interface-revision", "max-outputs",
                "max-surfaces", "max-bindings")
TYPES = {"u8": "uint8_t", "u16": "uint16_t", "u32": "uint32_t",
         "u64": "uint64_t", "i32": "int32_t"}
LIMITS = {"u16": 0xFFFF, "u32": 0xFFFFFFFF, "u64": 0xFFFFFFFFFFFFFFFF,
          "i32": 0x7FFFFFFF}
TOKEN = re.compile(r'\s*(?:(//.*)|(\{)|(\})|"([^"\\]*)"'
                   r'|([A-Za-z_][\w-]*)=("[^"\\]*"|#true|#false|\d+)(?![\w"#-])'
                   r'|([A-Za-z_][\w-]*)(?![\w=-]))')


class SchemaError(ValueError):
    pass


class Node:
    def __init__(self, name, line):
        self.name, self.line = name, line
        self.args, self.props, self.children = [], {}, None

    def arg(self):
        if len(self.args) != 1:
            raise SchemaError(f"line {self.line}: `{self.name}` needs one string name")
        return self.args[0]

    def prop(self, key, kind):
        value = self.props.get(key)
        if type(value) is not kind:
            raise SchemaError(f"line {self.line}: `{self.name}` needs {kind.__name__} {key}")
        return value


def value(text):
    if text.startswith('"'):
        return text[1:-1]
    if text in ("#true", "#false"):
        return text == "#true"
    return int(text)


def parse(text):
    """Parse the one-node-per-line KDL subset these schemas use; refuse the rest."""
    top = Node(None, 0)
    top.children = []
    stack = [top]
    for number, line in enumerate(text.splitlines(), 1):
        position, node, done = 0, None, False
        while line[position:].strip():
            match = TOKEN.match(line, position)
            if not match or done:
                raise SchemaError(f"line {number}: unsupported syntax: {line.strip()}")
            position = match.end()
            comment, opened, closed, string, key, prop, bare = match.groups()
            if comment:
                break
            if closed:
                if node or len(stack) == 1:
                    raise SchemaError(f"line {number}: unbalanced `}}`")
                stack.pop()
                done = True
            elif opened:
                if not node:
                    raise SchemaError(f"line {number}: `{{` must end a node line")
                node.children = []
                stack.append(node)
                done = True
            elif node is None:
                if not bare:
                    raise SchemaError(f"line {number}: expected a node name")
                node = Node(bare, number)
                stack[-1].children.append(node)
            elif key:
                if key in node.props:
                    raise SchemaError(f"line {number}: duplicate property {key}")
                node.props[key] = value(prop)
            elif string is not None and not node.props:
                node.args.append(string)
            else:
                raise SchemaError(f"line {number}: unsupported syntax: {line.strip()}")
    if len(stack) != 1:
        raise SchemaError("unterminated node block")
    return top.children


def protocol(nodes, name):
    found = [node for node in nodes if node.name == "protocol"]
    if len(found) != 1 or found[0].arg() != name or found[0].children is None:
        raise SchemaError(f"expected one protocol `{name}` with children")
    return found[0]


def parse_field(node, record):
    if node.name != "field":
        raise SchemaError(f"line {node.line}: unknown child `{node.name}` in `{record}`")
    key, typ = node.arg(), node.prop("type", str)
    unknown = set(node.props) - {"type", "count", "reserved", "sample"}
    if unknown or node.children is not None:
        raise SchemaError(f"line {node.line}: field `{key}` has unsupported shape")
    if node.props.get("reserved", True) is not True:
        raise SchemaError(f"line {node.line}: field `{key}` reserved must be #true")
    reserved = "reserved" in node.props
    if "sample" not in node.props:
        raise SchemaError(f"line {node.line}: field `{key}` is missing sample")
    sample = node.props["sample"]
    if typ == "u8":
        count = node.prop("count", int)
        if not 1 <= count <= 256 or reserved:
            raise SchemaError(f"line {node.line}: field `{key}` has an invalid octet run")
        if not isinstance(sample, str) or len(sample) != 2 * count \
                or not re.fullmatch(r"[0-9a-f]*", sample):
            raise SchemaError(f"line {node.line}: field `{key}` sample must be {count} hex bytes")
    elif typ in LIMITS:
        if "count" in node.props:
            raise SchemaError(f"line {node.line}: field `{key}` count needs type u8")
        count = 1
        if type(sample) is not int or sample > LIMITS[typ] or (reserved and sample):
            raise SchemaError(f"line {node.line}: field `{key}` sample is out of range")
    else:
        raise SchemaError(f"line {node.line}: field `{key}` has unsupported type `{typ}`")
    return (key, typ, int(typ[1:]) // 8, count, reserved, sample)


def parse_record(node, extension):
    name = node.arg()
    allowed = {"transfer", "kind", "max"} | ({"gate"} if extension else set())
    if set(node.props) != allowed or not node.children:
        raise SchemaError(f"line {node.line}: record `{name}` has unsupported shape")
    transfer, kind = node.prop("transfer", str), node.prop("kind", int)
    maximum = node.prop("max", int)
    gate = node.prop("gate", str) if extension else None
    if transfer not in ("snapshot", "projection") or not 0 < maximum <= 0xFFFFFFFF \
            or not 0 < kind <= 0xFFFF or (kind >= EXTENSION_FLOOR) != extension:
        raise SchemaError(f"line {node.line}: record `{name}` has invalid transfer, kind or max")
    fields = tuple(parse_field(child, name) for child in node.children)
    if len({field[0] for field in fields}) != len(fields):
        raise SchemaError(f"line {node.line}: record `{name}` repeats a field")
    return (name, transfer, kind, gate, maximum, fields)


def collect(owner):
    """Return the row contract carried by `owner`: header values and row children."""
    rows = {"header": tuple(owner.prop(key, int) for key in HEADER_PROPS),
            "capabilities": [], "outcomes": [], "records": []}
    for node in owner.children:
        if node.name == "capability":
            rows["capabilities"].append((node.arg(), node.prop("bit", int)))
        elif node.name == "outcome":
            rows["outcomes"].append((node.arg(), node.prop("value", int)))
        elif node.name in ("record", "extension-record"):
            rows["records"].append(parse_record(node, node.name == "extension-record"))
        elif owner.name == "row-layouts":
            raise SchemaError(f"line {node.line}: unexpected WM row layout node `{node.name}`")
    return rows


def read_rows(text):
    root = protocol(parse(text), "sophia_wm_fs_v1")
    layouts = [node for node in root.children if node.name == "row-layouts"]
    if len(layouts) != 1:
        raise SchemaError("WM file contract requires exactly one row-layouts node")
    layout = layouts[0]
    if layout.args or set(layout.props) != set(HEADER_PROPS) or not layout.children:
        raise SchemaError(f"line {layout.line}: row-layouts has unsupported shape")
    rows = collect(layout)
    if rows["header"][0] == 0:
        raise SchemaError("WM row layouts require a nonzero interface")
    names = [name for name, _ in rows["capabilities"]]
    bits = [bit for _, bit in rows["capabilities"]]
    if not names or len(set(names)) != len(names) or len(set(bits)) != len(bits) \
            or max(bits) > 63:
        raise SchemaError("WM row capabilities must be unique bits below 64")
    records = rows["records"]
    ordinary = [row for row in records if row[3] is None]
    if len(ordinary) != ORDINARY_ROWS or len(records) - len(ordinary) != EXTENSION_ROWS:
        raise SchemaError("review changed WM row inventory before regeneration")
    if len({row[0] for row in records}) != len(records) \
            or len({(row[1], row[2]) for row in records}) != len(records):
        raise SchemaError("WM row names and transfer kinds must be unique")
    for name, _, _, gate, _, _ in records:
        if gate is not None and gate not in names:
            raise SchemaError(f"row `{name}` is gated on unknown capability `{gate}`")
    return rows



def snake(value):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", value).lower()


def render(rows):
    """Return the generated header and source text for the fixed rows."""
    table = []
    for name, family, kind, gate, maximum, fields in rows["records"]:
        offset, laid = 0, []
        for key, typ, width, count, _, _ in fields:
            laid.append((key, typ, width, count, offset))
            offset += width * count
        table.append((snake(name), family, kind, gate, maximum, offset, laid))
    banner = f"/* Generated by tools/generate_wm_rows.py from {SCHEMA} row-layouts. */"
    h = [banner, "#ifndef SOPHIA_WM_RECORDS_H", "#define SOPHIA_WM_RECORDS_H",
         "#include <stddef.h>", "#include <stdint.h>"]
    for name, bit in rows["capabilities"]:
        h.append(f"#define SOPHIA_WF_CAP_{name.upper()} (UINT64_C(1) << {bit})")
    h += ["/* Fixed rows only; no socket headers or transfer framing.",
          " * Decode and encode check width and reserved bytes. Complete file-record",
          " * validation additionally checks sections and negotiated capabilities.",
          " * Outputs are unchanged on error. Return 0, -1 invalid, -4 argument/space.",
          " * Destination must not overlap the value or its source bytes. */"]
    c = [banner, '#include "internal.h"']
    for name, family, kind, gate, maximum, size, fields in table:
        base = "sophia_wf_" + name
        h += [f"#define SOPHIA_WF_{name.upper()}_BYTES {size}u",
              f"struct {base} {{"]
        for key, typ, width, count, offset in fields:
            if not key.startswith("reserved"):
                suffix = f"[{count}]" if count > 1 else ""
                h.append(f"    {TYPES[typ]} {key}{suffix};")
        h += ["};", f"int {base}_decode(const void *, size_t, struct {base} *);",
              f"int {base}_encode(void *, size_t, const struct {base} *);"]
        c += [f"int {base}_decode(const void *src, size_t size, struct {base} *out)",
              "{", "    const uint8_t *p = src;", f"    struct {base} v;",
              "    if (!src || !out) return -4;", f"    if (size != {size}u) return -1;",
              "    memset(&v, 0, sizeof(v));"]
        for key, typ, width, count, offset in fields:
            if key.startswith("reserved"):
                c.append(f"    if (!wf_zero(p + {offset}, {width * count})) return -1;")
            elif count > 1:
                c.append(f"    memcpy(v.{key}, p + {offset}, {count});")
            else:
                getter = "wf_signed" if typ == "i32" else "wf_get"
                c.append(f"    v.{key} = ({TYPES[typ]}){getter}(p + {offset}, {width});")
        c += ["    *out = v;", "    return 0;", "}",
              f"int {base}_encode(void *dst, size_t size, const struct {base} *v)",
              "{", f"    uint8_t p[{size}];",
              f"    if (!dst || !v || size < {size}u) return -4;",
              "    memset(p, 0, sizeof(p));"]
        for key, typ, width, count, offset in fields:
            if key.startswith("reserved"):
                continue
            if count > 1:
                c.append(f"    memcpy(p + {offset}, v->{key}, {count});")
            else:
                c.append(f"    wf_put(p + {offset}, (uint64_t)v->{key}, {width});")
        c += ["    memcpy(dst, p, sizeof(p));", "    return 0;", "}"]
    c += ["int wf_row_layout(uint16_t family, uint16_t kind, size_t *width,",
          "                  uint32_t *maximum, uint64_t *capabilities)", "{"]
    families = {"snapshot": "SOPHIA_WF_SNAPSHOT", "projection": "SOPHIA_WF_PROJECTION"}
    caps = {"snapshot_action": "ACTIONS", "snapshot_session_operation": "SESSION_OPERATIONS",
            "projection_indicator": "INDICATORS", "projection_output_status": "INDICATORS"}
    for name, family, kind, gate, maximum, size, fields in table:
        cap = gate.upper() if gate else caps.get(name)
        cap_expr = "SOPHIA_WF_CAP_" + cap if cap else "0"
        if cap == "OUTPUT_LAUNCH_CONTEXT":
            cap_expr += " | SOPHIA_WF_CAP_LAUNCH_ORIGIN"
        if cap == "PRESENTATION_ACTIONS":
            cap_expr += " | SOPHIA_WF_CAP_ACTIONS | SOPHIA_WF_CAP_SURFACE_INSTANCES"
        cond = f"family == {families[family]} && kind == {kind}u"
        if name == "snapshot_action":
            cond = f"(family == SOPHIA_WF_SNAPSHOT || family == SOPHIA_WF_CONFIGURATION) && kind == {kind}u"
        c += [f"    if ({cond}) {{", f"        *width = {size}; *maximum = {maximum};",
              f"        *capabilities = {cap_expr};", "        return 0;", "    }"]
    c += ["    return -1;", "}",
          "int wf_row_reserved(uint16_t family, uint16_t kind, const uint8_t *p)", "{"]
    for name, family, kind, gate, maximum, size, fields in table:
        reserved = [(offset, width * count) for key, typ, width, count, offset in fields
                    if key.startswith("reserved")]
        if reserved:
            cond = " && ".join(f"wf_zero(p + {offset}, {width})" for offset, width in reserved)
            c += [f"    if (family == {families[family]} && kind == {kind}u)",
                  f"        return {cond} ? 0 : -1;"]
    c += ["    return 0;", "}"]
    h += ["#endif", ""]
    return "\n".join(h), "\n".join(c) + "\n"


def generate(root=ROOT):
    rows = read_rows((root / SCHEMA).read_text())
    return render(rows)


def main(argv):
    if argv[1:] not in ([], ["--check"]):
        raise SchemaError("usage: generate_wm_rows.py [--check]")
    header, source = generate()
    if argv[1:]:
        if (ROOT / HEADER).read_text() != header or (ROOT / SOURCE).read_text() != source:
            raise SchemaError("generated WM rows are stale; run tools/generate_wm_rows.py")
        return
    (ROOT / "src/wm_files").mkdir(exist_ok=True)
    (ROOT / HEADER).write_text(header)
    (ROOT / SOURCE).write_text(source)


if __name__ == "__main__":
    try:
        main(sys.argv)
    except SchemaError as error:
        sys.exit(f"generate_wm_rows.py: {error}")

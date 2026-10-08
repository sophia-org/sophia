"""Inspect ELF64 loader paths without executing a proposed privileged tool."""
from pathlib import Path
import struct


def loader_paths(path):
    data = path.read_bytes()
    if data[:7] != b"\x7fELF\x02\x01\x01" or len(data) < 64:
        raise ValueError("privileged tools must be native little-endian ELF64 files")
    phoff = struct.unpack_from("<Q", data, 32)[0]
    size, count = struct.unpack_from("<HH", data, 54)
    if size != 56 or not 1 <= count <= 256 or phoff + size * count > len(data):
        raise ValueError("invalid ELF program headers")
    headers = [struct.unpack_from("<IIQQQQQQ", data, phoff + i * size) for i in range(count)]

    def segment(header):
        offset, extent = header[2], header[5]
        if offset + extent > len(data):
            raise ValueError("truncated ELF segment")
        return data[offset:offset + extent]

    paths = []
    for header in headers:
        if header[0] == 3:  # PT_INTERP
            raw = segment(header)
            if not raw.endswith(b"\0") or raw.count(b"\0") != 1:
                raise ValueError("invalid ELF interpreter")
            paths.append(raw[:-1].decode())
    dynamic = [segment(header) for header in headers if header[0] == 2]
    if len(dynamic) > 1:
        raise ValueError("multiple ELF dynamic segments")
    entries = []
    if dynamic:
        if len(dynamic[0]) % 16:
            raise ValueError("invalid ELF dynamic extent")
        for tag, value in struct.iter_unpack("<QQ", dynamic[0]):
            if tag == 0:
                break
            entries.append((tag, value))
    offsets = [value for tag, value in entries if tag in (15, 29)]
    if not offsets:
        return paths
    address = [value for tag, value in entries if tag == 5]
    extent = [value for tag, value in entries if tag == 10]
    if len(address) != 1 or len(extent) != 1:
        raise ValueError("missing ELF string table")
    matches = [header for header in headers if header[0] == 1 and
               header[3] <= address[0] and address[0] + extent[0] <= header[3] + header[5]]
    if len(matches) != 1:
        raise ValueError("ELF string table outside a load segment")
    header = matches[0]
    strings = segment(header)[address[0] - header[3]:address[0] - header[3] + extent[0]]
    for offset in offsets:
        if offset >= len(strings) or b"\0" not in strings[offset:]:
            raise ValueError("invalid ELF library search path")
        raw = strings[offset:].split(b"\0", 1)[0].decode()
        for item in raw.split(":"):
            item = item.replace("${ORIGIN}", str(path.parent)).replace("$ORIGIN", str(path.parent))
            if "$" in item or not Path(item).is_absolute():
                raise ValueError("unsupported or relative ELF loader path")
            paths.append(item)
    return paths

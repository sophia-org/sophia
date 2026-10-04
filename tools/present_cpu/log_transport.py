"""Lossless post-measurement guest log export with bounded decompression."""
import base64
import gzip
import hashlib
import io
import re

BEGIN = "sophia_qemu_cpu_log_gzip_begin\n"
END = "sophia_qemu_cpu_log_gzip_end\n"


def unpack(text, limit=128 * 1024 * 1024):
    if text.count(BEGIN) != 1 or text.count(END) != 1:
        raise ValueError("missing or duplicate compressed session log")
    before, rest = text.split(BEGIN)
    encoded, after = rest.split(END)
    if len(encoded) > 32 * 1024 * 1024:
        raise ValueError("compressed session log exceeds bound")
    try:
        compressed = base64.b64decode("".join(encoded.split()), validate=True)
        with gzip.GzipFile(fileobj=io.BytesIO(compressed)) as stream:
            raw = stream.read(limit + 1)
    except (ValueError, OSError, EOFError) as error:
        raise ValueError("invalid compressed session log") from error
    if len(raw) > limit:
        raise ValueError("session log exceeds uncompressed bound")
    metadata = re.findall(r"^sophia_qemu_cpu_log schema=1 transport=tmpfs "
                          r"export=gzip_base64_after_measurement bytes=(\d+) "
                          r"sha256=([0-9a-f]{64}) tmpfs_used_kib=(\d+) "
                          r"export_start_uptime=([0-9.]+)$", before, re.M)
    endings = re.findall(r"^sophia_qemu_cpu_log schema=1 export_end_uptime=([0-9.]+) status=0$",
                         after, re.M)
    sha = hashlib.sha256(raw).hexdigest()
    if len(metadata) != 1 or len(endings) != 1:
        raise ValueError("missing or duplicate session log metadata")
    size, expected_sha, used, start = metadata[0]
    elapsed = float(endings[0]) - float(start)
    if int(size) != len(raw) or expected_sha != sha or elapsed < 0:
        raise ValueError("session log metadata mismatch")
    return before + raw.decode(errors="replace") + after, raw, {
        "bytes": len(raw), "sha256": sha,
        "tmpfs_used_kib": int(used), "export_seconds": elapsed,
        "compressed_bytes": len(compressed),
        "compressed_sha256": hashlib.sha256(compressed).hexdigest(),
    }

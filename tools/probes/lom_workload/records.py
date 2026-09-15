"""Bounded, strict evidence decoding. Host and client logs stay separate."""

import re
from pathlib import Path


class InvalidEvidence(ValueError):
    pass


U64 = (1 << 64) - 1
MAX_LOG_BYTES = 64 * 1024 * 1024
MAX_RECORDS = 100_000

# A tuple gives mandatory integer fields, fields admitting zero, and enums.
# Unknown fields fail rather than quietly changing the interpreted contract.
HOST = {
    "sophia_shell_action_receipt": (
        "connection_epoch content_grant_epoch event_id output candidate_generation "
        "presentation_epoch target_id target_generation action monotonic_usec",
        "", {"status": {"issued", "acknowledged"}, "disposition": {"0", "1", "2"}},
    ),
    "sophia_shell_action_cause": (
        "connection_epoch event_id output action activation_serial policy_connection_epoch",
        "activation_serial policy_connection_epoch",
        {"admission": {"Admitted", "Duplicate", "RejectedCapacity"}},
    ),
    "sophia_shell_action_policy": (
        "policy_connection_epoch activation_serial action transaction request_id indicator_generation",
        "", {"outcome": {"Committed", "RejectedInvalid", "RejectedStale", "TimedOut", "Disconnected"}},
    ),
    "sophia_shell_indicator_state": (
        "connection_epoch indicator_generation output indicator action slot state_bits entries",
        "action slot state_bits", {},
    ),
    "sophia_shell_native_binding": (
        "connection_epoch content_grant_epoch output candidate_generation native_owner "
        "native_frame head target_generation heads", "", {},
    ),
    "sophia_shell_native_completion": (
        "output native_owner native_frame heads monotonic_usec", "",
        {"timestamp_source": {"kernel", "observation_fallback"},
         "missing_kernel_timestamp": {"0", "1"}},
    ),
}
CLIENT = {
    "lom_panel_candidate": (
        "connection_epoch content_grant_epoch output candidate_generation presentation_epoch "
        "indicator_generation width height bytes", "",
        {"status": {"presented"}, "checksum": None},
    ),
}


def unsigned(text, *, zero=False):
    if len(text) > 20 or not re.fullmatch(r"[0-9]+", text):
        raise InvalidEvidence("not a bounded unsigned integer")
    value = int(text)
    if not (0 if zero else 1) <= value <= U64:
        raise InvalidEvidence("integer outside its range")
    return value


def decode(line, specs):
    # Host capture has sequence/time fields before the approved record.
    line = line.split("\t", 3)[-1] if "\t" in line else line
    words = line.split()
    if not words or words[0] not in specs:
        return None
    name = words[0]
    fields = {}
    for token in words[1:]:
        if token.count("=") != 1:
            raise InvalidEvidence(f"malformed {name} field")
        key, value = token.split("=")
        if key in fields:
            raise InvalidEvidence(f"duplicate {name}.{key}")
        fields[key] = value
    numbers, zero, enums = specs[name]
    if set(fields) != {"schema", *numbers.split(), *enums}:
        raise InvalidEvidence(f"missing or unknown {name} fields")
    if fields.pop("schema") != "1":
        raise InvalidEvidence(f"unsupported {name} schema")
    for field in numbers.split():
        fields[field] = unsigned(fields[field], zero=field in zero.split())
    for field, allowed in enums.items():
        if field == "checksum":
            if not re.fullmatch(r"[0-9a-f]{16}", fields[field]):
                raise InvalidEvidence("invalid pixel checksum")
        elif fields[field] not in allowed:
            raise InvalidEvidence(f"invalid {name}.{field}")
    return name, fields


def read(path, specs):
    with Path(path).open("rb") as stream:
        data = stream.read(MAX_LOG_BYTES + 1)
    if len(data) > MAX_LOG_BYTES:
        raise InvalidEvidence("log exceeds evidence byte budget")
    result = {name: [] for name in specs}
    count = 0
    for line in data.decode("utf-8", errors="strict").splitlines():
        if len(line) > 16384:
            raise InvalidEvidence("record exceeds evidence line budget")
        record = decode(line, specs)
        if record is not None:
            count += 1
            if count > MAX_RECORDS:
                raise InvalidEvidence("log exceeds evidence record budget")
            name, fields = record
            result[name].append(fields)
    return result

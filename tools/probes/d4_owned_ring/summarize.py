#!/usr/bin/env python3
"""Validate local probe cells and report medians; never a native gate analyzer."""
import argparse
import itertools
import json
from collections import defaultdict
from pathlib import Path
from statistics import median


def summarize(path):
    records = [json.loads(line) for line in path.read_text().splitlines()]
    config = records.pop(0)
    if config != {"event": "config", "warmup": 32, "iterations": 128,
                  "slots": 3, "reverse": config.get("reverse"),
                  "native": False, "cross_process": False}:
        raise ValueError(f"{path}: unexpected probe configuration")
    groups = defaultdict(list)
    setups, drains = {}, {}
    for record in records:
        key = (record["bytes"], record["retain"], record["arm"])
        event = record["event"]
        if event == "sample":
            groups[key].append(record)
        elif event in ("setup", "drained"):
            target = setups if event == "setup" else drains
            if key in target:
                raise ValueError(f"{path}: duplicate {event}: {key}")
            target[key] = record
        else:
            raise ValueError(f"{path}: unknown record: {event}")
    expected = set(itertools.product((184320, 4194304), (1, 4),
                                    ("fresh", "mmap_copy", "pread_sealed", "pread_unsealed")))
    if not (set(groups) == set(setups) == set(drains) == expected):
        raise ValueError(f"{path}: missing or extra cells")
    result = []
    for (size, retain, arm), rows in sorted(groups.items()):
        if [row["iteration"] for row in rows] != list(range(128)):
            raise ValueError(f"{path}: incomplete/duplicated/out-of-order samples")
        if drains[(size, retain, arm)]["remaining_resource_bytes"] != 0:
            raise ValueError(f"{path}: incomplete drain")
        summary = {"run": path.name, "bytes": size, "retain": retain, "arm": arm}
        for field in ("producer", "session", "copy", "allocate", "generation", "scan"):
            values = [row[field + "_ns"] + (row["release_ns"] if field == "session" else 0)
                      for row in rows]
            summary[field + "_us"] = median(values) / 1000
        summary["minor_faults_median"] = median(row["minor_faults"] for row in rows)
        summary["major_faults"] = sum(row["major_faults"] for row in rows)
        result.append(summary)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("captures", type=Path, nargs="+")
    args = parser.parse_args()
    print(json.dumps([row for path in sorted(args.captures) for row in summarize(path)], indent=2))

#!/usr/bin/env python3
"""Compare periodic render-work counters within one ordered session log.

No sampling or process access. A decreasing counter or uptime starts a new
baseline. Context replacement can also reset counters; intervals crossing an
unobserved replacement are not suitable for acceptance.
"""
import argparse
import json
import re
from pathlib import Path

PREFIX = "sophia_live_render_work "
TOTALS = ("composition_full_frames_count", "composition_partial_frames_count",
          "composition_repaint_pixels_count", "composition_target_pixels_count")
REASONS = ("no_table", "disabled", "unknown_age", "no_history", "beyond_history",
           "damage_unavailable", "plan")
COHORT = ("frames", "full", "partial", "repaint_pixels", "target_pixels")
DETAILS = tuple("damage_full_" + name + "_count" for name in REASONS) + tuple(
    "damage_stable_geometry_" + name + "_count" for name in COHORT)


def ratio(numerator, denominator):
    return numerator / denominator if denominator else None


def analyze(lines):
    previous = None
    intervals = []
    resets = []
    for ordinal, line in enumerate(lines, 1):
        if PREFIX not in line:
            continue
        fields = {}
        for field in line.split(PREFIX, 1)[1].split():
            key, sep, value = field.partition("=")
            if key in (*TOTALS, *DETAILS, "uptime_msec"):
                if key in fields or not sep or not re.fullmatch(r"[0-9]+", value):
                    raise ValueError(f"line {ordinal}: invalid or duplicate {key}")
                fields[key] = int(value)
        for key in (*TOTALS, "uptime_msec"):
            if key not in fields:
                raise ValueError(f"line {ordinal}: missing {key}")
        if any(key in fields for key in DETAILS) and not all(key in fields for key in DETAILS):
            raise ValueError(f"line {ordinal}: incomplete damage counters")
        if previous is not None:
            old_ordinal, old = previous
            keys = set(fields) - {"uptime_msec"}
            if keys != set(old) - {"uptime_msec"}:
                resets.append({"line": ordinal, "reason": "counter_set_changed"})
            elif fields["uptime_msec"] <= old["uptime_msec"] or any(fields[k] < old[k] for k in keys):
                resets.append({"line": ordinal, "reason": "counter_or_time_reset"})
            else:
                delta = {key: fields[key] - old[key] for key in keys}
                full, partial, pixels, target = (delta[key] for key in TOTALS)
                item = {
                    "from_line": old_ordinal, "to_line": ordinal,
                    "elapsed_msec": fields["uptime_msec"] - old["uptime_msec"],
                    "frames": full + partial, "full": full, "partial": partial,
                    "full_fraction": ratio(full, full + partial),
                    # Rectangles can overlap: this is summed drawing area,
                    # not the area of the union or measured GPU work.
                    "repaint_area_ratio": ratio(pixels, target),
                }
                if all(key in delta for key in DETAILS):
                    reasons = {name: delta["damage_full_" + name + "_count"] for name in REASONS}
                    cohort = {name: delta["damage_stable_geometry_" + name + "_count"] for name in COHORT}
                    if sum(reasons.values()) != full or cohort["full"] + cohort["partial"] != cohort["frames"]:
                        raise ValueError(f"line {ordinal}: repaint counters do not conserve frames")
                    if cohort["full"] > full or cohort["partial"] > partial:
                        raise ValueError(f"line {ordinal}: geometry cohort exceeds all frames")
                    cohort["full_fraction"] = ratio(cohort["full"], cohort["frames"])
                    cohort["repaint_area_ratio"] = ratio(cohort["repaint_pixels"], cohort["target_pixels"])
                    item.update(full_reasons=reasons, stable_geometry=cohort)
                intervals.append(item)
        previous = ordinal, fields
    if previous is None:
        raise ValueError("no render-work records")
    return {"intervals": intervals, "resets": resets}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path, help="one session's records in chronological order")
    args = parser.parse_args()
    try:
        with args.log.open() as source:
            result = analyze(source)
    except (OSError, ValueError) as error:
        parser.exit(1, f"render-work: {error}\n")
    print(json.dumps(result, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()

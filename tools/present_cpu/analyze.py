#!/usr/bin/env python3
"""Validate a fixed Present workload before reporting CPU or regression results."""
import math
import statistics

from proc_delta import cpu_delta, guest_delta, guest_process_delta

ACCOUNTING_METHOD = "guest-capacity-idle-v1/task-schedstat-v1"
CLOCK_FIELDS = {
    "owner_tid",
    "queries", "completions", "wire_bound", "wire_hardware_bound", "unclocked_bound",
    "wire_executions", "wire_execution_wait_usec", "owner_passes", "owner_waits",
    "owner_ring_ready", "owner_fd_ready", "owner_wait_deadlines", "native_ready",
    "native_ready_consumed", "native_ready_idle", "native_errors", "native_event_waits",
    "native_short_waits", "native_service_waits", "admission_errors", "admission_fake_retries",
}
FULL_REASONS = {
    "damage_full_" + reason + "_count" for reason in
    ("no_table", "disabled", "unknown_age", "no_history", "beyond_history",
     "damage_unavailable", "plan")
}
RENDER_FIELDS = FULL_REASONS | {
    "composition_full_frames_count", "composition_partial_frames_count",
    "composition_repaint_pixels_count", "composition_target_pixels_count",
    "damage_stable_geometry_frames_count", "damage_stable_geometry_full_count",
    "damage_stable_geometry_partial_count", "capture_context_creations_count",
    "pipeline_creations_count", "capture_failures_count", "transfer_failures_count",
    "snapshot_captures_count", "capture_surface_creations_count",
    "import_cache_imports_count", "import_cache_hits_count",
}


def interval(text, prefix, start, end, required):
    rows = []
    for line in text.splitlines():
        if not line.startswith(prefix + " "):
            continue
        fields = dict(word.split("=", 1) for word in line.split()[1:] if "=" in word)
        if "observed_monotonic_usec" not in fields:
            raise ValueError(f"{prefix}: missing measurement timestamp")
        row = {k: int(v) for k, v in fields.items()}
        if start <= row["observed_monotonic_usec"] <= end:
            if not required <= row.keys():
                raise ValueError(f"{prefix}: missing fields {sorted(required - row.keys())}")
            rows.append(row)
    if len(rows) < 2:
        raise ValueError(f"{prefix}: need two records inside workload")
    keys = rows[0].keys() - {"schema", "observed_monotonic_usec", "timing_enabled", "owner_tid",
                           "wire_execution_wait_max_usec"}
    for a, b in zip(rows, rows[1:]):
        if a.get("owner_tid") != b.get("owner_tid"):
            raise ValueError("owner thread changed during counter interval")
        if a["observed_monotonic_usec"] >= b["observed_monotonic_usec"]:
            raise ValueError(f"{prefix}: unordered timestamps")
        if not keys <= b.keys() or any(b[k] < a[k] for k in keys):
            raise ValueError(f"{prefix}: counter reset or missing field")
    delta = {k: rows[-1][k] - rows[0][k] for k in keys}
    if "owner_tid" in rows[0]:
        delta["owner_tid"] = rows[0]["owner_tid"]
    return (delta,
            (rows[-1]["observed_monotonic_usec"] - rows[0]["observed_monotonic_usec"]) / 1e6)


def percentile(values, fraction):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * fraction) - 1)] if values else None


def analyze(work, text):
    """Malformed, incomplete, reset or changed workloads fail closed."""
    try:
        return _analyze(work, text)
    except (KeyError, ValueError, TypeError, IndexError, ZeroDivisionError, StopIteration, AttributeError) as error:
        return {"status": "INVALID", "benefit": "unmeasured", "error": str(error)}


def _analyze(work, text):
    checks = []

    def check(name, condition):
        checks.append({"name": name, "pass": bool(condition)})

    if work["status"] != "complete":
        raise ValueError("workload did not complete")
    start, end = work["measure_start_usec"], work["measure_end_usec"]
    config = work["config"]
    if work.get("schema", 1) == 2:
        check("damage_workload", config["pattern"] == "fixed_patch_v1" and
              config["size"] in ("small", "head") and
              config["damage"] in ("absent", "full", "patch") and
              (config["size"] != "head" or config["clients"] == 1))
    duration = (end - start) / 1e6
    check("sample_duration", config["seconds"] <= duration <= config["seconds"] + .5)
    clock, clock_span = interval(text, "sophia_present_clock_service", start, end, CLOCK_FIELDS)
    render, render_span = interval(text, "sophia_live_render_work", start, end, RENDER_FIELDS)
    # A ten-second probe can contain only two five-second records. A full
    # measurement must cover at least 80%; never call a sparse pair a minute.
    coverage = .4 if config["seconds"] == 10 else .8
    check("counter_span", min(clock_span, render_span) >= duration * coverage)
    check("native_owner", "sophia_live_native_head schema=2 status=ready " in text)
    check("normal_session", "sophia_live_session_mode schema=1 mode=normal " in text and
          "sophia_live_session_mode schema=1 mode=proof " not in text)
    check("startup_app_succeeded", "sophia_session_app schema=1 status=exited id=cpu "
          "source=startup exit_status=exit status: 0\n" in text)
    check("client_completed", "sophia_present_cpu schema=1 status=complete\n" in text)
    check("guest_kernel_health", not any(error in text for error in
          ("Kernel panic", "BUG: soft lockup", "rcu: INFO:", "watchdog: Watchdog detected hard LOCKUP")))
    check("no_fatal", "runtime_fatal" not in text)
    bound = clock["wire_bound"]
    hardware, unclocked = clock["wire_hardware_bound"], clock["unclocked_bound"]
    source = "hardware" if hardware == bound else "unclocked" if unclocked == bound else "mixed_or_fake"
    check("visible_source", bound > 0 and source != "mixed_or_fake")
    check("no_recovery", all(clock[k] == 0 for k in
                            ("admission_errors", "admission_fake_retries", "native_errors")))
    check("no_render_failure", render["capture_failures_count"] == render["transfer_failures_count"] == 0)
    check("steady_resources", render["capture_context_creations_count"] == render["pipeline_creations_count"] == 0)
    full = render["composition_full_frames_count"]
    partial = render["composition_partial_frames_count"]
    check("render_progress", full + partial > 0 and clock["completions"] > 0)
    check("full_reason_partition", sum(render[k] for k in FULL_REASONS) == full)
    check("stable_geometry_partition", render["damage_stable_geometry_frames_count"] ==
          render["damage_stable_geometry_full_count"] + render["damage_stable_geometry_partial_count"])
    check("repaint_area", 0 < render["composition_repaint_pixels_count"] <= render["composition_target_pixels_count"])

    windows = work["windows"]
    check("client_count", len(windows) == config["clients"] and 1 <= len(windows) <= 2)
    for i, window in enumerate(windows):
        expected = {k: window["initial"][k] for k in ("x", "y", "width", "height")}
        check(f"geometry_{i}", expected == window["before"] == window["after"])
        if work.get("schema", 1) == 2:
            check(f"source_pixel_checks_{i}", window["source_pixel_checks_before_grace"] == 9)
            check(f"changed_pixels_{i}", window["unchanged_presents"] == 0)
            head, patch = window["initial"]["head"], window["initial"]["patch"]
            layout = window["initial"]["output_layout"]
            check(f"output_layout_{i}", bool(layout) and head in layout and
                  layout == windows[0]["initial"]["output_layout"] and
                  all(h["width"] > 0 and h["height"] > 0 for h in layout))
            check(f"patch_{i}", patch == {"x": 40, "y": 40, "width": 120, "height": 120})
            check(f"head_bounds_{i}", expected["x"] >= head["x"] and expected["y"] >= head["y"] and
                  expected["x"] + expected["width"] <= head["x"] + head["width"] and
                  expected["y"] + expected["height"] <= head["y"] + head["height"])
            if config["size"] == "head":
                check(f"head_size_{i}", expected == {"x": head["x"] + 16, "y": head["y"] + 16,
                                                    "width": head["width"] - 32, "height": head["height"] - 32})
        sent = window["sent"]
        check(f"events_{i}", sent > 0 and sent == window["completed"] == window["idle"] and
              window["outstanding_at_exit"] == window["unexpected_events"] == window["msc_regressions"] == 0)
        pairs = window["complete_ust_msc"]
        check(f"clock_{i}", len(pairs) == sent and all(a[0] <= b[0] and a[1] <= b[1]
                                                     for a, b in zip(pairs, pairs[1:])))
        check(f"mode_counts_{i}", len(window["modes_copy_flip_skip_suboptimal"]) == 4 and
              sum(window["modes_copy_flip_skip_suboptimal"]) == sent)
        check(f"latencies_{i}", len(window["send_to_complete_usec"]) == sent and
              all(v >= 0 for v in window["send_to_complete_usec"]))
        check(f"starvation_{i}", window["starvation"] == 0)
        if config["mode"] == "open":
            check(f"fixed_offers_{i}", window["offered"] == sent ==
                  work["expected_offers_per_window"] == config["seconds"] * config["rate_per_window"]
                  and window["late_slots"] == 0)
    rects = [w["before"] for w in windows]
    overlap = any(max(a["x"], b["x"]) < min(a["x"] + a["width"], b["x"] + b["width"])
                  and max(a["y"], b["y"]) < min(a["y"] + a["height"], b["y"] + b["height"])
                  for i, a in enumerate(rects) for b in rects[i+1:])
    check("same_head_disjoint", len({w["initial"]["crtc"] for w in windows}) == 1 and not overlap)
    before, after = work["proc_before"], work["proc_after"]
    for name, snapshot in (("before", before), ("after", after)):
        memory = {line.split()[0].rstrip(":"): int(line.split()[1])
                  for line in snapshot["guest_meminfo"].splitlines()}
        check("guest_memory_" + name, memory["MemAvailable"] >= 256 * 1024)
        check("guest_log_bound_" + name, 0 <= snapshot["guest_log_bytes"] <= 128 * 1024 * 1024)
    cpu_span = (after["monotonic_usec"] - before["monotonic_usec"]) / 1e6
    check("cpu_brackets_workload", before["monotonic_usec"] <= start < end <= after["monotonic_usec"]
          and cpu_span - duration < .5)
    cpu = {name: cpu_delta(before[name], after[name], cpu_span) for name in ("session", "client")}
    hz = before["session"]["clock_ticks_per_second"]
    guest_processes = guest_process_delta(before.get("guest_processes"), after.get("guest_processes"),
                                         hz, before["monotonic_usec"])
    if guest_processes is None:
        raise ValueError("missing guest process attribution")
    guest = guest_delta(before["guest_stat"], after["guest_stat"],
                        before["guest_schedstat"], after["guest_schedstat"], cpu_span, hz,
                        guest_processes["runtime_seconds"],
                        cpu["session"]["cpu_seconds"] + cpu["client"]["cpu_seconds"])
    check("guest_accounting", guest["accounting_consistent"])
    check("guest_steal", max(guest["steal_fraction"], guest["max_cpu_steal_fraction"]) <= .01)
    check("guest_unaccounted", guest["unaccounted_share"] <= .20)
    completed = work["completed_at_measure_end"]
    check("interval_completions", len(completed) == len(windows) and
          all(0 < n <= w["completed"] for n, w in zip(completed, windows)))
    count = sum(completed)
    latencies = [n for w in windows for n in w["send_to_complete_usec"]]
    owner = next(t for t in cpu["session"]["threads"] if t["tid"] == clock["owner_tid"])
    metrics = {
        "cpu_ms_per_complete": cpu["session"]["cpu_seconds"] * 1000 / count,
        "owner_cpu_ms_per_complete": owner["runtime_ns"] / 1e6 / count,
        "client_cpu_ms_per_complete": cpu["client"]["cpu_seconds"] * 1000 / count,
        "guest_cpu_ms_per_complete": guest["busy_seconds"] * 1000 / count,
        "guest_unaccounted_share": guest["unaccounted_share"],
        "guest_outside_target_share": guest["outside_target_share"],
        "guest_steal_fraction": guest["steal_fraction"],
        "guest_max_cpu_steal_fraction": guest["max_cpu_steal_fraction"],
        "completions_per_second": count / duration,
        "owner_voluntary_switches_per_complete": owner["voluntary_switches"] / count,
        "wire_execution_wait_mean_usec": clock["wire_execution_wait_usec"] / clock["wire_executions"],
        "repaint_area_ratio": render["composition_repaint_pixels_count"] / render["composition_target_pixels_count"],
    }
    for key in ("owner_passes", "owner_waits", "owner_wait_deadlines", "owner_ring_ready",
                "native_ready", "native_ready_idle", "native_event_waits", "native_short_waits"):
        metrics[key + "_per_complete"] = clock[key] / clock["completions"]
    metrics.update({"latency_" + p + "_usec": percentile(latencies, q)
                    for p, q in (("p50", .5), ("p95", .95), ("p99", .99))})
    return {"schema": 1, "status": "VALID" if all(c["pass"] for c in checks) else "INVALID",
            "accounting_method": ACCOUNTING_METHOD,
            "checks": checks, "config": config, "source": source, "cpu": cpu, "guest": guest, "metrics": metrics,
            "output_layout": windows[0]["initial"].get("output_layout"),
            "guest_processes": guest_processes,
            "guest_memory": {name: {"meminfo": snapshot["guest_meminfo"],
                                     "log_bytes": snapshot["guest_log_bytes"]}
                             for name, snapshot in (("before", before), ("after", after))},
            "completed_in_interval": count, "sent_cohort": sum(w["sent"] for w in windows),
            "mode_mix": [sum(w["modes_copy_flip_skip_suboptimal"][i] for w in windows) for i in range(4)],
            "counters": clock, "render": render, "cpu_interval_seconds": cpu_span,
            "counter_interval_seconds": clock_span, "render_interval_seconds": render_span,
            "scope": "Core pixmaps; guest-relative CPU; no W1 or live acceptance. Context switches are proxies."}


def summarize(reports):
    if not reports or any(r["status"] != "VALID" for r in reports):
        return {"status": "INVALID", "benefit": "unmeasured"}
    keys = ("mode", "target", "rate_per_window", "clients", "seconds", "grace_seconds", "buffer_kind",
            "size", "damage", "pattern")
    # Earlier archived samples used alternating full backgrounds. Retain their
    # identity, but never compare them with the new fixed-patch pixel stream.
    configs = [{"size": "small", "damage": "absent", "pattern": "alternating_background_v1",
                **r["config"]} for r in reports]
    first = reports[0]
    if any(r["source"] != first["source"] or r.get("output_layout") != first.get("output_layout") or
           r.get("accounting_method") != ACCOUNTING_METHOD or
           any(config[k] != configs[0][k] for k in keys)
           for r, config in zip(reports, configs)):
        return {"status": "INVALID", "error": "workload or clock source changed"}
    mode_set = [n > 0 for n in first["mode_mix"]]
    if any([n > 0 for n in r["mode_mix"]] != mode_set for r in reports):
        return {"status": "INVALID", "error": "completion mode set changed"}
    return {"status": "VALID", "runs": len(reports), "source": first["source"],
            "output_layout": first.get("output_layout"),
            "accounting_method": ACCOUNTING_METHOD,
            "config": {k: configs[0][k] for k in keys},
            "mode_set": mode_set, "mode_counts": [r["mode_mix"] for r in reports],
            "metrics": {k: {"median": statistics.median(r["metrics"][k] for r in reports),
                            "min": min(r["metrics"][k] for r in reports),
                            "max": max(r["metrics"][k] for r in reports)} for k in first["metrics"]}}


def compare(baseline, candidate):
    """Gate thresholds were chosen before measurement; validity is separate."""
    if (baseline["status"] != "VALID" or candidate["status"] != "VALID" or
            min(baseline["runs"], candidate["runs"]) < 3 or
            baseline["config"] != candidate["config"] or baseline["source"] != candidate["source"] or
            baseline.get("output_layout") != candidate.get("output_layout") or
            baseline["mode_set"] != candidate["mode_set"] or
            baseline.get("accounting_method") != ACCOUNTING_METHOD or
            candidate.get("accounting_method") != ACCOUNTING_METHOD):
        return {"status": "INVALID", "reason": "need three valid matched runs per revision"}
    if (baseline["config"]["mode"] == "open" and
            baseline["mode_counts"] != candidate["mode_counts"]):
        return {"status": "INVALID", "reason": "fixed-rate completion counts or modes changed"}
    changes = {}
    for key, a in baseline["metrics"].items():
        b = candidate["metrics"][key]
        changes[key] = {"baseline": a, "candidate": b,
                        "separated_ranges": "increase" if b["min"] > a["max"] else
                            "decrease" if b["max"] < a["min"] else "overlap",
                        "relative_change": b["median"] / a["median"] - 1 if a["median"] else None}
    cpu = changes["cpu_ms_per_complete"]["relative_change"]
    throughput = changes["completions_per_second"]["relative_change"]
    latency = changes["latency_p95_usec"]["relative_change"]
    guest = changes["guest_cpu_ms_per_complete"]["relative_change"]
    unaccounted_growth = (candidate["metrics"]["guest_unaccounted_share"]["median"] -
                          baseline["metrics"]["guest_unaccounted_share"]["median"])
    if unaccounted_growth > .05:
        return {"status": "INVALID", "reason": "unaccounted guest CPU share grew by over five percentage points",
                "metrics": changes}
    outside_growth = (candidate["metrics"]["guest_outside_target_share"]["median"] -
                      baseline["metrics"]["guest_outside_target_share"]["median"])
    if outside_growth > .05:
        return {"status": "INVALID", "reason": "CPU share outside Session/client grew by over five percentage points",
                "metrics": changes}
    regression = (cpu is None or guest is None or throughput is None or latency is None or
                  cpu > .10 or guest > .10 or throughput < -.02 or latency > .10)
    return {"status": "REGRESSION" if regression else "PASS", "metrics": changes,
            "saving_observed": (cpu is not None and guest is not None and cpu < 0 and guest < 0 and
                                changes["cpu_ms_per_complete"]["separated_ranges"] == "decrease" and
                                changes["guest_cpu_ms_per_complete"]["separated_ranges"] == "decrease"),
            "thresholds": {"cpu_increase": .10, "throughput_loss": .02, "p95_latency_increase": .10},
            "scope": "Investigate threshold crossings; three runs are a relative signal, not live acceptance."}

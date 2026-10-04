"""Process identity and per-thread accounting for an explicitly sampled PID."""

def guest_process_delta(before, after, hz, start_usec):
    """Diagnostic attribution only; never silently credit unseen process lifetime."""
    if before is None or after is None:
        return None
    key = lambda row: (row["pid"], row["start_ticks"])
    a, b = {key(row): row for row in before}, {key(row): row for row in after}
    if len(a) != len(before) or len(b) != len(after) or hz <= 0:
        raise ValueError("invalid guest process identities")
    rows = []
    for identity in a.keys() & b.keys():
        ticks = [b[identity][k] - a[identity][k] for k in ("user_ticks", "system_ticks")]
        if min(ticks) < 0:
            raise ValueError("guest process CPU counter reset")
        tasks_a = {(t["tid"], t["start_ticks"]): t["runtime_ns"] for t in a[identity]["tasks"]}
        tasks_b = {(t["tid"], t["start_ticks"]): t["runtime_ns"] for t in b[identity]["tasks"]}
        if tasks_a.keys() - tasks_b.keys():
            raise ValueError("guest task exited during measurement")
        runtime = 0
        for task, ns in tasks_b.items():
            if task not in tasks_a and task[1] < start_usec * hz // 1_000_000:
                raise ValueError("newly observed task predates guest snapshot")
            delta = ns - tasks_a.get(task, 0)
            if delta < 0:
                raise ValueError("guest task runtime reset")
            runtime += delta
        rows.append({"pid": identity[0], "start_ticks": identity[1],
                     "name_before": a[identity]["name"], "name": b[identity]["name"],
                     "cpu_seconds": sum(ticks) / hz, "runtime_seconds": runtime / 1e9})
    if a.keys() - b.keys():
        raise ValueError("guest process exited during measurement")
    for identity in b.keys() - a.keys():
        if identity[1] < start_usec * hz // 1_000_000:
            raise ValueError("newly observed process predates guest snapshot")
        row = b[identity]
        rows.append({"pid": identity[0], "start_ticks": identity[1], "name": row["name"],
                     "cpu_seconds": (row["user_ticks"] + row["system_ticks"]) / hz,
                     "runtime_seconds": sum(t["runtime_ns"] for t in row["tasks"]) / 1e9})
    return {"matched": sorted(rows, key=lambda row: (-row["cpu_seconds"], row["pid"])),
            "runtime_seconds": sum(row["runtime_seconds"] for row in rows),
            "appeared": sorted(b.keys() - a.keys()), "disappeared": sorted(a.keys() - b.keys())}


def cpu_delta(before, after, elapsed):
    if elapsed <= 0:
        raise ValueError("nonpositive CPU interval")
    if before is None or after is None:
        raise ValueError("missing explicitly sampled process")
    for key in ("pid", "start_ticks", "exe", "clock_ticks_per_second"):
        if before[key] != after[key]:
            raise ValueError(f"process identity changed: {key}")
    a = {t["tid"]: t for t in before["threads"]}
    b = {t["tid"]: t for t in after["threads"]}
    if a.keys() != b.keys():
        raise ValueError("thread set changed within measurement")
    threads = []
    for tid in a:
        if (a[tid]["start_ticks"], a[tid]["name"]) != (b[tid]["start_ticks"], b[tid]["name"]):
            raise ValueError("thread identity changed")
        delta = {k: b[tid][k] - a[tid][k] for k in
                 ("runtime_ns", "runqueue_ns", "voluntary_switches", "involuntary_switches")}
        if min(delta.values()) < 0:
            raise ValueError("thread counter reset")
        threads.append({"tid": tid, "name": a[tid]["name"],
                        **delta, "one_core_percent": delta["runtime_ns"] / elapsed / 1e7})
    deltas = [after[k] - before[k] for k in ("user_ticks", "system_ticks")]
    if min(deltas) < 0 or before["clock_ticks_per_second"] <= 0:
        raise ValueError("process counter reset")
    ticks = sum(deltas)
    tick_seconds = ticks / before["clock_ticks_per_second"]
    cpu_seconds = sum(t["runtime_ns"] for t in threads) / 1e9
    return {"cpu_seconds": cpu_seconds, "tick_seconds": tick_seconds,
            "one_core_percent": cpu_seconds / elapsed * 100,
            "threads": threads}


def guest_delta(before, after, sched_before, sched_after, elapsed, hz, task_seconds, target_seconds):
    def parse(raw):
        rows = {parts[0]: [int(n) for n in parts[1:9]] for line in raw.splitlines()
                if (parts := line.split()) and
                (parts[0] == "cpu" or parts[0].startswith("cpu") and parts[0][3:].isdigit())}
        if "cpu" not in rows or len(rows) < 2 or any(len(v) < 8 for v in rows.values()):
            raise ValueError("guest CPU accounting incomplete")
        return rows
    a, b = parse(before), parse(after)
    if a.keys() != b.keys() or hz <= 0 or elapsed <= 0:
        raise ValueError("guest CPU set changed or counters reset")
    deltas = {name: [y - x for x, y in zip(a[name], b[name])] for name in a}
    if any(min(row) < 0 for row in deltas.values()):
        raise ValueError("guest CPU counters reset")
    delta, cpus = deltas["cpu"], len(deltas) - 1
    per_cpu_steal = {name: row[7] / hz / elapsed for name, row in deltas.items() if name != "cpu"}
    def runtimes(raw):
        rows = [line.split() for line in raw.splitlines() if line.split()]
        if rows[0] not in (["version", "15"], ["version", "16"], ["version", "17"]):
            raise ValueError("unsupported guest schedstat version")
        return {row[0]: int(row[7]) for row in rows if row[0] in per_cpu_steal and len(row) == 10}
    sa, sb = runtimes(sched_before), runtimes(sched_after)
    if sa.keys() != sb.keys() or sa.keys() != per_cpu_steal.keys() or any(sb[k] < sa[k] for k in sa):
        raise ValueError("guest scheduler CPU set changed or runtime reset")
    scheduled = sum(sb[k] - sa[k] for k in sa) / 1e9
    interrupts = (delta[5] + delta[6]) / hz
    capacity_busy = elapsed * cpus - (delta[3] + delta[4] + delta[7]) / hz
    # rq_cpu_time uses rq_clock, so it includes IRQ that interrupts a running
    # task. Adding all IRQ would count that twice. Idle/steal subtraction is
    # independent; cross-check it against the bounded IRQ overlap in rq time.
    busy = capacity_busy
    unaccounted = busy - task_seconds - interrupts
    # Both snapshots round idle, iowait and steal separately on every CPU.
    rounding = 2 * cpus * 3 / hz
    tolerance = max(rounding, .05 * busy)
    return {"cpus": cpus, "busy_seconds": busy,
            "scheduled_seconds": scheduled, "task_seconds": task_seconds,
            "capacity_minus_idle_seconds": capacity_busy,
            "consistency_tolerance_seconds": tolerance,
            "tick_busy_seconds": sum(delta[i] for i in (0, 1, 2, 5, 6)) / hz,
            "outside_target_share": max(0, busy - target_seconds) / busy if busy else 0,
            "field_seconds": {name: ticks / hz for name, ticks in zip(
                ("user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal"), delta)},
            "unaccounted_seconds": unaccounted,
            "unaccounted_share": max(0, unaccounted) / busy if busy else 0,
            "steal_fraction": delta[7] / hz / elapsed / cpus,
            "per_cpu_steal_fraction": per_cpu_steal,
            "max_cpu_steal_fraction": max(per_cpu_steal.values()),
            "iowait_seconds": delta[4] / hz,
            "total_seconds": sum(delta) / hz,
            "accounting_consistent": (0 < busy <= elapsed * cpus and scheduled > 0 and
                                      unaccounted >= -rounding and
                                      scheduled - tolerance <= busy <= scheduled + interrupts + tolerance)}

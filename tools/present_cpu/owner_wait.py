"""Attribute actual owner waits; queued authority items are not waits."""

REASONS = (
    "maintenance input input_receipts frames topology seat shell_interaction lifecycle "
    "cursor controls proof frame_deadline present native_deadline shortcut shell_output "
    "pacer service"
).split()
FIELDS = {f"{kind}_{reason}" for kind in ("selected", "expired", "pending")
          for reason in REASONS} | {"owner_tid"}


def attribute(text, start, end):
    from analyze import interval

    waits, span = interval(text, "sophia_owner_wait", start, end, FIELDS)
    clock, _ = interval(text, "sophia_present_clock_service", start, end,
                        {"owner_tid", "owner_waits", "owner_wait_deadlines", "completions"})
    times = []
    for prefix in ("sophia_owner_wait", "sophia_present_clock_service"):
        rows = []
        for line in text.splitlines():
            if line.startswith(prefix + " "):
                fields = dict(word.split("=", 1) for word in line.split()[1:] if "=" in word)
                stamp = int(fields["observed_monotonic_usec"])
                if start <= stamp <= end:
                    rows.append(stamp)
        times.append(rows)
    if times[0] != times[1] or waits["owner_tid"] != clock["owner_tid"]:
        raise ValueError("owner wait records have different times or owners")
    selected = {reason: waits["selected_" + reason] for reason in REASONS}
    expired = {reason: waits["expired_" + reason] for reason in REASONS}
    pending = {reason: waits["pending_" + reason] for reason in REASONS}
    if sum(selected.values()) != clock["owner_waits"]:
        raise ValueError("wait reasons do not partition actual waits")
    if sum(expired.values()) != clock["owner_wait_deadlines"]:
        raise ValueError("expired reasons do not partition deadline returns")
    if any(expired[r] > selected[r] or pending[r] > clock["owner_waits"] for r in REASONS):
        raise ValueError("reason count exceeds its wait cohort")
    completed = clock["completions"]
    if completed <= 0:
        raise ValueError("no completions inside wait counter interval")
    return {"owner_tid": waits["owner_tid"], "span_seconds": span,
            "completions": completed, "selected": selected, "expired": expired,
            "pending": pending,
            "per_complete": {kind: {r: n / completed for r, n in counts.items()}
                             for kind, counts in (("selected", selected), ("expired", expired),
                                                  ("pending", pending))},
            "scope": "Selected reasons partition actual polls; equal caps keep the first reason. "
                     "Pending work overlaps and does not partition waits. Readiness can end any cap."}

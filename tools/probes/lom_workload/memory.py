"""Measured protocol/storage inventory, not RSS or GPU residency."""

from records import InvalidEvidence, SHUTDOWN_INVENTORY


def require(ok, reason):
    if not ok:
        raise InvalidEvidence(reason)


def verify_memory(host, grant, candidates, start, end):
    limits = host["sophia_shell_content_budget"]
    require(limits and all(row == limits[0] for row in limits), "missing or changed content budgets")
    cap = limits[0]
    require((cap["connection_epoch"], cap["content_grant_epoch"]) == grant,
            "content budget grant mismatch")
    sizes = {}
    for row in candidates.values():
        geometry = row["width"], row["height"], row["bytes"]
        require(sizes.setdefault(row["output"], geometry) == geometry,
                "allocation geometry changed during memory workload")
    # This declared workload has one panel and two reusable resource slots per
    # output. These are acceptance ceilings, not additions to server authority.
    slots = 2 * len(sizes)
    byte_bound = 2 * sum(value[2] for value in sizes.values())
    rows = host["sophia_shell_content_sample"]
    require(rows, "missing in-run content inventory")
    times = [row["monotonic_usec"] for row in rows]
    require(all(a < b for a, b in zip(times, times[1:])), "duplicate or regressing memory sample")
    before = [row for row in rows if row["monotonic_usec"] <= start]
    after = [row for row in rows if row["monotonic_usec"] >= end]
    require(before and after, "memory observations do not bracket workload")
    window = [before[-1], *[r for r in rows if start < r["monotonic_usec"] < end], after[0]]
    require(all(b["monotonic_usec"] - a["monotonic_usec"] <= 6_000_000
                for a, b in zip(window, window[1:])), "memory observation gap exceeds six seconds")
    warmed_ids = max(row["resource_ids"] for row in before)
    require(0 < warmed_ids <= slots, "invalid warmed resource-ID population")
    high = dict.fromkeys(SHUTDOWN_INVENTORY.split(), 0)
    for row in rows:
        require((row["connection_epoch"], row["content_grant_epoch"]) == grant,
                "memory sample grant mismatch")
        require(row["status"] == "active" and row["workers_joined"] == "0"
                and row["active_epochs"] == 1 and row["retired_epochs"] == 0
                and row["settled_candidates"] == 0, "unhealthy active content inventory")
        require(row["resources"] + row["transfers"] <= min(slots, cap["max_live_resources"])
                and row["resource_ids"] <= min(slots, cap["max_resource_ids"]),
                "resource slot or ID bound exceeded")
        if row["monotonic_usec"] >= start:
            require(row["resource_ids"] <= warmed_ids, "resource IDs grew after warmup")
        for field in ("staging", "resident", "retiring"):
            require(row[field + "_bytes"] <= cap["max_" + field + "_bytes"],
                    "negotiated storage bound exceeded")
        require(sum(row[f + "_bytes"] for f in ("staging", "resident", "retiring")) <= byte_bound
                and row["backing_bytes"] <= byte_bound,
                "two-slot pixel ownership bound exceeded")
        require(row["resident_bytes"] + row["reserved_resident_bytes"] <= cap["max_resident_bytes"],
                "reserved resident bound exceeded")
        require(row["reserved_bytes"] <= sum(cap["max_" + f + "_bytes"]
                                              for f in ("staging", "resident", "retiring"))
                and row["reserved_backing_bytes"] <= cap["max_resident_bytes"] + cap["max_retiring_bytes"],
                "epoch reservation bound exceeded")
        require(row["transfers"] <= cap["max_open_transfers"]
                and row["allocations"] <= min(len(sizes), cap["max_allocations_total"])
                and row["candidates"] <= slots
                and row["permits"] <= len(sizes) and row["demands"] <= len(sizes),
                "candidate/allocation/permit inventory bound exceeded")
        require(row["response_records"] <= cap["max_control_records"]
                and row["response_bytes"] <= cap["max_output_queue_bytes"]
                and row["input_bytes"] <= cap["max_input_queue_bytes"]
                and row["input_records"] * 24 <= cap["max_input_queue_bytes"],
                "transport inventory bound exceeded")
        for field in high:
            high[field] = max(high[field], row[field])
    return {"scope": "protocol_storage_inventory", "samples": len(rows),
            "workload_samples": len(window), "slot_bound": slots,
            "pixel_byte_bound": byte_bound, "warmed_resource_ids": warmed_ids,
            "high_water": high, "negotiated": cap}

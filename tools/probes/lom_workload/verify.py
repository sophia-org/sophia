#!/usr/bin/env python3
"""Verify exact action latency joins; not a replacement for the native gate."""

import argparse
import json
import math
from collections import defaultdict

from records import CLIENT, HOST, InvalidEvidence, read


def require(condition, reason):
    if not condition:
        raise InvalidEvidence(reason)


def key(row, names):
    return tuple(row[name] for name in names.split())


def index(rows, names, label, *, repeat=False):
    result = {}
    for row in rows:
        identity = key(row, names)
        require(identity not in result or (repeat and result[identity] == row),
                f"duplicate or contradictory {label}")
        result[identity] = row
    return result


def exactly_one(rows, label):
    require(len(rows) == 1, f"missing or ambiguous {label}")
    return rows[0]


def budgets(value):
    names = {"warmup_usec", "duration_usec", "actions_per_output", "output_count",
             "ack_p95_usec", "ack_max_usec", "native_p95_usec", "native_max_usec"}
    require(isinstance(value, dict) and set(value) == names, "missing or unknown workload budget")
    require(all(type(v) is int and 0 < v <= 300_000_000 for v in value.values()),
            "invalid workload budget")
    require(value["duration_usec"] == 60_000_000 and value["output_count"] == 2
            and value["actions_per_output"] == 20, "not the declared 60s/40-action workload")
    require(value["ack_p95_usec"] <= value["ack_max_usec"]
            and value["native_p95_usec"] <= value["native_max_usec"], "inverted latency budgets")
    return value


class Capture:
    def __init__(self, host, client):
        self.rows = host
        self.candidates = index(client["lom_panel_candidate"],
                                "connection_epoch content_grant_epoch output candidate_generation",
                                "Lom candidate")
        require(self.candidates, "no Lom candidate origin evidence")
        self.grant = exactly_one(list({identity[:2] for identity in self.candidates}), "grant")
        self.outputs = sorted({identity[2] for identity in self.candidates})
        self.states = index(host["sophia_shell_indicator_state"],
                            "connection_epoch indicator_generation output slot",
                            "indicator state", repeat=True)
        require(all(row["state_bits"] <= 15 and row["slot"] <= 65535
                    for row in self.states.values()), "invalid indicator state bits or slot")
        publications = defaultdict(list)
        for state in self.states.values():
            publications[key(state, "connection_epoch indicator_generation")].append(state)
        for rows in publications.values():
            require(len(rows) == rows[0]["entries"] <= 256
                    and all(row["entries"] == len(rows) for row in rows),
                    "incomplete indicator publication")
            for output in {row["output"] for row in rows}:
                entries = [row for row in rows if row["output"] == output]
                require(len(entries) <= 32 and sum(bool(row["state_bits"] & 1) for row in entries) == 1,
                        "workspace publication lacks one exact active slot")
        native = index(host["sophia_shell_native_completion"],
                       "native_owner output native_frame", "native completion", repeat=True)
        bindings = defaultdict(list)
        # Repeated identical rows are harmless; a conflicting head row is not.
        unique = index(host["sophia_shell_native_binding"],
                       "connection_epoch content_grant_epoch output candidate_generation head",
                       "candidate head binding", repeat=True)
        for binding in unique.values():
            require(key(binding, "connection_epoch content_grant_epoch") == self.grant,
                    "replaced native content grant")
            bindings[key(binding, "connection_epoch content_grant_epoch output candidate_generation")].append(binding)
        self.completed = {}
        self.mode_refresh = {}
        self.unqualified = 0
        topology = {}
        claimed_frames = set()
        for identity, candidate in self.candidates.items():
            require(candidate["bytes"] == candidate["width"] * candidate["height"] * 4,
                    "candidate pixel byte count mismatch")
            heads = bindings.get(identity, [])
            require(heads, "presented candidate has no owned native queue binding")
            first = heads[0]
            require(1 <= first["heads"] <= 4 and len(heads) == first["heads"],
                    "incomplete native head coverage")
            require(all(key(h, "native_owner native_frame heads") ==
                        key(first, "native_owner native_frame heads") for h in heads),
                    "candidate spans unrelated native frames")
            # Mode qualification is not measured cadence or a healthy-driver claim.
            require(all(60_000 <= h["mode_refresh_millihz"] <= (1 << 32) - 1 for h in heads),
                    "native head mode is below 60 Hz or has invalid refresh")
            layout = tuple(sorted(key(h, "native_owner head target_generation mode_refresh_millihz")
                                  for h in heads))
            old = topology.setdefault(identity[2], layout)
            require(old == layout, "native owner or topology changed during workload")
            self.mode_refresh[identity[2]] = {
                str(h["head"]): h["mode_refresh_millihz"] for h in heads
            }
            frame_key = key(first, "native_owner output native_frame")
            require(frame_key not in claimed_frames, "one native frame claimed by two candidates")
            claimed_frames.add(frame_key)
            completion = native.get(frame_key)
            if completion is None:
                # Logical primary publication can precede sibling convergence.
                # Keep this candidate unavailable; never borrow a later frame's time.
                self.unqualified += 1
                continue
            require(completion["heads"] == first["heads"], "native completion head count differs")
            if completion["timestamp_source"] != "kernel" or completion["missing_kernel_timestamp"] != "0":
                self.unqualified += 1
                continue
            self.completed[identity] = completion["monotonic_usec"]

    def candidate(self, output, generation):
        identity = (*self.grant, output, generation)
        require(identity in self.candidates, "issued action has no exact presented origin")
        return self.candidates[identity]

    def state(self, revision, output, *, action=None, slot=None):
        return exactly_one([
            row for row in self.states.values()
            if row["connection_epoch"] == self.grant[0]
            and row["indicator_generation"] == revision and row["output"] == output
            and (action is None or row["action"] == action)
            and (slot is None or row["slot"] == slot)
        ], "published indicator meaning")


def verify(host, client, workload):
    workload = budgets(workload)
    capture = Capture(host, client)
    require(len(capture.outputs) == workload["output_count"], "wrong workload output count")
    receipts = host["sophia_shell_action_receipt"]
    issued = index([r for r in receipts if r["status"] == "issued"],
                   "connection_epoch event_id", "issued event")
    acknowledged = index([r for r in receipts if r["status"] == "acknowledged"],
                         "connection_epoch event_id", "validated ACK")
    causes = index(host["sophia_shell_action_cause"], "connection_epoch event_id", "WM admission")
    policies = index(host["sophia_shell_action_policy"],
                     "policy_connection_epoch activation_serial", "policy outcome")
    require(len(issued) == workload["output_count"] * workload["actions_per_output"],
            "missing or extra intended workload actions")
    require(issued.keys() == acknowledged.keys() == causes.keys(),
            "missing or orphan ACK/WM outcome")
    start = min(r["monotonic_usec"] for r in issued.values())
    end = start + workload["duration_usec"]
    samples = defaultdict(list)
    claimed_policies = set()
    for event, issue in issued.items():
        require(key(issue, "connection_epoch content_grant_epoch") == capture.grant,
                "action belongs to a replaced grant")
        require(issue["output"] in capture.outputs and start <= issue["monotonic_usec"] < end,
                "action outside fixed workload window")
        require(issue["disposition"] == "0", "invalid issued-action disposition")
        ack = acknowledged[event]
        identity_fields = set(issue) - {"status", "disposition", "monotonic_usec"}
        require(all(ack[f] == issue[f] for f in identity_fields)
                and ack["disposition"] == "1", "ACK does not consume exact issued action")
        ack_usec = ack["monotonic_usec"] - issue["monotonic_usec"]
        require(ack_usec >= 0, "ACK precedes issuance")
        cause = causes[event]
        require(cause["admission"] == "Admitted" and cause["output"] == issue["output"]
                and cause["action"] == issue["action"], "action was not admitted to exact WM queue")
        policy_key = key(cause, "policy_connection_epoch activation_serial")
        require(policy_key not in claimed_policies, "WM admission replayed for another action")
        claimed_policies.add(policy_key)
        policy = policies.get(policy_key)
        require(policy is not None and policy["outcome"] == "Committed"
                and policy["action"] == issue["action"], "missing committed causal policy outcome")
        origin = capture.candidate(issue["output"], issue["candidate_generation"])
        require(origin["presentation_epoch"] == issue["presentation_epoch"],
                "action presentation epoch mismatch")
        before = capture.state(origin["indicator_generation"], issue["output"], action=issue["action"])
        require(before["state_bits"] & 1 == 0, "no-op action in state-changing workload")
        require(policy["indicator_generation"] > origin["indicator_generation"],
                "policy did not publish a newer indicator revision")
        after = capture.state(policy["indicator_generation"], issue["output"], slot=before["slot"])
        require(after["state_bits"] & 1 == 1, "requested workspace did not become active")
        matches = [(time, identity) for identity, time in capture.completed.items()
                   if identity[2] == issue["output"]
                   and capture.candidates[identity]["indicator_generation"] == policy["indicator_generation"]]
        require(matches, "no native candidate with originating committed indicator revision")
        completed, identity = min(matches)
        native_usec = completed - issue["monotonic_usec"]
        require(native_usec >= 0, "matching native candidate precedes action")
        samples[issue["output"]].append({
            "event_id": issue["event_id"], "ack_usec": ack_usec, "native_usec": native_usec,
            "activation_serial": policy["activation_serial"], "policy_transaction": policy["transaction"],
            "indicator_generation": policy["indicator_generation"], "candidate_generation": identity[3],
        })
    report = {}
    for output in capture.outputs:
        times = [t for identity, t in capture.completed.items() if identity[2] == output]
        require(times, "no qualified native completion on output")
        require(min(times) + workload["warmup_usec"] <= start, "warmup was not completed on every output")
        require(max(times) >= end, "capture lacks full workload duration on every output")
        values = samples[output]
        require(len(values) == workload["actions_per_output"], "missing per-output action samples")
        summary = {"count": len(values), "samples": values,
                   "mode_refresh_millihz_by_head": capture.mode_refresh[output]}
        for metric in ["ack", "native"]:
            ordered = sorted(v[f"{metric}_usec"] for v in values)
            p95 = ordered[math.ceil(len(ordered) * 0.95) - 1]
            maximum = ordered[-1]
            require(p95 <= workload[f"{metric}_p95_usec"]
                    and maximum <= workload[f"{metric}_max_usec"], f"{metric} latency budget exceeded")
            summary[f"{metric}_p95_usec"] = p95
            summary[f"{metric}_max_usec"] = maximum
        report[str(output)] = summary
    return {"schema": 1, "status": "pass", "scope": "causal_action_latency",
            "percentile_estimator": "nearest_rank", "window_start_usec": start,
            "window_end_usec": end, "budgets": workload, "outputs": report,
            "unqualified_candidates": capture.unqualified}


def unique_json_object(pairs):
    result = {}
    for name, value in pairs:
        require(name not in result, "duplicate workload budget")
        result[name] = value
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--client", required=True)
    parser.add_argument("--budgets", required=True)
    args = parser.parse_args()
    try:
        with open(args.budgets, encoding="utf-8") as source:
            data = source.read(16385)
            require(len(data) <= 16384, "workload budget file too large")
            workload = json.loads(data, object_pairs_hook=unique_json_object)
        result = verify(read(args.host, HOST), read(args.client, CLIENT), workload)
    except (OSError, ValueError, KeyError) as error:
        print(json.dumps({"schema": 1, "status": "fail", "reason": str(error)}))
        return 1
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

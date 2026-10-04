import copy
import unittest

from analyze import CLOCK_FIELDS, FULL_REASONS, RENDER_FIELDS, analyze, compare, summarize


def fixture(source="unclocked", mode="open"):
    process = {"pid": 42, "exe": "/test/sophia", "start_ticks": 1,
               "clock_ticks_per_second": 100, "user_ticks": 10, "system_ticks": 0,
               "threads": [{"tid": 42, "name": "sophia", "start_ticks": 1,
                            "runtime_ns": 100, "runqueue_ns": 0,
                            "voluntary_switches": 1, "involuntary_switches": 0}]}
    before = {"monotonic_usec": 0, "session": process, "client": copy.deepcopy(process)}
    before["client"]["pid"] = 43
    before["client"]["threads"][0]["tid"] = 43
    after = copy.deepcopy(before)
    after["monotonic_usec"] = 10_000_000
    for key in ("session", "client"):
        after[key]["user_ticks"] += 10
        after[key]["threads"][0]["runtime_ns"] += 100_000_000
    before["guest_stat"] = "cpu 100 0 0 1000 0 0 0 0 0 0\ncpu0 100 0 0 1000 0 0 0 0\n"
    after["guest_stat"] = "cpu 120 0 0 1980 0 0 0 0 0 0\ncpu0 120 0 0 1980 0 0 0 0\n"
    before["guest_schedstat"] = "version 17\ncpu0 0 0 0 0 0 0 200 0 0\n"
    after["guest_schedstat"] = "version 17\ncpu0 0 0 0 0 0 0 200000200 0 0\n"
    for sample in (before, after):
        sample["guest_meminfo"] = "MemAvailable: 512000 kB\n"
        sample["guest_log_bytes"] = 1024
        sample["guest_processes"] = [{
            "pid": p["pid"], "name": "test", "start_ticks": p["start_ticks"],
            "user_ticks": p["user_ticks"], "system_ticks": p["system_ticks"],
            "tasks": [{k: t[k] for k in ("tid", "start_ticks", "runtime_ns")} for t in p["threads"]],
        } for p in (sample["session"], sample["client"])]
    rect = {"x": 32, "y": 32, "width": 320, "height": 240}
    window = {"initial": {"window": 1, "crtc": 5, **rect}, "before": rect, "after": rect,
              "sent": 50, "completed": 50, "idle": 50, "outstanding_at_exit": 0,
              "unexpected_events": 0, "msc_regressions": 0, "starvation": 0,
              "offered": 50, "late_slots": 0, "complete_ust_msc": [[i*1000, 0] for i in range(50)],
              "send_to_complete_usec": [1000]*50, "modes_copy_flip_skip_suboptimal": [50, 0, 0, 0]}
    work = {"status": "complete", "config": {"mode": mode, "target": "next", "rate_per_window": 5,
            "clients": 1, "seconds": 10, "grace_seconds": 10, "buffer_kind": "core_pixmap_cpu"},
            "measure_start_usec": 0, "measure_end_usec": 10_000_000, "proc_before": before,
            "proc_after": after, "windows": [window], "expected_offers_per_window": 50,
            "completed_at_measure_end": [50]}
    log = "sophia_live_native_head schema=2 status=ready output=1\n"
    log += "sophia_live_session_mode schema=1 mode=normal configured_apps=1 startup_apps=1\n"
    log += "sophia_session_app schema=1 status=exited id=cpu source=startup exit_status=exit status: 0\n"
    log += "sophia_present_cpu schema=1 status=complete\n"
    for t, n in ((1_000_000, 10), (6_000_000, 20)):
        clock = dict.fromkeys(CLOCK_FIELDS, 0)
        clock.update(owner_tid=42, completions=n, wire_bound=n, wire_executions=n, wire_execution_wait_usec=n*1000,
                     owner_passes=n*2, owner_waits=n, native_event_waits=n, native_ready=n,
                     native_ready_consumed=n)
        clock["unclocked_bound" if source == "unclocked" else "wire_hardware_bound"] = n
        render = dict.fromkeys(RENDER_FIELDS, 0)
        render.update(composition_full_frames_count=n, damage_full_plan_count=n,
                      damage_stable_geometry_frames_count=n, damage_stable_geometry_full_count=n,
                      composition_repaint_pixels_count=n*100, composition_target_pixels_count=n*100)
        for prefix, fields in (("sophia_present_clock_service", clock), ("sophia_live_render_work", render)):
            fields.update(schema=1, observed_monotonic_usec=t)
            log += prefix + " " + " ".join(f"{k}={v}" for k, v in sorted(fields.items())) + "\n"
    return work, log


class Validity(unittest.TestCase):
    def test_proof_mode_failed_app_and_kernel_stall_are_invalid(self):
        for fault in ("proof", "app_exit", "missing_complete", "kernel_stall"):
            work, text = fixture()
            if fault == "proof": text = text.replace("mode=normal", "mode=proof")
            elif fault == "app_exit": text = text.replace("exit status: 0", "exit status: 1")
            elif fault == "missing_complete": text = text.replace("sophia_present_cpu schema=1 status=complete", "")
            else: text += "[100.00] watchdog: BUG: soft lockup - CPU#1 stuck\n"
            self.assertEqual(analyze(work, text)["status"], "INVALID", fault)

    def test_real_unclocked_and_hardware_are_distinct_valid_sources(self):
        for source in ("hardware", "unclocked"):
            result = analyze(*fixture(source))
            self.assertEqual(result["status"], "VALID", result)
            self.assertEqual(result["source"], source)
            self.assertEqual(result["metrics"]["cpu_ms_per_complete"], 2)

    def test_controls_invalidate_bad_evidence(self):
        cases = ("fake", "reset", "missing", "starved", "moved", "idle", "msc", "late", "threads",
                 "reasons", "stable", "area", "pipeline", "recovery", "latency", "modes", "zero",
                 "fatal", "timestamp", "duration", "empty", "query_only")
        for mutation in cases:
            with self.subTest(mutation=mutation):
                work, log = fixture()
                window = work["windows"][0]
                if mutation == "fake": log = log.replace("unclocked_bound=20", "unclocked_bound=10")
                elif mutation == "reset": log = log.replace("completions=20", "completions=0")
                elif mutation == "missing": log = ""
                elif mutation == "starved": window["starvation"] = 1
                elif mutation == "moved": window["after"] = {"x": 400}
                elif mutation == "idle": window["idle"] -= 1
                elif mutation == "msc": window["complete_ust_msc"][-1] = [0, 0]
                elif mutation == "late": window["late_slots"] = 1
                elif mutation == "threads": work["proc_after"]["session"]["threads"] = []
                elif mutation == "reasons": log = log.replace("damage_full_plan_count=20", "damage_full_plan_count=21")
                elif mutation == "stable": log = log.replace("damage_stable_geometry_full_count=20", "damage_stable_geometry_full_count=21")
                elif mutation == "area": log = log.replace("composition_repaint_pixels_count=2000", "composition_repaint_pixels_count=2001")
                elif mutation == "pipeline": log = log.replace("pipeline_creations_count=0", "pipeline_creations_count=1", 1)
                elif mutation == "recovery": log = log.replace("admission_errors=0", "admission_errors=1", 1)
                elif mutation == "latency": window["send_to_complete_usec"].pop()
                elif mutation == "modes": window["modes_copy_flip_skip_suboptimal"][0] = 40
                elif mutation == "zero": work["completed_at_measure_end"] = [0]
                elif mutation == "fatal": log += "sophia_runtime_fatal failure_code=example\n"
                elif mutation == "timestamp": log = log.replace("observed_monotonic_usec", "missing_time")
                elif mutation == "duration": work["measure_end_usec"] += 1_000_000
                elif mutation == "empty": work["windows"] = []
                elif mutation == "query_only": log = log.replace("native_event_waits=20", "no_field=20")
                self.assertEqual(analyze(work, log)["status"], "INVALID")

    def test_zero_resource_churn_and_reason_partition_are_counter_gates(self):
        work, log = fixture()
        result = analyze(work, log)
        self.assertEqual(sum(result["render"][k] for k in FULL_REASONS), 10)
        log = log.replace("pipeline_creations_count=0", "pipeline_creations_count=1")
        self.assertEqual(analyze(work, log)["status"], "VALID")  # Created before warmup.

    def test_regression_is_not_invalidity_or_a_pass(self):
        base = summarize([analyze(*fixture())]*3)
        self.assertEqual(compare(base, base)["status"], "PASS")
        for key, multiplier in (("cpu_ms_per_complete", 1.11), ("completions_per_second", .97),
                                ("latency_p95_usec", 1.11), ("guest_cpu_ms_per_complete", 1.11)):
            candidate = copy.deepcopy(base)
            candidate["metrics"][key]["median"] *= multiplier
            self.assertEqual(compare(base, candidate)["status"], "REGRESSION")

    def test_fewer_runs_or_changed_workload_cannot_qualify(self):
        base = summarize([analyze(*fixture())]*3)
        candidate = copy.deepcopy(base)
        candidate["config"]["rate_per_window"] = 1
        self.assertEqual(compare(base, candidate)["status"], "INVALID")
        self.assertEqual(compare(base, summarize([analyze(*fixture())]))["status"], "INVALID")
        mixed = [analyze(*fixture()), analyze(*fixture("hardware"))]
        self.assertEqual(summarize(mixed)["status"], "INVALID")

    def test_steal_and_work_leaving_session_invalidate_measurement(self):
        for stat in ("cpu 120 0 0 1960 0 0 0 20 0 0\ncpu0 120 0 0 1960 0 0 0 20\n",
                     "cpu 120 0 20 1960 0 0 0 0 0 0\ncpu0 120 0 20 1960 0 0 0 0\n"):
            work, log = fixture()
            work["proc_after"]["guest_stat"] = stat
            if "120 0 20" in stat:
                work["proc_after"]["guest_schedstat"] = work["proc_after"]["guest_schedstat"].replace(
                    "200000200", "400000200")
            result = analyze(work, log)
            self.assertEqual(result["status"], "INVALID")
            self.assertTrue(any(not c["pass"] and c["name"] in ("guest_steal", "guest_unaccounted")
                                for c in result["checks"]))

    def test_one_busy_cpu_cannot_hide_steal_in_idle_capacity(self):
        work, log = fixture()
        work["proc_before"]["guest_stat"] = "cpu 100 0 0 4000 0 0 0 0\n" + "".join(
            f"cpu{i} {100 if i == 0 else 0} 0 0 1000 0 0 0 0\n" for i in range(4))
        work["proc_after"]["guest_stat"] = "cpu 120 0 0 7940 0 0 0 40\n" + "".join(
            f"cpu{i} {120 if i == 0 else 0} 0 0 {1940 if i == 0 else 2000} 0 0 0 {40 if i == 0 else 0}\n"
            for i in range(4))
        for snapshot in ("proc_before", "proc_after"):
            work[snapshot]["guest_schedstat"] += "".join(
                f"cpu{i} 0 0 0 0 0 0 0 0 0\n" for i in range(1, 4))
        result = analyze(work, log)
        self.assertEqual(result["guest"]["steal_fraction"], .01)
        self.assertEqual(result["guest"]["max_cpu_steal_fraction"], .04)
        self.assertEqual([c["name"] for c in result["checks"] if not c["pass"]], ["guest_steal"])

    def test_unaccounted_growth_is_invalid_even_with_lower_session_cpu(self):
        base = summarize([analyze(*fixture())]*3)
        for share, expected in ((.05, "PASS"), (.0501, "INVALID")):
            candidate = copy.deepcopy(base)
            candidate["metrics"]["cpu_ms_per_complete"]["median"] *= .8
            candidate["metrics"]["guest_unaccounted_share"]["median"] = share
            self.assertEqual(compare(base, candidate)["status"], expected)

    def test_moving_cpu_elsewhere_does_not_claim_a_saving(self):
        base = summarize([analyze(*fixture())]*3)
        candidate = copy.deepcopy(base)
        candidate["metrics"]["cpu_ms_per_complete"] = {"min": 1, "max": 1, "median": 1}
        self.assertFalse(compare(base, candidate)["saving_observed"])
        candidate["metrics"]["guest_outside_target_share"]["median"] = .051
        self.assertEqual(compare(base, candidate)["status"], "INVALID")

    def test_guest_scheduler_data_and_task_lifetimes_are_required(self):
        for mutation in ("version", "reset", "empty", "exit"):
            work, log = fixture()
            if mutation == "version":
                work["proc_after"]["guest_schedstat"] = "version 18\n"
            elif mutation == "reset":
                work["proc_after"]["guest_schedstat"] = work["proc_after"]["guest_schedstat"].replace("200000200", "1")
            elif mutation == "empty":
                work["proc_before"]["guest_processes"] = None
            else:
                work["proc_after"]["guest_processes"].pop()
            self.assertEqual(analyze(work, log)["status"], "INVALID")

    def test_kernel_worker_renaming_is_not_a_new_lifetime(self):
        work, log = fixture()
        work["proc_before"]["guest_processes"][0]["name"] = "kworker/events"
        work["proc_after"]["guest_processes"][0]["name"] = "kworker/rcu"
        self.assertEqual(analyze(work, log)["status"], "VALID")

    def test_interrupts_in_runqueue_time_are_not_counted_twice(self):
        work, log = fixture()
        work["proc_after"]["guest_stat"] = (
            "cpu 120 0 0 1970 0 10 0 0\ncpu0 120 0 0 1970 0 10 0 0\n")
        work["proc_after"]["guest_schedstat"] = work["proc_after"]["guest_schedstat"].replace(
            "200000200", "300000200")
        result = analyze(work, log)
        self.assertEqual(result["status"], "VALID", result)
        self.assertAlmostEqual(result["guest"]["busy_seconds"], .3)
        self.assertAlmostEqual(result["guest"]["unaccounted_seconds"], 0)

    def test_changed_accounting_method_cannot_compare(self):
        base = summarize([analyze(*fixture())]*3)
        candidate = copy.deepcopy(base)
        candidate["accounting_method"] = "tick-sampling"
        self.assertEqual(compare(base, candidate)["status"], "INVALID")

    def test_memory_pressure_and_oversized_logs_cannot_qualify(self):
        for mutation in ("memory", "log"):
            work, log = fixture()
            if mutation == "memory":
                work["proc_after"]["guest_meminfo"] = "MemAvailable: 1 kB\n"
            else:
                work["proc_after"]["guest_log_bytes"] = 128 * 1024 * 1024 + 1
            self.assertEqual(analyze(work, log)["status"], "INVALID")

    def test_range_separation_includes_touching_as_overlap(self):
        base = summarize([analyze(*fixture())]*3)
        key = "cpu_ms_per_complete"
        base["metrics"][key] = {"min": 1, "median": 2, "max": 3}
        for lo, hi, expected in ((3.1, 4, "increase"), (0, .9, "decrease"),
                                 (3, 4, "overlap"), (0, 1, "overlap"), (1, 3, "overlap")):
            candidate = copy.deepcopy(base)
            candidate["metrics"][key] = {"min": lo, "median": (lo + hi) / 2, "max": hi}
            self.assertEqual(compare(base, candidate)["metrics"][key]["separated_ranges"], expected)

    def test_counter_coverage_at_probe_and_full_run_boundaries(self):
        for seconds, threshold in ((10, 4_000_000), (60, 48_000_000)):
            for span in (threshold - 1, threshold):
                work, log = fixture(mode="closed")
                work["config"]["seconds"] = seconds
                work["measure_end_usec"] = seconds * 1_000_000
                work["proc_after"]["monotonic_usec"] = seconds * 1_000_000
                idle = 1000 + seconds * 100 - 20
                work["proc_after"]["guest_stat"] = (
                    f"cpu 120 0 0 {idle} 0 0 0 0\ncpu0 120 0 0 {idle} 0 0 0 0\n")
                log = log.replace("observed_monotonic_usec=6000000",
                                  f"observed_monotonic_usec={1_000_000 + span}")
                result = analyze(work, log)
                self.assertEqual(result["status"], "VALID" if span == threshold else "INVALID", result)
                if span < threshold:
                    self.assertEqual([c["name"] for c in result["checks"] if not c["pass"]], ["counter_span"])

    def test_owner_is_reported_not_assumed_to_be_process_leader(self):
        work, log = fixture()
        for snapshot in ("proc_before", "proc_after"):
            work[snapshot]["session"]["threads"][0]["tid"] = 88
        self.assertEqual(analyze(work, log)["status"], "INVALID")
        self.assertEqual(analyze(work, log.replace("owner_tid=42", "owner_tid=88"))["status"], "VALID")

    def test_malformed_input_always_gets_invalid_result(self):
        for work in (None, [], {"status": "complete", "config": []}):
            self.assertEqual(analyze(work, "")["status"], "INVALID")


if __name__ == "__main__":
    unittest.main()

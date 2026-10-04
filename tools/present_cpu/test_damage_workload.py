import copy
import os
from pathlib import Path
import subprocess
import unittest

from analyze import analyze, compare, summarize
from test_analyze import fixture


def head_fixture():
    work, log = fixture()
    work["schema"] = 2
    work["config"].update(size="head", damage="patch", pattern="fixed_patch_v1")
    window = work["windows"][0]
    rect = dict(x=16, y=16, width=992, height=736)
    window["initial"].update(rect)
    window["initial"].update(head=dict(x=0, y=0, width=1024, height=768),
                             patch=dict(x=40, y=40, width=120, height=120))
    window["initial"]["output_layout"] = [window["initial"]["head"].copy()]
    window.update(before=rect.copy(), after=rect.copy(), source_pixel_checks_before_grace=9, unchanged_presents=0)
    return work, log


class DamageWorkload(unittest.TestCase):
    def test_geometry_and_pixel_checks_cannot_be_omitted(self):
        work, log = head_fixture()
        self.assertEqual(analyze(work, log)["status"], "VALID")
        mutations = [
            lambda w: w["windows"][0].update(source_pixel_checks_before_grace=0),
            lambda w: w["windows"][0]["initial"]["head"].update(width=1023),
            lambda w: w["windows"][0]["initial"]["patch"].update(width=121),
            lambda w: w["config"].pop("damage"),
            lambda w: w["config"].update(pattern="unknown"),
            lambda w: w["windows"][0].update(unchanged_presents=1),
            lambda w: w["windows"][0]["initial"].update(output_layout=[]),
        ]
        for mutation in mutations:
            changed = copy.deepcopy(work)
            mutation(changed)
            self.assertEqual(analyze(changed, log)["status"], "INVALID")

    def test_distinct_damage_streams_and_old_pixels_cannot_claim_a_speedup(self):
        work, log = head_fixture()
        report = analyze(work, log)
        reference = summarize([report] * 3)
        for field, value in (("damage", "full"), ("size", "small"),
                             ("pattern", "alternating_background_v1")):
            changed = copy.deepcopy(report)
            changed["config"][field] = value
            self.assertEqual(summarize([report, changed])["status"], "INVALID")
            self.assertEqual(compare(reference, summarize([changed] * 3))["status"], "INVALID")
        historical = analyze(*fixture())
        old = summarize([historical] * 3)
        self.assertEqual(old["config"]["pattern"], "alternating_background_v1")
        self.assertEqual(compare(old, reference)["status"], "INVALID")
        changed = copy.deepcopy(report)
        changed["output_layout"].append(dict(x=1024, y=0, width=1024, height=768))
        self.assertEqual(summarize([report, changed])["status"], "INVALID")
        self.assertEqual(compare(reference, summarize([changed] * 3))["status"], "INVALID")

    def test_guest_parameters_reject_overlap_and_unknown_modes_before_boot(self):
        root = Path(__file__).resolve().parents[2]
        prefix = (root / "tools/qemu_session_harness.sh").read_text().split('case "$SCENARIO" in')[0]
        script = prefix + '\nprintf "%s\\n" "$cpu_cmdline"\n'
        env = {k: v for k, v in os.environ.items() if not k.startswith("SOPHIA_")}
        env.update(SOPHIA_QEMU_SCENARIO="cpu", SOPHIA_QEMU_CPU_CLIENTS="1", SOPHIA_QEMU_CPU_SIZE="head")
        for damage in ("absent", "full", "patch"):
            run = subprocess.run(["bash", "-c", script], env={**env, "SOPHIA_QEMU_CPU_DAMAGE": damage},
                                 capture_output=True, text=True, check=True)
            self.assertIn("sophia.cpu_size=head", run.stdout)
            self.assertIn("sophia.cpu_clients=1", run.stdout)
            self.assertIn("sophia.cpu_damage=" + damage, run.stdout)
        for key, value in (("CLIENTS", "2"), ("SIZE", "bad"), ("DAMAGE", "bad")):
            run = subprocess.run(["bash", "-c", script], env={**env, "SOPHIA_QEMU_CPU_" + key: value},
                                 capture_output=True, text=True)
            self.assertNotEqual(run.returncode, 0)
            self.assertIn("invalid CPU size/damage", run.stderr)

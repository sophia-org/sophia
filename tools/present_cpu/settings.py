"""Explicit opt-in mode comparisons; all other settings must stay identical."""
import re

CHOICES = {"present_evidence": ("full", "aggregate"), "renderer_worker": ("private", "shared")}


def validate_settings(settings):
    if settings.keys() != CHOICES.keys() or any(settings[k] not in v for k, v in CHOICES.items()):
        raise ValueError("missing or unknown experiment settings")


def comparison_settings(before, after, varied=None):
    validate_settings(before)
    validate_settings(after)
    changed = {key for key in CHOICES if before[key] != after[key]}
    if changed != ({varied} if varied else set()):
        raise ValueError("comparison must change exactly the declared setting")


def verify_settings(text, settings):
    validate_settings(settings)
    modes = re.findall(r"^sophia_present_evidence schema=1 mode=(\w+)$", text, re.M)
    if modes != [settings["present_evidence"]]:
        raise ValueError("Session did not confirm the requested evidence mode")
    records = re.findall(r"^sophia_live_native_resources .*status=complete .*", text, re.M)
    if len(records) != 1:
        raise ValueError("missing renderer resource completion")
    fields = dict(word.split("=", 1) for word in records[0].split()[1:] if "=" in word)
    expected = 1 if settings["renderer_worker"] == "shared" else 2
    if int(fields["renderer_workers"]) != expected:
        raise ValueError("renderer count does not match one-card/two-head experiment")
    for key in ("worker_failures", "worker_hard_stalls", "worker_result_misroutes",
                "worker_release_enqueue_failures", "frame_slots_leased"):
        if int(fields[key]) != 0:
            raise ValueError(f"renderer completion failed: {key}")
    return fields

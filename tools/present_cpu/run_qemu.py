#!/usr/bin/env python3
"""Run autonomous headless CPU trials; preserve failures and compare stored baselines.

The fixed-rate arm defaults to five Presents/s/window, below the established
software guest capacity. The closed arm measures saturation. No host display,
input, DRM nodes or session sockets are opened. KVM is required for comparability.
"""
import argparse
import json
import os
import platform
import signal
from pathlib import Path
import subprocess
import tempfile
import time

from analyze import ACCOUNTING_METHOD, analyze, compare, summarize
from build_overlay import digest
from build_record import source_identity
from log_transport import unpack

ROOT = Path(__file__).resolve().parents[2]


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def host():
    def read(path):
        try:
            return path.read_text().strip()
        except OSError as error:
            return f"unreadable:{error.errno}"
    cpu = Path("/proc/cpuinfo").read_text().splitlines()
    return {"loadavg": Path("/proc/loadavg").read_text().strip(),
            "kernel": platform.release(), "machine": platform.machine(),
            "cpu_identity": sorted({line.strip() for line in cpu if
                                     line.startswith(("model name", "microcode", "vendor_id", "cpu family", "stepping"))}),
            "kvm_parameters": {str(p): read(p) for p in Path("/sys/module/kvm/parameters").glob("*")},
            "transparent_hugepages": read(Path("/sys/kernel/mm/transparent_hugepage/enabled")),
            "meminfo": Path("/proc/meminfo").read_text(),
            "governors": {str(p): p.read_text().strip() for p in
                          Path("/sys/devices/system/cpu/cpufreq").glob("policy*/scaling_governor")},
            "processes": subprocess.check_output(["ps", "-eo", "pid,comm,pcpu,pmem"], text=True)}


def trial(args, payload, name, mode, seconds):
    directory = args.out / name
    directory.mkdir()
    write(directory / "host-before.json", host())
    env = {k: v for k, v in os.environ.items() if not k.startswith("SOPHIA_") and k not in
           ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY", "DBUS_SESSION_BUS_ADDRESS")}
    env.update(SOPHIA_QEMU_SCENARIO="cpu", SOPHIA_QEMU_OUT_DIR=str(directory.resolve()),
               SOPHIA_QEMU_EVIDENCE=str((directory / "serial.log").resolve()),
               SOPHIA_QEMU_KERNEL=str(args.kernel.resolve()),
               SOPHIA_QEMU_INITRAMFS=str(args.initramfs.resolve()), SOPHIA_QEMU_CPUS=str(args.cpus),
               SOPHIA_QEMU_MEMORY_MIB="2048", SOPHIA_QEMU_SINGLE_CARD="1",
               SOPHIA_QEMU_CPU_MODE=mode, SOPHIA_QEMU_CPU_SECONDS=str(seconds),
               SOPHIA_QEMU_CPU_GRACE="10", SOPHIA_QEMU_CPU_RATE=str(args.rate),
               SOPHIA_QEMU_CPU_CLIENTS=str(args.clients), SOPHIA_QEMU_CPU_SIZE=args.size,
               SOPHIA_QEMU_CPU_DAMAGE=args.damage,
               SOPHIA_QEMU_CPU_TARGET=args.target, SOPHIA_QEMU_GPU_MODE="software",
               SOPHIA_QEMU_ACCEL="kvm")
    for key, path in (("kernel", args.kernel), ("initramfs", args.initramfs)):
        if digest(path) != payload["files"][key]["sha256"]:
            raise ValueError(f"{key} changed during campaign")
    start = time.monotonic()
    command = [str(ROOT / "tools/qemu_session_harness.sh")]
    # AF_UNIX paths are limited to 108 bytes; evidence paths may be much longer.
    with tempfile.TemporaryDirectory(prefix="sophia-cpu-") as sockets, (directory / "launcher.log").open("w") as log:
        env.update(SOPHIA_QEMU_VNC_SOCKET=str(Path(sockets) / "display"),
                   SOPHIA_QEMU_QMP_SOCKET=str(Path(sockets) / "qmp"))
        # This process group contains only this invocation's harness, guest and logger.
        with subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT,
                              start_new_session=True) as child:
            try:
                code = child.wait(timeout=seconds + 150)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
                code = 124
    serial = directory / "serial.log"
    text = serial.read_text(errors="replace") if serial.exists() else ""
    export = directory / "cpu-export.log"
    if export.exists():
        text += "\n" + export.read_text(errors="replace")
    records = [line.removeprefix("sophia_present_cpu_result ") for line in text.splitlines()
               if line.startswith("sophia_present_cpu_result ")]
    if len(records) == 1:
        # Keep raw workload evidence even when subsequent log export failed.
        (directory / "workload-record.json").write_text(records[0] + "\n")
    if code or len(records) != 1:
        report = {"status": "INVALID", "benefit": "unmeasured", "reason": "guest/workload failure",
                  "exit": code, "workload_records": len(records)}
    else:
        try:
            text, session_log, log_record = unpack(text)
            (directory / "session.log").write_bytes(session_log)
            work = json.loads(records[0])
            write(directory / "workload.json", work)
            report = analyze(work, text)
            report["session_log"] = log_record
        except (ValueError, KeyError, TypeError) as error:
            report = {"status": "INVALID", "error": str(error)}
    report.update(wall_seconds=time.monotonic() - start, command=command, exit=code)
    write(directory / "analysis.json", report)
    write(directory / "host-after.json", host())
    print(name, report["status"], flush=True)
    return report


def run(args):
    args.out.mkdir(parents=True, exist_ok=False)
    payload = json.loads(Path(str(args.initramfs) + ".json").read_text())
    before = host()
    identity = {"payload": payload, "qemu": subprocess.check_output(["qemu-system-x86_64", "--version"], text=True),
                "runner_source": source_identity(),
                "vcpus": args.cpus, "memory_mib": 2048, "accelerator": "kvm",
                "gpu": "software virtio, one card/two outputs", "quiet_window": args.quiet_note,
                "session_mode": "normal", "export_transport": "virtio-serial-file",
                "host_before": before}
    write(args.out / "IDENTITY.json", identity)
    # Compatibility excludes only the revision under test and its image digest.
    compatibility = {k: identity[k] for k in ("qemu", "vcpus", "memory_mib", "accelerator", "gpu",
                                             "session_mode", "export_transport")}
    compatibility["accounting_method"] = ACCOUNTING_METHOD
    compatibility.update({k: payload["files"][k]["sha256"] for k in
                          ("base", "kernel", "client", "init", "export", "cat", "gzip", "base64",
                           "sha256sum", "wc", "du")})
    compatibility.update({k: before[k] for k in
                          ("governors", "kernel", "machine", "cpu_identity", "kvm_parameters", "transparent_hugepages")})
    baseline = json.loads(args.baseline.read_text()) if args.baseline else None
    if baseline and baseline.get("compatibility") != compatibility:
        raise ValueError("baseline environment, kernel, client or guest recipe differs")
    reports = {mode: [] for mode in args.modes}
    summary = {"schema": 1, "status": "RUNNING", "compatibility": compatibility,
               "identity": identity, "modes": {}, "scope": "Guest relative signal; no live or W1 acceptance"}

    def save(status):
        summary["status"] = status
        summary["host_after"] = host()
        write(args.out / "SUMMARY.json", summary)

    for mode in args.modes:
        probe = trial(args, payload, "probe-" + mode, mode, 10)
        summary.setdefault("probes", {})[mode] = probe
        if probe["status"] != "VALID":
            save("INVALID")
            return 2
    if args.probe_only:
        save("PROBE_PASS")
        return 0
    for number in range(3):
        for mode in args.modes if number % 2 == 0 else reversed(args.modes):
            report = trial(args, payload, f"{number+1}-{mode}", mode, 60)
            reports[mode].append(report)
            summary["modes"][mode] = summarize(reports[mode])
            if report["status"] != "VALID":
                save("INVALID")
                return 2
            save("RUNNING")
    status = "BASELINE_RECORDED"
    if baseline:
        summary["binary_comparison"] = {
            "baseline": baseline["identity"]["payload"]["files"]["sophia"],
            "candidate": payload["files"]["sophia"],
            "same_binary": baseline["identity"]["payload"]["files"]["sophia"]["sha256"] ==
                payload["files"]["sophia"]["sha256"],
        }
        summary["comparison"] = {mode: compare(baseline["modes"][mode], summary["modes"][mode]) for mode in args.modes}
        status = "PASS" if all(v["status"] == "PASS" for v in summary["comparison"].values()) else "REVIEW_REQUIRED"
    save(status)
    return 0 if status != "REVIEW_REQUIRED" else 3


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for key in ("kernel", "initramfs", "out"):
        parser.add_argument("--" + key, type=Path, required=True)
    parser.add_argument("--quiet-note", required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--cpus", type=int, choices=range(1, 17), default=4)
    parser.add_argument("--rate", type=int, choices=range(1, 241), default=5)
    parser.add_argument("--target", choices=("zero", "next"), default="next")
    parser.add_argument("--clients", type=int, choices=(1, 2), default=2)
    parser.add_argument("--size", choices=("small", "head"), default="small")
    parser.add_argument("--damage", choices=("absent", "full", "patch"), default="absent")
    parser.add_argument("--modes", choices=("open", "closed"), nargs="+", default=["open", "closed"])
    parser.add_argument("--probe-only", action="store_true")
    args = parser.parse_args()
    if args.size == "head" and args.clients != 1:
        parser.error("--size=head requires --clients=1")
    raise SystemExit(run(args))

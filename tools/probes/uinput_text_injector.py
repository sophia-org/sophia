#!/usr/bin/env python3
"""Create a bounded virtual keyboard or mouse and inject input through Linux uinput."""

from __future__ import annotations

import argparse
import fcntl
import os
from pathlib import Path
import signal
import struct
import sys
import time


EV_SYN = 0
EV_KEY = 1
SYN_REPORT = 0
BUS_USB = 0x03
DEFAULT_TEXT = "sophia\n"
KEY_LEFTSHIFT = 42
KEY_LEFTMETA = 125
KEY_LEFTCTRL = 29
KEY_LEFTALT = 56
KEY_BACKSPACE = 14
EV_REL = 2
REL_X = 0
REL_Y = 1
BTN_LEFT = 0x110
MAX_SHAKE_HZ = 2000
MAX_SHAKE_SECONDS = 600.0
MAX_SHAKE_AMPLITUDE = 64

KEY_CODES = {
    "a": 30,
    "b": 48,
    "c": 46,
    "d": 32,
    "e": 18,
    "f": 33,
    "g": 34,
    "h": 35,
    "i": 23,
    "j": 36,
    "k": 37,
    "l": 38,
    "m": 50,
    "n": 49,
    "o": 24,
    "p": 25,
    "q": 16,
    "r": 19,
    "s": 31,
    "t": 20,
    "u": 22,
    "v": 47,
    "w": 17,
    "x": 45,
    "y": 21,
    "z": 44,
    "\n": 28,
}

IOC_NRBITS = 8
IOC_TYPEBITS = 8
IOC_SIZEBITS = 14
IOC_NRSHIFT = 0
IOC_TYPESHIFT = IOC_NRSHIFT + IOC_NRBITS
IOC_SIZESHIFT = IOC_TYPESHIFT + IOC_TYPEBITS
IOC_DIRSHIFT = IOC_SIZESHIFT + IOC_SIZEBITS
IOC_NONE = 0
IOC_WRITE = 1
UINPUT_TYPE = ord("U")


def ioctl_code(direction: int, number: int, size: int = 0) -> int:
    return (
        (direction << IOC_DIRSHIFT)
        | (UINPUT_TYPE << IOC_TYPESHIFT)
        | (number << IOC_NRSHIFT)
        | (size << IOC_SIZESHIFT)
    )


UI_DEV_CREATE = ioctl_code(IOC_NONE, 1)
UI_DEV_DESTROY = ioctl_code(IOC_NONE, 2)
UI_DEV_SETUP = ioctl_code(IOC_WRITE, 3, 92)
UI_SET_EVBIT = ioctl_code(IOC_WRITE, 100, 4)
UI_SET_KEYBIT = ioctl_code(IOC_WRITE, 101, 4)
UI_SET_RELBIT = ioctl_code(IOC_WRITE, 102, 4)
INPUT_EVENT = struct.Struct("llHHi")
UINPUT_SETUP = struct.Struct("HHHH80sI")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Create a virtual keyboard or mouse, publish its /dev/input/event "
            "path, then inject bounded input after a trigger file appears."
        )
    )
    parser.add_argument("--text")
    parser.add_argument("--chord", choices=("logout", "recovery"))
    parser.add_argument("--ready-file", type=Path)
    parser.add_argument("--trigger-file", type=Path)
    parser.add_argument("--result-file", type=Path)
    parser.add_argument("--followup-chord", choices=("logout", "recovery"))
    parser.add_argument("--followup-trigger-file", type=Path)
    parser.add_argument("--followup-result-file", type=Path)
    parser.add_argument("--shake-hz", type=int)
    parser.add_argument("--shake-seconds", type=float)
    parser.add_argument("--shake-amplitude", type=int, default=8)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    parser.add_argument("--key-interval-ms", type=float, default=2.0)
    parser.add_argument("--self-test", action="store_true")
    return parser.parse_args()


def validate_text(text: str) -> list[int]:
    unsupported = sorted(set(text).difference(KEY_CODES))
    if unsupported:
        rendered = ", ".join(repr(character) for character in unsupported)
        raise ValueError(f"unsupported characters: {rendered}")
    if not text:
        raise ValueError("text must not be empty")
    return [KEY_CODES[character] for character in text]


def input_sequence(
    text: str | None, chord: str | None
) -> tuple[str, str, list[tuple[int, int]]]:
    if text is not None and chord is not None:
        raise ValueError("--text and --chord are mutually exclusive")
    if chord == "logout":
        keys = [KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_CODES["q"]]
        sequence = [(keycode, 1) for keycode in keys]
        sequence.extend((keycode, 0) for keycode in reversed(keys))
        return "chord", "chord=logout keys=3", sequence
    if chord == "recovery":
        keys = [KEY_LEFTCTRL, KEY_LEFTALT, KEY_BACKSPACE]
        sequence = [(keycode, 1) for keycode in keys]
        sequence.extend((keycode, 0) for keycode in reversed(keys))
        return "chord", "chord=recovery keys=3", sequence

    resolved_text = DEFAULT_TEXT if text is None else text
    keycodes = validate_text(resolved_text)
    sequence = []
    for keycode in keycodes:
        sequence.extend(((keycode, 1), (keycode, 0)))
    return "text", f"characters={len(resolved_text)}", sequence


def shake_plan(args: argparse.Namespace) -> tuple[int, float, int] | None:
    """A pointer shake: alternating horizontal motion at a fixed report rate.

    One relative motion per report is what a hand produces on a 1 kHz mouse,
    and it is the shape the compositor's routing and cursor paths are paced
    by. The device is a mouse rather than a keyboard, so it is exclusive with
    every key sequence.
    """
    if args.shake_hz is None and args.shake_seconds is None:
        return None
    if args.shake_hz is None or args.shake_seconds is None:
        raise ValueError("--shake-hz and --shake-seconds are required together")
    if args.text is not None or args.chord is not None or args.followup_chord is not None:
        raise ValueError("a shake is exclusive with --text, --chord and --followup-chord")
    if not 1 <= args.shake_hz <= MAX_SHAKE_HZ:
        raise ValueError(f"--shake-hz must be between 1 and {MAX_SHAKE_HZ}")
    if not 0 < args.shake_seconds <= MAX_SHAKE_SECONDS:
        raise ValueError(
            f"--shake-seconds must be positive and at most {MAX_SHAKE_SECONDS:g}"
        )
    if not 1 <= args.shake_amplitude <= MAX_SHAKE_AMPLITUDE:
        raise ValueError(f"--shake-amplitude must be between 1 and {MAX_SHAKE_AMPLITUDE}")
    return args.shake_hz, args.shake_seconds, args.shake_amplitude


def publish(path: Path | None, value: str) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    with temporary.open("x", encoding="utf-8") as output:
        output.write(value)
        output.write("\n")
    os.chmod(temporary, 0o600)
    os.replace(temporary, path)


def find_event_device(name: str, deadline: float) -> Path:
    sysfs = Path("/sys/devices/virtual/input")
    while time.monotonic() < deadline:
        for input_path in sysfs.glob("input*"):
            try:
                if (input_path / "name").read_text(encoding="utf-8").strip() != name:
                    continue
            except (FileNotFoundError, PermissionError):
                continue
            event_paths = sorted(input_path.glob("event*"))
            if event_paths:
                return Path("/dev/input") / event_paths[0].name
        time.sleep(0.01)
    raise TimeoutError("uinput event node did not appear before the deadline")


def wait_for_trigger(path: Path, deadline: float, stopped: list[bool]) -> None:
    while not path.exists():
        if stopped[0]:
            raise InterruptedError("stopped before injection")
        if time.monotonic() >= deadline:
            raise TimeoutError("injection trigger did not appear before the deadline")
        time.sleep(0.005)


def emit(device: int, event_type: int, code: int, value: int) -> None:
    os.write(device, INPUT_EVENT.pack(0, 0, event_type, code, value))


def inject(device: int, sequence: list[tuple[int, int]], interval: float) -> None:
    for keycode, value in sequence:
        emit(device, EV_KEY, keycode, value)
        emit(device, EV_SYN, SYN_REPORT, 0)
        if interval:
            time.sleep(interval)


def inject_shake(
    device: int, rate_hz: int, seconds: float, amplitude: int, stopped: list[bool]
) -> tuple[int, float]:
    """Emit alternating REL_X reports on an absolute schedule.

    Returns the reports sent and the rate actually achieved, so a run that
    could not hold its rate says so instead of being read as one that did.
    """
    period_ns = 1_000_000_000 // rate_hz
    planned = int(rate_hz * seconds)
    started = time.monotonic_ns()
    sent = 0
    for index in range(planned):
        if stopped[0]:
            break
        remaining = started + index * period_ns - time.monotonic_ns()
        if remaining > 0:
            time.sleep(remaining / 1e9)
        emit(device, EV_REL, REL_X, amplitude if index % 2 == 0 else -amplitude)
        emit(device, EV_SYN, SYN_REPORT, 0)
        sent += 1
    elapsed_ns = time.monotonic_ns() - started
    achieved_hz = sent * 1e9 / elapsed_ns if elapsed_ns > 0 else 0.0
    return sent, achieved_hz


def self_test(mode: str, description: str, events: int) -> int:
    if INPUT_EVENT.size != 24 or UINPUT_SETUP.size != 92:
        raise RuntimeError(
            f"unexpected Linux ABI sizes: input_event={INPUT_EVENT.size} "
            f"uinput_setup={UINPUT_SETUP.size}"
        )
    print(
        "sophia_uinput schema=1 status=self_test_passed "
        f"mode={mode} {description} events={events}"
    )
    return 0


def validate_followup(args: argparse.Namespace) -> None:
    followup_paths = (args.followup_trigger_file, args.followup_result_file)
    if args.followup_chord is None:
        if any(path is not None for path in followup_paths):
            raise ValueError("follow-up paths require --followup-chord")
        return
    if args.text is not None or args.chord is None:
        raise ValueError("--followup-chord requires an initial --chord")
    if args.followup_trigger_file is None and not args.self_test:
        raise ValueError("--followup-chord requires --followup-trigger-file")


def main() -> int:
    args = parse_args()
    if args.timeout_seconds <= 0:
        raise ValueError("--timeout-seconds must be positive")
    if args.key_interval_ms < 0 or args.key_interval_ms > 1000:
        raise ValueError("--key-interval-ms must be between 0 and 1000")
    validate_followup(args)
    shake = shake_plan(args)
    if shake is not None:
        rate_hz, seconds, amplitude = shake
        mode = "shake"
        description = f"rate_hz={rate_hz} seconds={seconds:g} amplitude={amplitude}"
        sequence: list[tuple[int, int]] = []
        events = int(rate_hz * seconds) * 2
    else:
        mode, description, sequence = input_sequence(args.text, args.chord)
        events = len(sequence) * 2
    followup = None
    if args.followup_chord is not None:
        followup = input_sequence(None, args.followup_chord)
    if args.self_test:
        self_test(mode, description, events)
        if followup is not None:
            followup_mode, followup_description, followup_sequence = followup
            self_test(followup_mode, followup_description, len(followup_sequence) * 2)
        return 0
    if args.ready_file is None or args.trigger_file is None:
        raise ValueError("--ready-file and --trigger-file are required")

    stopped = [False]

    def stop(_signum: int, _frame: object) -> None:
        stopped[0] = True

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    kind = "Mouse" if shake is not None else "Keyboard"
    name = f"Sophia Virtual {kind} {os.getpid()}"
    deadline = time.monotonic() + args.timeout_seconds
    device = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
    created = False
    try:
        fcntl.ioctl(device, UI_SET_EVBIT, EV_SYN)
        fcntl.ioctl(device, UI_SET_EVBIT, EV_KEY)
        if shake is not None:
            # udev tags a pointer only when a mouse button sits beside the
            # relative axes; without BTN_LEFT libinput never sees the device.
            fcntl.ioctl(device, UI_SET_EVBIT, EV_REL)
            fcntl.ioctl(device, UI_SET_RELBIT, REL_X)
            fcntl.ioctl(device, UI_SET_RELBIT, REL_Y)
            fcntl.ioctl(device, UI_SET_KEYBIT, BTN_LEFT)
        # One device retains input ownership across ordered proof phases.
        keycodes = {keycode for keycode, _value in sequence}
        if followup is not None:
            keycodes.update(keycode for keycode, _value in followup[2])
        for keycode in sorted(keycodes):
            fcntl.ioctl(device, UI_SET_KEYBIT, keycode)
        encoded_name = name.encode("utf-8")
        setup = UINPUT_SETUP.pack(
            BUS_USB,
            0x534F,
            0x4D53 if shake is not None else 0x5048,
            1,
            encoded_name.ljust(80, b"\0"),
            0,
        )
        fcntl.ioctl(device, UI_DEV_SETUP, setup)
        fcntl.ioctl(device, UI_DEV_CREATE)
        created = True
        event_path = find_event_device(name, deadline)
        publish(args.ready_file, str(event_path))
        print(
            "sophia_uinput schema=1 status=ready "
            f"device={event_path} mode={mode} {description}",
            flush=True,
        )
        wait_for_trigger(args.trigger_file, deadline, stopped)
        injected_at_usec = time.monotonic_ns() // 1_000
        if shake is not None:
            rate_hz, seconds, amplitude = shake
            publish(args.result_file, str(injected_at_usec))
            reports, achieved_hz = inject_shake(
                device, rate_hz, seconds, amplitude, stopped
            )
            print(
                "sophia_uinput schema=1 status=injected "
                f"mode={mode} {description} events={reports * 2} "
                f"achieved_hz={achieved_hz:.1f}",
                flush=True,
            )
        else:
            inject(device, sequence, args.key_interval_ms / 1000.0)
            publish(args.result_file, str(injected_at_usec))
            print(
                "sophia_uinput schema=1 status=injected "
                f"mode={mode} {description} events={len(sequence) * 2}",
                flush=True,
            )
        if followup is not None:
            followup_mode, followup_description, followup_sequence = followup
            assert args.followup_trigger_file is not None
            wait_for_trigger(args.followup_trigger_file, deadline, stopped)
            followup_at_usec = time.monotonic_ns() // 1_000
            inject(device, followup_sequence, args.key_interval_ms / 1000.0)
            publish(args.followup_result_file, str(followup_at_usec))
            print(
                "sophia_uinput schema=1 status=injected phase=followup "
                f"mode={followup_mode} {followup_description} "
                f"events={len(followup_sequence) * 2}",
                flush=True,
            )
        while not stopped[0]:
            time.sleep(0.05)
    finally:
        if created:
            try:
                fcntl.ioctl(device, UI_DEV_DESTROY)
            except OSError:
                pass
        os.close(device)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, TimeoutError, ValueError) as error:
        print(f"sophia_uinput schema=1 status=failed reason={error}", file=sys.stderr)
        raise SystemExit(1) from error

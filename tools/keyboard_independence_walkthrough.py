#!/usr/bin/env python3
"""Walk an operator through the attended keyboard-independence gate.

Checks what the gate would refuse on, lets the operator name the two
keyboards, says what each phase asks for, runs the gate, and then reads the
result back. `--check` does everything but run the gate, from anywhere.
"""
import argparse
import glob
import grp
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
RUNNER = ROOT / 'tools/run_keyboard_independence_gate_tty4.sh'
VERIFIER = ROOT / 'tools/verify_keyboard_independence_physical.sh'
ARCHIVE_VERIFIER = ROOT / 'tools/verify_keyboard_independence_physical_archive.sh'
EVIDENCE = Path('/tmp/sophia-keyboard-independence-physical')
PROOF_TEXT = 'twokeyboards'


def say(text=''):
    print(text, flush=True)


def run(command, **kwargs):
    return subprocess.run(command, capture_output=True, text=True, **kwargs)


def greetd_vt():
    try:
        text = Path('/etc/greetd/config.toml').read_text()
    except OSError:
        return None
    terminal = False
    for line in text.splitlines():
        if re.match(r'^\[[^\]]+\]\s*$', line):
            terminal = line.strip() == '[terminal]'
        elif terminal and re.match(r'^\s*vt\s*=', line):
            return line.split('=', 1)[1].strip()
    return None


def preconditions(require_console):
    """Every reason the gate would refuse, as (ok, line) pairs."""
    checks = []
    status = run(['git', '-C', str(ROOT), 'status', '--short']).stdout.strip()
    checks.append((not status, 'repository tree is clean' if not status
                   else 'repository tree is dirty; commit or stash first:\n' + status))
    head = run(['git', '-C', str(ROOT), 'rev-parse', 'HEAD']).stdout.strip()
    signed = run(['git', '-C', str(ROOT), 'verify-commit', head]).returncode == 0
    checks.append((signed, f'HEAD {head[:12]} carries a valid signature' if signed
                   else f'HEAD {head[:12]} is not signed; the gate binds a signed commit'))
    keyd = run(['pgrep', '-x', 'keyd']).returncode == 0
    checks.append((not keyd, 'keyd is not running' if not keyd
                   else 'keyd is running; stop it (sudo sv down keyd): it merges every keyboard into one'))
    kitty = shutil.which('kitty')
    checks.append((bool(kitty), f'kitty at {kitty}' if kitty else 'kitty is not installed'))
    groups = {grp.getgrgid(g).gr_name for g in os.getgroups()}
    checks.append(('input' in groups, 'this user is in the input group' if 'input' in groups
                   else 'this user is not in the input group; the guard cannot open the seat'))
    runtime = os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}'
    runtime_ok = Path(runtime).is_dir() and Path(runtime).stat().st_uid == os.getuid()
    checks.append((runtime_ok, f'runtime directory {runtime}' if runtime_ok
                   else f'runtime directory missing or not yours: {runtime}'))
    if require_console:
        try:
            console = os.ttyname(0)
        except OSError:
            console = ''
        match = re.match(r'^/dev/tty(\d+)$', console)
        manager = greetd_vt()
        if not match:
            checks.append((False, f'not on a text console ({console or "no tty"}); switch with Ctrl+Alt+F3 and log in there'))
        elif manager and match.group(1) == manager:
            checks.append((False, f'{console} is the display manager\'s console; use another'))
        else:
            checks.append((True, f'on text console {console}'))
    return checks


def keyboards():
    rows = []
    for link in sorted(glob.glob('/dev/input/by-id/*-event-kbd')):
        node = os.path.realpath(link)
        readable = os.access(node, os.R_OK | os.W_OK)
        name = Path(link).name[len('usb-'):-len('-event-kbd')] if Path(link).name.startswith('usb-') else Path(link).name
        note = ''
        if '-if0' in Path(link).name:
            note = 'extra interface of the same device; pick the main one'
        elif not readable:
            note = 'not readable and writable by you'
        rows.append((link, node, name, note))
    return rows


def choose(rows, prompt, exclude=None):
    while True:
        answer = input(f'{prompt} [1-{len(rows)}]: ').strip()
        if not answer.isdigit() or not 1 <= int(answer) <= len(rows):
            say('  give the number from the list')
            continue
        row = rows[int(answer) - 1]
        if exclude and os.path.realpath(row[0]) == os.path.realpath(exclude[0]):
            say('  that is the same device as keyboard A; pick the other keyboard')
            continue
        if not os.access(row[1], os.R_OK | os.W_OK):
            say('  that node is not readable and writable by you; pick another or fix its permissions')
            continue
        return row


def explain(a, b):
    say()
    say('What the gate will ask of you, in order. Press ONLY the keys named.')
    say(f'  Keyboard A (you will unplug it):  {a[2]}')
    say(f'  Keyboard B (stays, types the end): {b[2]}')
    say()
    say('Phase 1, guard on the whole seat (on this console):')
    say('  1. Hold Ctrl+Alt on A and press Backspace on B; release; wait 3 s. Nothing must happen.')
    say('  2. Press and release Ctrl+Alt+Backspace on A alone. The guard arms.')
    say('  3. Press Ctrl+Alt+Backspace on A once more. The guard triggers and exits.')
    say('Phase 2, guard pinned to A only (on this console):')
    say('  1. Press and release Ctrl+Alt+Backspace on B; wait 3 s. Nothing must happen.')
    say('  2 and 3. The same two chords on A: arms, then triggers.')
    say('Phase 3, the session takes the display and shows a guide in Kitty:')
    say('  1. Tap Left Shift on B.')
    say('  2. Press and HOLD Left Shift on A, and keep holding.')
    say('  3. Still holding A: tap Left Shift on B once; wait 3 s.')
    say('  4. Still holding A: UNPLUG keyboard A. The screen advances when Sophia sees it leave.')
    say('  5. Plug A back in; the screen advances when Sophia announces it under a new identity.')
    say('  6. Tap Left Shift on A.')
    say(f'  7. On B, type {PROOF_TEXT} and press Enter. The session ends at once.')
    say()
    say('Before the display changes hands the runner builds the release binary (minutes),')
    say('and the launcher asks for your sudo password to stop and later restore greetd.')
    say('When the run ends the greeter comes back on its own console; switch back to')
    say('this one (Ctrl+Alt+F<this console>) to read the result below.')


def summarise():
    say()
    if not (EVIDENCE / 'session.log').is_file():
        say(f'no evidence was written under {EVIDENCE}; the gate refused before the session')
        return 1
    verified = run([str(VERIFIER), str(EVIDENCE), PROOF_TEXT])
    say(verified.stdout.strip() or verified.stderr.strip())
    if verified.returncode != 0:
        for name in ('guard_seat.log', 'guard_pinned.log', 'session.log'):
            path = EVIDENCE / name
            if path.is_file():
                lines = path.read_text(errors='replace').splitlines()
                say(f'--- last lines of {name}')
                for line in lines[-12:]:
                    say('  ' + line[:160])
        return 1
    archived = run([str(ARCHIVE_VERIFIER)])
    say(archived.stdout.strip() or archived.stderr.strip())
    return archived.returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='check preconditions and list keyboards; run nothing')
    args = parser.parse_args()
    say('Keyboard independence gate, walkthrough')
    say()
    ok = True
    for passed, line in preconditions(require_console=not args.check):
        say(('  ok   ' if passed else '  FAIL ') + line)
        ok = ok and passed
    rows = keyboards()
    say()
    say('Keyboards the kernel reports:')
    for index, (link, node, name, note) in enumerate(rows, 1):
        say(f'  {index}. {name}  ({node}){"  <- " + note if note else ""}')
    mains = [row for row in rows if not row[3]]
    if len(mains) < 2:
        say('  FAIL fewer than two usable keyboards; plug the second one in')
        ok = False
    if not ok:
        say()
        say('Resolve the FAIL lines, then run this again.')
        return 1
    if args.check:
        say()
        say('Preconditions hold. Run this without --check from a text console to start.')
        return 0
    say()
    a = choose(rows, 'Keyboard A, the one you will unplug')
    b = choose(rows, 'Keyboard B, the one that stays', exclude=a)
    explain(a, b)
    say()
    if input("Type 'go' to start, anything else to stop: ").strip() != 'go':
        say('Nothing started.')
        return 1
    env = dict(os.environ, SOPHIA_KEYBOARD_A=a[0], SOPHIA_KEYBOARD_B=b[0])
    env.setdefault('SOPHIA_KEYBOARD_INDEPENDENCE_SEAT', 'seat0')
    result = subprocess.run([str(RUNNER)], env=env)
    say()
    say(f'gate exit status {result.returncode}')
    return summarise() if result.returncode == 0 else (summarise() or 1)


if __name__ == '__main__':
    raise SystemExit(main())

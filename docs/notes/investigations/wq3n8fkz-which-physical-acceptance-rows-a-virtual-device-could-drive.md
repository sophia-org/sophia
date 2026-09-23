---
id: wq3n8fkz
date: 2026-09-20
kind: investigation
status: resolved
tags: [investigation, session, tooling]
---
# Which physical acceptance rows a virtual device could drive

## Question

Eleven open rows carry `@physical`. Several are years-old acceptance work that
has stayed open because it needs a hand on a mouse. XTEST shipped this week --
can it close any of them, and if not, what can?

## XTEST cannot, for two independent reasons

**It is not in a live session -- by default.** `live_session.rs` attaches no
injection policy, so every live client sees XTEST as absent and t093 closed
with discovery deliberately left disabled. That is a default, not a wall: the
adapter is complete, admission is decided per connection, and `--admit-xtest`
attaches a namespace-keyed policy that issues injectors to the session's
clients. (Corrected 2026-09-20; the original read this as unreachable.)

**It is the wrong layer by design.** t139 established that emergency recovery
is the input guard's, a separate process polling libinput directly, so a
synthetic source cannot reach the physical path *by construction*. That is a
property we built on purpose. XTEST drives X clients; it does not drive the
seat, and these rows are about what the seat does.

## What does reach a live session

A virtual device on `/dev/uinput`, created before the session opens its seat so
that udev enumerates it beside the physical devices. This is not speculative:
`tools/benchmark_sophia_glxgears_shake_tty3.sh` already does it, and its header
states the same complaint these rows have -- the benchmark's rule "has only
ever been exercised by a hand on the mouse, which is neither repeatable nor
present on an unattended run".

The tool is `tools/probes/uinput_text_injector.py`. **What it emits today is
narrower than what it registers**, and the gap is exactly what decides this
question:

| | registered as a capability | actually emitted |
| --- | --- | --- |
| `EV_KEY` keys | yes | yes -- `--text`, and `--chord logout\|recovery` |
| `BTN_LEFT` | yes, so libinput sees a mouse | **never** |
| `REL_X` | yes | yes -- `--shake-hz`, alternating |
| `REL_Y` | yes | **never** |
| wheel | no | no |

So today it can type, send two fixed chords, and shake horizontally. It cannot
click, cannot move vertically, and cannot scroll.

## The split

**Driveable now** -- the input these rows need is already emittable, and the
verdict is in records the session already writes (`sophia_live_surface_geometry`,
`sophia_live_session_focus`, `sophia_live_output_authority`,
`sophia_live_native_startup_output`, `sophia_shell_components_shutdown`):

- **t019** -- idle, VT resume, normal logout during actual use. `--chord logout`
  exists; idle is a wait. The closest to free of the eleven.
- **t077** -- signing-dialog lockout: bounded input-delivery and control
  recovery, healthy focus, close, VT and clean shutdown. `--chord recovery` is
  the chord this row is about.
- **t012** -- revalidate installed startup after the output-ownership repair.
  Startup evidence, no pointer at all.
- **t007** -- third-terminal crash repair: both panels shown, terminal bounds
  correct, new terminals admitted. Launching terminals is scriptable; bounds
  are recorded.

**Blocked on one small addition** -- these need a button, vertical motion, or a
wheel, and nothing else:

- **t009** -- panel pointer hit targets, popout anchoring, focus, stop/relaunch.
- **t011** -- scrolling repair, three Kitty windows, vertical scrolling.
- **t060** -- namespace-scoped QueryPointer menu placement and drag.
- **t062** -- explicit pointer-grab promotion after the click-lease repair.
  `sophia_live_explicit_pointer_grab` already records the verdict.
- **t004** -- maximized/fullscreen stacking, if the window operations are
  reached by pointer rather than by a bound shortcut.

**Partly or wholly human:**

- **t081** -- Lom workspace clicks and shortcuts are driveable once buttons
  exist, but "flash-free native panel" is a judgement about rendered output
  over time. A capture harness could bound it; a record cannot.
- **t068** -- Brave's client-internal GPU device mismatch and accelerated video
  without GPU restarts. Third-party browser internals; neither input nor our
  records decide it.

## What the split is worth

**Four rows need no new tooling at all**, and five more need one change to one
file: emit `BTN_LEFT` press and release, emit `REL_Y`, and register and emit a
wheel axis. The capabilities are already declared -- `BTN_LEFT` is registered
precisely so libinput recognises the device -- so this is filling in emitters
beside `shake`, not new machinery, in a file that already has a `--self-test`.

## What this does not claim

That these rows would then be closed. Driving a session is not accepting one.
Each of the four says *accept* or *observe* or *revalidate*, and whether a
record standing in for a person satisfies that is a judgement about the row,
not about the tooling -- one for whoever owns the acceptance, and worth
deciding per row rather than in general. What the tooling changes is that the
evidence can be produced unattended and repeatably, so the person is deciding
from a recorded run rather than performing it.

Note also that `@physical` rows historically pair with an installed release and
real outputs; a virtual input device does not make a session headless, and the
GPU half of t009, t011 and t081 still wants real hardware.

## May a virtual device supply the emergency chord?

t094 decided the general case -- a uinput device is admitted and marked
`virtual=true`, never refused -- and left this one to t145. It is not
hypothetical, and the answer is already implied by two facts.

**Our own tooling does it today.** `uinput_text_injector.py` has
`--chord recovery`, and it is `KEY_LEFTCTRL, KEY_LEFTALT, KEY_BACKSPACE` --
keycode 14 being the exact `EVDEV_KEY_BACKSPACE` that t139 pinned as the
reserved chord. A virtual device can trigger emergency recovery now, and that
is how the recovery path gets exercised at all.

**Refusing it would not raise the bar.** `/dev/uinput` is gated by the `input`
group: `tools/setup_sophia_uinput.sh` installs the udev rule and adds the user
to it. Membership in that group already permits *reading every input device* --
every keystroke, including passwords. That is a strictly greater privilege than
restarting a session. Refusing the chord from a virtual source would leave the
larger capability untouched and break the only legitimate automated use of the
smaller one.

So: **allowed, and marked** -- the same shape t094 chose for devices, for the
same reason. What must not happen is a virtual trigger being mistaken for a
physical one, and t094's `virtual=true|false` on the evidence line is exactly
what prevents it. The rule this note proposes, mirroring the two-keyboard
verifier:

> An emergency chord from a virtual device is a valid **rehearsal** of the
> recovery path and never a valid **acceptance** of it. Any row that accepts
> emergency recovery requires `virtual=false` on the source, as the attended
> two-keyboard verifier requires it on both keyboards.

That keeps t077 and t019 drivable unattended while leaving their acceptance
claim resting on hardware. **This is a security policy recommendation rather
than a settled decision; it is recorded here so it is visible and overrulable
rather than assumed.**

Note the contrast with t139, and that it is not a contradiction. t139 refuses
the chord from a *synthetic* source -- an XTEST injector inside the authority,
which never reaches the seat. A uinput device is not synthetic in that sense:
libinput enumerates it as a device and the physical path genuinely runs. The
two rules protect different boundaries, and a virtual device crosses the one
t139 was never about.

## Connections

- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  where XTEST lives, and why it stays inside an admitted private instance.
- t094, in plan with the obligations lane, covers per-physical-device identity
  through backend and Session ingress: the same territory, and where a virtual
  device's identity would have to be honest.

## Overtaken by events — 2026-09-20, same day

Eight of the eleven rows this note sorted closed within hours of it being
written, and none of them closed the way it proposed. The operator had been
running Sophia/Hagia/Lom as an ordinary dev session for several days; asked
directly, they accepted t004, t007, t009, t011, t012, t019 and t068 on that
use, and t077 on having signed with pinentry several times. Daily driving is
the acceptance these rows were asking for, and it had already happened.

That is worth recording plainly rather than quietly deleting the tables. The
note reasoned carefully about *how to produce evidence unattended* and never
asked whether the evidence already existed. Its own last section came close --
"driving a session is not accepting one" -- and the corollary went unsaid:
**if only a person can accept, ask the person before building the rig.**

What survives:

- **The uinput analysis is still correct**, and still the answer for anything
  that must run unattended or repeatably. The emitter gap in
  `uinput_text_injector.py` (no `BTN_LEFT`, no `REL_Y`, no wheel) is real, and
  the three rows left -- t060, t062, t081 -- are exactly the ones it blocks.
  They are also the three the operator did *not* accept on daily use, which is
  a fair signal they need deliberate exercise rather than incidental use.
- **The XTEST verdict stands** and is the durable half: XTEST is the wrong
  layer for seat behaviour by construction, not by omission.
- **The virtual-chord policy recommendation stands**, unaffected: rehearsal
  yes, acceptance no, `virtual=false` required for any row accepting emergency
  recovery. t145 still owns the decision.

The tables above are kept as written, with this section as their correction.

## The headless half of t147, 2026-09-23

`cargo xtask check xtest-selection` is the scenario t147 asked for, headless:
a production session with `--no-input --admit-xtest` runs the XTEST driver,
which drag-selects in one real xterm and middle-click pastes into another. It
passes on `owner_changes=1 conversions=2 injected_buttons=4`. `--self-test`
fails a blank-row drag (`selection_text_mismatch`), a session without
`--admit-xtest` (XTEST absent), and no middle-click (stdout mismatch). The
gate's judge also has unit tests, so each counter obligation fails on its own
even when the session exits cleanly.

It took two XTEST repairs to get here, t155 (buttons at the origin) and t156
(pointer events at the focus instead of under the pointer), and one reducer
gap: retained evidence dropped `sophia_live_selection`'s counts, which is why
the installed session of 2026-09-21 could not say whether PRIMARY moved.

Still open for t147 after this: one XTEST injection on the installed desktop
so `sophia_live_session_xtest` shows a nonzero count. The 2026-09-21 session
admitted XTEST (`issued=23`) but injected nothing.

## The QEMU half, 2026-09-23

`SOPHIA_QEMU_SCENARIO=xtest-selection tools/qemu_session_harness.sh` boots the
guest with the driver in the image (`/usr/bin/xtest_selection_driver`, built in
release alongside `sophia`, with DejaVu Sans Mono so xterm's cell size is the
one the driver aims by) and runs the headless gate's session on a scanned-out
virtio head, physical input devices present and nothing typed. Nothing is sent
from the host; the guest bounds itself and powers off, and
`tools/verify_qemu_xtest_selection_evidence.sh` reads the serial log: the
markers, the session's application record with the driver's stdout matched
(the driver's own line never reaches the console in `--client` mode), one
bounded completion, `owner_changes>=1 conversions>=2`, and a completed XTEST
record with four buttons and no refusals. `SOPHIA_QEMU_XTEST_ROW=5` drags a
blank row and must fail.

Results (`.artifacts/t147-qemu-xtest-selection/`): the first guest run failed
at `convert_refused` -- the guest's xterm runs in the C locale and offers
STRING, not UTF8_STRING, so the driver now falls back to STRING as a pasting
client does. With that, `green.log` passes with `owner_changes=1
conversions=3 injected_buttons=4` (three conversions: the refused UTF8_STRING,
the STRING read-back, xterm B's paste) and `stdout_match=true`;
`red-blank-row.log` fails with the driver's `selection_text_mismatch` and the
guest's `status=failed reason=xtest_selection_exit`. The QEMU half of t147 is
done.

## Operator step: the hardware half

On the installed desktop, in a terminal on the live display:

```sh
cd ~/dev/sophia && cargo build --offline -p sophia-session --all-features \
    --example xtest_selection_driver
target/debug/examples/xtest_selection_driver; echo
```

The driver opens two xterms, drags the marker row in the first, middle-clicks
the second and prints one line: `status=pass ...` or `status=fail reason=...`.
It needs the session started with `--admit-xtest`, which the installed session
already is (`issued=23`). At the next logout the retained events log carries
`sophia_live_session_xtest ... injected_buttons=4` (or more, if run more than
once); that record with a nonzero count is the hardware half of t147. A
`status=fail` is a finding for t124, and the reason token says which half.

First attempt, 2026-09-23, on the installed release `20260918-d444eba2`:
`status=fail reason=pointer_not_over_target`, the aim landing at (17,46)
inside xterm A's frame with QueryPointer naming no child. That release
predates t155 and t156, so it is the pre-fix behaviour seen on hardware, not
a new finding; the driver needs a release built from `bf43425d` or later.

Second attempt, same day, on release `bf43425d`: the session ended before the
driver ran -- a Super+button on a freshly tiled column hit a session-fatal
lookup in the WM gesture path, unrelated to XTEST
([qvj77ywn](qvj77ywn-a-super-button-on-a-window-between-two-committed-layouts-ends-the-session.md),
fixed). The hardware half waits for the next release.

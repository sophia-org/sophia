---
id: 4m65c17q
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, x11, session]
---
# Primary selection does not reach another client from xterm

## Question

Selecting text with the mouse in xterm and pasting it into another window does
not transfer it. Why?

## Evidence

Reported from a live Hagia session on release `0.1.0-84d906f148d6`, by hand:
select with the mouse in xterm, then middle-click or shift+Insert in a kitty
window. Nothing arrives.

Ruled out already: xterm's `ctrl+shift+c` and `ctrl+shift+v`. Stock xterm 411
binds neither. Its only selection bindings are `Shift <KeyPress> Insert` and
`~Ctrl ~Meta <Btn2Up>`, both `insert-selection(SELECT, CUT_BUFFER0)`, plus
mouse drag to PRIMARY and CUT_BUFFER0. So the earlier key-based attempt never
set CLIPBOARD and never reached Sophia. **The mouse path is different: it is
the ordinary PRIMARY transfer and it should work.**

Not yet established: whether the owner is recorded
(`SetSelectionOwner`), whether the requestor's `ConvertSelection` produces a
`SelectionRequest` to the owner, whether the owner's reply is routed back as
`SelectionNotify`, and whether the two clients being in different namespaces
sends the transfer through the clipboard portal
(`crates/sophia-x-authority/src/clipboard.rs`, which has
`SameNamespace`, `UnknownRequestorNamespace` and `Portal` refusal cases).
CUT_BUFFER0 is a property on the root window and is a second, independent path
worth checking separately.

The live session emitted no selection records, but the only one that exists,
`sophia_live_selection`, is written at teardown, so its absence says nothing.

## What the code says, 2026-09-20

Three of the four questions above are answered, and together they move the
suspect rather than narrow it.

**Two X clients on one display are always in the same namespace, so the
clipboard portal is not implicated.** The namespace belongs to the listener,
not to the connection: `run_x11_core_socket_server(path, namespace)` takes one
and serves every client that connects on it. A cross-namespace transfer
describes two displays, not two clients. So the `SameNamespace`,
`UnknownRequestorNamespace` and `Portal` cases in `clipboard.rs` are not on
the path this report walked, and the question of one namespace against two
answers itself.

**The same-namespace transfer already works, and a gate already proves it.**
`x_server_frontend_routes_selection_notify_to_the_requestor_client`
(`tests/x11_wire/admission_frontend.rs`) runs two real clients over a real
socket through the whole round trip -- SetSelectionOwner, ConvertSelection,
the SelectionRequest routed to the owner, ChangeProperty, SendEvent of
SelectionNotify routed back to the requestor, GetProperty -- and asserts the
bytes arrive. It passes. The routing is in
`connection/protocol_routing.rs`, which sends SelectionRequest to the client
owning the owner window and SelectionNotify to the requestor.

So the earlier note was wrong to say nothing in the gates exercises a
cross-client selection transfer. What is missing is a *real-client* smoke,
which is a different thing and still missing.

The cross-*display* counterpart -- two namespaces, the transfer granted by
the clipboard portal -- is proven as well, in
`tests/x11_wire/clipboard_frontend.rs`, for CLIPBOARD and, since this was
written, for PRIMARY.

**CUT_BUFFER0 works too, and is now covered.** Added
`cut_buffer_zero_on_the_root_is_readable_by_another_client`
(`tests/x11_wire/selection_cut_buffer.rs`): one client writes the buffer on
the root, a different client reads it back with its type and format intact.
The second path is sound, independently of selection ownership, targets,
timestamps and the portal.

## Where that leaves it

Every mechanism this report blamed is demonstrably working. The remaining
possibility is that the transfer never begins -- that **xterm never takes
PRIMARY at all**, because the mouse drag that would make it do so does not
reach xterm as a drag. A selection gesture is a button press, motion while
held, and a release; if any of those is not delivered the way xterm expects,
there is no SetSelectionOwner to route and nothing downstream is at fault.

That is now a testable claim rather than a guess, because XTEST can drive the
gesture. It could not before: this needed a hand on a mouse in a live
session, which is why the original report is a by-hand one.

Read [a key routed to focus is refused when the pointer is over another
client's window](r7k2mvqd-a-key-routed-to-focus-is-refused-when-the-pointer-is-over-another-clients-window.md)
beside this. It is not the same defect -- that one is about keys -- but it is
the same shape: input refused for a reason about the pointer's position, at a
layer no wire profile sees, and invisible to every gate because the wire is
answered.

## Validation and remaining work

- [x] Establish whether one namespace behaves differently from two: they
      cannot differ here, because a display is one namespace.
- [x] Check CUT_BUFFER0 independently: it works, and is now covered.
- [x] Establish that the same-namespace selection round trip works: it does,
      and a socket-level gate already proved it.
- [x] Drive a real mouse selection into a real xterm with XTEST and establish
      whether SetSelectionOwner is ever sent. **It is**, once t155 put XTEST
      buttons where the pointer is: `owner_changes=1`, three runs of three.
      See the section of 2026-09-22 below.
- [x] The WM session: drag and paste pass headless under Hagia with the
      operator's own desktop and Hagia configuration, three of three
      (`owner_changes=1 conversions=2`). See the section of 2026-09-23 below.
- [ ] Explain the original report, a physical drag on the installed desktop
      with Hagia running. The headless XTEST path is healthy with and without
      the WM, so what is left is the physical path -- and one shared defect
      found on the way, the frontend's implicit grab
      ([t158](urxcuj5s-an-implicit-pointer-grab-delivers-by-position-not-to-the-window-that-took-the-press.md)),
      which loses a release made past the text widget's edge. That is a
      candidate the operator can confirm or rule out: does a drag that ends
      inside the text work, and one that runs off the edge fail?
- [x] The paste half -- middle-click in a second xterm. Blocked until t156
      resolved XTEST pointer events against the Engine's scene; now three of
      three pass with `conversions=2`, xterm B asking for PRIMARY and receiving
      it. (Keyboard focus in a no-WM session never moves past the first window
      -- no WM focus, no click focus. Recorded as an observation about keys;
      not this row's scope.)
- [x] Make drag and paste repeatable: `cargo xtask check xtest-selection`
      (t147) runs both halves against two real xterms and reads the verdict
      from the session's own counters; `--self-test` fails a blank-row drag,
      a session without `--admit-xtest`, and no middle-click. The retained
      evidence reducer used to strip `owner_changes` and `conversions`; it now
      keeps them, so an installed session's events log carries the answer.
- [x] Add the real-client smoke. `crates/sophia-session/examples/selection_probe.rs`
      runs two out-of-process x11rb clients against `x11_conformance_host` --
      the production frontend, not a test harness -- and carries the round
      trip: owner None before the claim, the claim returned by
      GetSelectionOwner, ConvertSelection arriving at the owner as a
      SelectionRequest, and the payload bytes back at the requestor. This
      confirms the wire gate from outside the process; it does not speak to
      xterm, which is still the open question above.

## A 300-tick QEMU session saw no SetSelectionOwner at all, 2026-09-20

The two-xterm QEMU session scenario types `sophia` into the focused terminal
and then double-clicks (`qemu_qmp_pointer.py`, `dx, dy, clicks = (40, 18, 2)`).
Across the whole run it recorded `sophia_live_selection schema=1
status=complete owner_changes=0 conversions=0`.

That counter is worth more than its name suggests. It is not a session-side
tally of anything interpreted: `transport.rs:303` sets it from
`trace.major_opcode == 22`, which is SetSelectionOwner on the wire, upstream of
parsing, acceptance and routing. Zero therefore means no such request ever
reached the authority -- not that one was refused or lost downstream.

**This is suggestive and not an answer, because the gesture's aim is not
established.** The QMP pointer moves *relative* by 40,18 from wherever it
already is and clicks twice there; nothing in the evidence says that landed on
the typed text, on blank terminal, or on window chrome. Four button events were
observed and all four routed with `suppressed_no_target_count=0`, so they
reached *a* target, but which surface is not recorded. A double-click on
anything but a word selects nothing, and xterm would then be correct to send
nothing.

Settling it needs the gesture aimed at a known point inside a known xterm,
which is what XTEST buys. `--admit-xtest` cannot simply be added to this
scenario: `config.rs:1061` refuses the flag alongside `--expect-physical-text`
or `--expect-physical-pointer`, because a synthetic source could satisfy an
input proof. That guard is right, and the scenario exists for those proofs. An
XTEST run in QEMU wants its own scenario without them.

Open work is tracked as t124 in `todo.md`.

## A headless attempt, and what stopped it, 2026-09-22

The open row can be driven without a TTY or a guest: `x11_conformance_host
--admit-xtest` is the production frontend on a private display, a real `xterm`
starts against it (fonts and RENDER suffice), `xdotool` issues the XTEST drag,
and `selection_probe PRIMARY` reads the owner from outside the process. That is
the whole apparatus the row asks for, and it was assembled and run here.

It did not reach the answer. The first `xdotool` XTEST request ended the host:
`X11 dispatch ended before its effects were published`, every classification
flag false. Isolated to the trigger, it is not xterm and not an ordinary
disconnect -- a lone `xdotool mousedown 1` on the bare root does it. That is
its own defect, recorded as
[t154](945mtp8i-a-real-xtest-client-that-closes-right-after-a-zero-delay-fakeinput-ends-the-frontend-service.md),
and it blocks this row: the client behaviour that would answer the question is
the one the frontend cannot survive.

So the row above stays open, and its status is unchanged from the QEMU run: no
`SetSelectionOwner` was observed, and that is because the gesture never landed,
not because xterm declined to send one.

**Why it never landed, established once t154 was repaired the same day.** With
the host surviving, the drag was re-run aimed at the text row and the pointer
read back: `QueryPointer` still answered `0,0` over the root. Two things were
wrong with the apparatus, neither of them xterm. `xdotool mousemove` is
`WarpPointer`, not XTEST, and on this host it did not move the pointer, so
every XTEST button fell on the root. And the `x11_conformance_host` example has
no routed-input consumer at all: a `FakeInput` against it hands the input to
the broker and then blocks that connection in the barrier until the client
leaves, so an XTEST motion sent through `wire.py` hung on its own `sync`. The
example host cannot make a gesture land; it was never a viable vehicle for
this row, which the QEMU run's unaimed double-click had in common with it.

The vehicle is the host the XTEST profile already uses:
`native_input_conformance_host --socket … --cookie-file …`, auth
`SOPHIA-PRIVATE-INPUT-1` (`tools/probes/x11_conformance/run.py:80-113`), which
drains and settles routed input and whose `xtest_button_pair` and
`xtest_motion` cases prove delivery to a client window. Attaching a real
`xterm` to it needs an `XAUTHORITY` carrying that cookie; driving the drag
needs XTEST motion from `wire.py`, not `xdotool`. The scripts under
`.artifacts/t124-xtest-service-exit/` are the wrong host and are kept as the
record of that.

## Reproduced headless on the production session, 2026-09-22

The gate's vehicle is the production session itself: its display speaks
MIT-MAGIC-COOKIE-1, so a real xterm connects (the m4 host admits only
SOPHIA-PRIVATE-INPUT-1, which libxcb cannot send). A driver,
`crates/sophia-session/examples/xtest_selection_driver.rs`, is launched as the
session's `--client`, spawns xterm A, waits until `QueryPointer` resolves to it,
and drives an XTEST drag across the text row.

With the aim confirmed, xterm sends no `SetSelectionOwner`:
`sophia_live_selection owner_changes=0`. A protocol trace of xterm shows why,
and it is not xterm: every XTEST button is delivered at the screen origin, so
the drag is a zero-length selection. That is
[t155](csiz9c9x-an-xtest-button-is-delivered-at-the-screen-origin-not-where-the-pointer-is.md),
which blocks this row. It does not explain the original report, which was a
physical drag; once t155 lands the gate can say whether a correctly placed
drag makes xterm take PRIMARY, and if it does, the physical path is where this
row's answer lies.

## xterm takes PRIMARY when the drag is placed, 2026-09-22

With t155 repaired, the driver's drag -- aim confirmed over xterm A, press, six
motions carrying Button1, release, now all at the pointer's position -- makes
xterm send SetSelectionOwner. The session's wire counter reads
`owner_changes=1`; `GetSelectionOwner` names xterm's VT100 widget `0x400016`;
`ConvertSelection` returns the 26 bytes of the marker row. Three runs of three,
each `bounded_complete`. Before t155 the same run read `owner_changes=0`, so
the gate shows red and green on that repair as well.

That answers this row's question for the headless path and leaves the original
report unexplained: a physical drag does not use the XTEST injector. The
remaining row is the physical path, and the paste half waits on t156.

## Under Hagia, headless, with the operator's configuration, 2026-09-23

The same driver under the WM the report was made against. The session is a
normal one -- the daily profile declares applications, and a normal session
refuses `--client` -- so the driver runs as the only startup application:

```sh
# $CFG: a fresh 0700 directory holding copies of ~/.config/{sophia,hagia,lom},
# with the six `workspace N output-key=K` lines removed from desktop.kdl.
# The headless output matches no configured connector, so its policy key is 0
# and Hagia refuses assigned workspaces on it; that is the profile on the
# wrong head, not a defect. The WM is the release the profile names.
env -u DISPLAY -u XAUTHORITY -u WAYLAND_DISPLAY -u SOPHIA_SHELL_CONFIG \
    XDG_CONFIG_HOME=$CFG target/debug/sophia session run --display=:91 \
    --no-input --admit-xtest \
    --wm-process=$HOME/.local/state/sophia/desktop-releases/20260918-d444eba2/hagia \
    --wm-interface=sophia_wm_v1 \
    --session-app=t124driver=$PWD/target/debug/examples/xtest_selection_driver \
    --session-start=t124driver --exit-when-startup-exits --max-runtime-ms=120000
```

Getting there found four things, in order (`.artifacts/t124-hagia-headless/`):

1. A debug build with `--wm-process` panicked at startup: component prepare
   read the session profile through an accessor that asserted it was still
   Prepared, and WM startup had already activated it. Fixed; release builds
   never saw it.
2. With no configuration at all, Hagia's default bindings name session
   slots Sophia's default profile does not admit, and Hagia restart-loops
   (t159). The operator's pair does not have this problem.
3. Hagia tiled B beside A and slid A left. The driver's drag, aimed with A's
   geometry from before B mapped, ran off A into B -- and the session's XTEST
   seam re-hit-tested every event, so B got the release and A never claimed
   PRIMARY. The seam now holds the pressed surface until the last release,
   as the implicit grab requires; the driver re-reads A after B settles and
   takes the text row from xterm's size hints rather than height / 8, since
   Hagia had made A 702 px tall.
4. With the release kept on A's surface but past its edge, xterm still
   claimed nothing: the frontend delivers by position inside the surface, so
   the release reached the shell window, not the text widget. That is
   [t158](urxcuj5s-an-implicit-pointer-grab-delivers-by-position-not-to-the-window-that-took-the-press.md),
   shared with physical input and left open; the driver's default drag ends
   inside A and `--overshoot` is its red probe.

With those, three runs of three pass under Hagia: xterm A claims PRIMARY on
the drag, the driver reads the marker back, and xterm B asks for PRIMARY on
the middle-click (`owner_changes=1 conversions=2`, `injected_buttons=4`).
Sophia's selection path and the WM session are cleared for this row. What
remains is the physical path, with t158 as the named suspect.

## On the installed desktop, 2026-09-23

The same driver on the operator's live session (release `30acf6af`, Hagia,
two outputs) passes: XTEST drag in xterm A, PRIMARY taken, text read back,
middle-click in xterm B. The first attempt on the 2026-09-18 release failed
the aim check as the pre-fix headless runs did, and the second ended the
session on an unrelated WM gesture defect
([qvj77ywn](qvj77ywn-a-super-button-on-a-window-between-two-committed-layouts-ends-the-session.md),
fixed in `30acf6af`). So on hardware, under the WM the report was made
against, a synthetic drag and paste work. Only the physical drag itself is
left, and it is the operator's to try: a text drag that ends inside xterm's
text area, and one that runs off the window's edge.

## Connections

- [A kitty window waits out a four-second layout budget before it opens](cnbxdj48-a-kitty-window-waits-out-a-four-second-layout-budget-before-it-opens.md) --
  same session, and the same reading method.

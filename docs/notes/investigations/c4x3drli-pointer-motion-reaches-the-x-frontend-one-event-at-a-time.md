---
id: c4x3drli
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, rendering, input, session]
---
# Pointer motion reaches the X frontend one event at a time

## Question

After the composition cadence was repaired, a client on a 120Hz head runs at
~118 FPS and still drops to ~60 while the pointer is moved continuously. Why
does input cost exactly half the cadence, and is coalescing motion the repair?

## Evidence

Measured on the reference host, discrete Navi 31 driving DP-1 at 120Hz beside
DP-2 at 60Hz, with `glxgears`:

| condition | before the cadence repairs | after |
| --- | --- | --- |
| offscreen, not composited | ~9,400 FPS | ~9,000 FPS |
| onscreen, idle | ~57 FPS | **~118 FPS** |
| onscreen, pointer moving | ~31 FPS | **~60 FPS** |

Offscreen is unchanged, so neither the client nor the GPU is the constraint in
any of these. The cadence repairs
(`310cf886`, `34d1f7e5`, `19ff37a0`) doubled both the ceiling and the floor and
removed one halving. **A second halving survives them**, in the same exact
ratio, which is what this investigates.

A slower new `kitty` window under the same load is consistent with authority
work queueing behind the same contention, and is not separately measured.

### The mechanism, from the source

`live_session/input.rs:1672-1699` builds one `RoutedInputRequest` and one
`XAuthorityRoutedInput` **per motion event**, mints a fresh
`XAuthorityInputDeliveryId` for each, and calls `route_bounded_input` per
event. Every motion packet therefore becomes an individual ordered ingress send
with its own acknowledgement, which is what keeps authority work continuously
available to the owner loop.

Three kinds of motion are already coalesced; the one that reaches X is not:

- cursor-plane moves fold N motions into one dirty flag
  (`owner_loop/physical_input_phase.rs:440-449`, `cursor_moves_coalesced`)
- WM hover policy inputs merge into the queue tail
  (`live_session/input/pointer_focus.rs:32-50`)
- **X delivery does not coalesce at all**

`RoutedInputCoalescer` (`sophia-engine/src/input/routed.rs:40-115`) already
implements exactly the required behaviour -- latest-wins motion per target
surface, flushed on frame boundary, state-changing input, target crossing,
drag, grab and focus change -- and is proven by four tests
(`sophia-engine/tests/input.rs:334-419`). It is **dead code**: nothing in
`sophia-session` constructs one.

### Measured on the repeatable rig

`tools/benchmark_sophia_glxgears_shake_tty3.sh` now drives a virtual mouse at
1 kHz through the standalone benchmark, so the perturbation is measured without
a hand. On the reference host, `glxgears` in the standalone single-output path:

| condition | client FPS |
| --- | --- |
| idle | 59.7 |
| 1 kHz pointer shake | 41.7 (samples 43.3 / 38.0 / 43.8) |

44,286 motion events injected at exactly 1000.0 Hz. The standalone path idles
at 60, not the 118 of the dual-monitor desktop, so this is a smaller drop than
the desktop's halving -- but the same mechanism: continuous motion perturbs the
cadence. This is the before-number the coalescer repair has to lift back toward
60.

### Measured after the repair

The coalescer is wired and behaves as designed: **19,952 motion packets became
914 deliveries** in one 20-second shaken run, about one per composed frame.

| condition | client FPS |
| --- | --- |
| idle | 59.7 |
| shaken, per-event routing | 41.7 |
| shaken, coalesced | **48.1** |

So the hypothesis is **partly confirmed and not sufficient**. Per-event routing
was a real cost -- 15% of the frame rate came back -- but roughly 11.6 FPS is
still lost under motion, and the input phase is no longer where it goes:
`max_input_phase_msec=0`, with composition itself running at 47.3 a second
against 59.7 idle.

The residual points at the cursor plane, not at input routing. In the same run
`sophia_live_session_cursor schema=6 path=atomic_plane` reports 890 hardware
updates (about one per frame) against 7,538 queued, **234 `cursor_only`
commits** -- atomic commits carrying nothing but the cursor -- and
`updates_primary_in_flight=48`, cursor commits made while a primary flip was
outstanding, which the benchmark's own rule says must be zero on the atomic
path. `max_motion_to_submit_msec=18` is longer than the 16.7 ms frame. A
`p95_frame_msec` of 33.4, exactly two frame intervals, is the shape of a frame
occasionally missed rather than a uniform slowdown.

That is a separate mechanism from this note's, and is read out in
[[0lamaqyi-a-blocking-cursor-only-commit-spends-the-vblank-the-next-frame-needed]]:
the cursor-only commit blocks until a vblank, so one issued while a client is
drawing spends the vblank that client's next frame needed. Tracked as t120.

## Finding and resolution

Not established. The per-event routing is confirmed and the coalescer exists
unused, but that motion volume is *the* cause of the surviving halving is a
hypothesis the measurements above are consistent with rather than proof. The
cadence repairs removed a halving whose mechanism was read directly from the
scheduling guard; this one has no equivalent reading yet.

What the evidence cannot currently say is which: the owner loop is spending its
turns on motion delivery, or the composed frame is retiring a tick late behind
the acknowledgement traffic. The scheduler record distinguishes them --
`cadence_deferred_batches`, `cadence_repaints`, `merged_batches`,
`max_input_phase_msec` -- but reduction retains only `frame_interval_usec`.
Confirming the mechanism before the repair means admitting those counters, or
measuring with the reporter under a synthetic shake.

The repair, if the hypothesis holds: thread a `RoutedInputCoalescer` through
the physical input routing context as `&mut`, the way `next_input_delivery`
and `pointer_focus_handoff` already are (`live_session/input.rs:394-398`);
pass a frame-boundary flag derived from `primary_frame_pacer.repaint_due`,
which is in scope at the drain site because `physical_input_phase.rs` includes
`lifecycle.rs`; buffer motion and flush on that boundary or on a barrier the
coalescer already names. A delivery id and route lease are then minted once per
flushed motion rather than once per event.

**This is the session's most latency-sensitive path**, interleaved with pointer
grabs, focus handoff, route leases and saturation accounting, and it runs a
live desktop. A fault here presents as a pointer that feels wrong or a click
landing on the wrong surface, not as a failing test. The coalescer's own tests
say when it must flush; nothing yet tests it through the live routing path, and
that integration is where the risk is.

## Validation and remaining work

- [x] Confirm the mechanism before repairing it. The shake rig answered it:
      per-event routing cost real frames (41.7 to 48.1 FPS recovered by
      coalescing alone) but was not the whole cost, and `max_input_phase_msec`
      fell to zero while the rest remained -- which is what pointed at the
      cursor plane and became t120.
- [x] Thread the coalescer and flush at the frame boundary. Landed; release is
      bounded by the frame interval as well as the pacer, because a session
      composing from client submissions requests almost no paced repaints and
      waiting only on the pacer delivered no motion at all.
- [x] Test the integration through the live routing path, not only the
      coalescer in isolation: five tests drive it, covering one delivery per
      frame, release on the clock with no repaint requested, a button after its
      motion, and a crossing delivering both surfaces in order.
- [x] The reporter's rule -- at least 55 FPS with a p95 of at most 25 ms --
      passes under the scripted shake once t120's gate lands: 59.9 FPS at a
      p95 of 16.7 ms, `status=pass` end to end.
- [x] One manual hand-on-mouse run, on the installed desktop at `a58800c3`:
      `glxgears` holds 117.8 FPS on the 120Hz head while the pointer moves --
      the halving this note opened on (118 idle, 60 moving) is gone -- and
      pointer feel is ordinary: motion inside Kitty and clicking inside the
      browser both land as they should.

Open work is tracked as t116 in `todo.md`.

## Connections

- [glxgears runs slow under dual-monitor pacing](lz1rbbmr-glxgears-runs-slow-under-dual-monitor-pacing.md) —
  the original 30 FPS report, whose `heads.first()` diagnosis was half right:
  the ordering was deterministic, and the refresh it carried was fabricated.

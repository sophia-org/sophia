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

- [ ] Confirm the mechanism before repairing it: admit the scheduler counters
      to reduction, or measure under the synthetic shake, and establish whether
      the loop is spending turns on delivery or retiring frames late.
- [ ] Thread the coalescer and flush at the frame boundary.
- [ ] Test the integration through the live routing path, not only the
      coalescer in isolation: a grab, a focus change and a target crossing must
      each still deliver their motion in order.
- [ ] Re-measure: `glxgears` should hold ~118 FPS under continuous motion, and
      the reporter's existing rule -- at least 55 FPS with a p95 of at most
      25 ms -- should pass under a scripted shake.

Open work is tracked as t116 in `todo.md`.

## Connections

- [glxgears runs slow under dual-monitor pacing](lz1rbbmr-glxgears-runs-slow-under-dual-monitor-pacing.md) —
  the original 30 FPS report, whose `heads.first()` diagnosis was half right:
  the ordering was deterministic, and the refresh it carried was fabricated.

---
id: 0lamaqyi
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, rendering, cursor, kms]
---
# A blocking cursor-only commit spends the vblank the next frame needed

## Question

With pointer motion coalesced ([[c4x3drli-pointer-motion-reaches-the-x-frontend-one-event-at-a-time]]),
a shaken client recovered from 41.7 to 48.1 FPS but not to its 59.7 idle rate,
and the input phase was no longer the cost: `max_input_phase_msec=0` while
composition itself ran at 47.3 a second. What is still paying?

## Evidence

Reference host, `just glxgears-shake`, 1 kHz synthetic shake over 20 s. The
cursor record from that run:

```
sophia_live_session_cursor schema=6 path=atomic_plane plane=accepted
  moves_coalesced=31 max_motion_to_submit_msec=18 max_update_msec=0
  updates_primary_in_flight=48 hardware_updates=890 queued=7538
  rides=564 cursor_only=234 combined_drops=0 fallbacks=0
```

and `sophia_live_present_cadence ... mean_fps=48.162 p95_frame_msec=33.363`.

A p95 of 33.4 ms is **exactly two frame intervals** at this head's 16.7 ms.
That is the shape of a frame occasionally missing a whole vblank, not of a
uniformly slower pipeline.

### The mechanism, from the source

`service_native` (`production_visual_runtime/service.rs:299-370`), which runs
on the owner loop, ends every service cycle with `service_idle_atomic_cursors`.
That reaches `service_pending_atomic_cursors`
(`persistent_native_scanout/cursor.rs:355`): for a head with a pending cursor,
no frame prepared or in flight, and a free CRTC, `plan_cursor_commit` answers
`CommitCursorOnly` and the owner submits
`build_native_cursor_only_atomic_request` — built **`.blocking()` with
`.without_page_flip_event()`** (`native_primary_plane/request.rs:394`).

`submit_native_cursor_only_commit` (`native_scanout/prepare.rs:678`) says so
plainly: it "returns when the commit has been applied, because it blocks."
Without `NONBLOCK` the ioctl returns only once the kernel has applied the
commit at the next vblank. **Each of those 234 commits held the owner loop for
up to one refresh.** They are issued precisely in the gap between a frame
retiring and the next Present arriving, so a client frame that arrives during
the hold cannot be composed until the ioctl returns — and it slips a vblank.
234 holds of up to 16.7 ms inside 20 s is the order of the missing fifth.

Nothing was wrong with the *decision*. `cursor_transaction_owner.rs` and
`validation/tla/CursorPlaneTransactionOwner.tla` settle that a cursor must not
wait on a client that "may not draw again for most of a second", and TLC
refuses the model without the cursor-only commit. Blocking was chosen so the
owner "never has to guess at a completion it did not observe", a cursor commit
carrying no page-flip event by design. Both hold **for an idle client**. For a
drawing one the next frame is a refresh away and carries the cursor for free —
564 rides did exactly that — so the cursor-only commit is pure cost.

Neither reference compositor blocks its own thread on a cursor move: smithay's
`page_flip` is always `NONBLOCK` and a cursor-only update is a partial frame
through it (`~/src/smithay/src/backend/drm/surface/atomic.rs:900`), and the X
modesetting driver uses the asynchronous legacy `drmModeMoveCursor`
(`~/src/xserver/hw/xfree86/drivers/video/modesetting/drmmode_display.c:1641`).

### A second finding: a counter that cannot mean what its name says

`updates_primary_in_flight=48` tripped the benchmark's rule that an atomic run
must report zero, "an atomic cursor committed while a flip was in flight". It
cannot mean that. The counter is incremented only on the legacy branch
(`cursor.rs:221`), after the atomic path has already returned at `:195`; and
the cursor plane is taken **at readiness, after the first frames**
(`lifecycle.rs:1340-1362`) — in this run the switch was logged at
`session.log:70`, after motion was first observed at line 31. The 48 are
ordinary ioctl updates from before the switch. The record's name and the
reporter's rule both implied a defect that does not exist.

## Finding and resolution

**Established.** A cursor-only commit blocks until a vblank, and issuing one
while a client is drawing spends the vblank that client's next frame needed.

The repair keeps the mechanism and gates the policy: a cursor-only commit is
admitted only while the client is **quiet** — no primary retired on that head
for two refresh intervals (`cursor_only_quiet`). While frames flow the pending
position waits for the frame that will carry it, which is the same waiting the
busy-CRTC arm already does and never drops a position. When a client stops,
the gate opens within two refreshes and idle behaviour is exactly as before.

The model carries it: `CursorPlaneTransactionOwner.tla` gains a `quiet`
variable, a fair `Quiesce` action, and the conjunct on `CommitCursorOnly`.
Fairness on quiescing rather than on drawing is what keeps
`PendingCursorEventuallyCommits` true — a drawing client carries the cursor on
its own commits, a stopped one eventually goes quiet.

The blocking submit is now timed (`cursor_only_max_msec`,
`cursor_only_total_msec`), so the claim that what the gate still admits costs
nothing is measured rather than argued. The miscounted field is renamed
`legacy_updates_primary_in_flight`, the record is `schema=7`, and the
reporter's phantom rule is deleted — whether motion perturbs pacing is the
cadence rule's judgement, which measures it directly.

## Validation and remaining work

- [x] Planner and quiet-window rules, with the 60/120Hz boundaries and the
      unknown-refresh fallback (`tests/cursor_transaction_owner.rs`).
- [x] TLC passes every invariant and the liveness property under the gate.
- [x] Reporter regression check covers the rename, the new cost fields and a
      schema-7 record; conformance reads schema 7.
- [x] Physical, on the rig: `cursor_only=0` and `cursor_only_total_msec=0`
      against 234 before, `rides` up from 564 to 721, client 59.57 FPS against
      a 59.68 idle baseline, and `p95_frame_msec` 16.686 -- one frame interval
      where it had been exactly two. The report passes end to end.
- [x] Physical, by hand, on the installed desktop at `a58800c3`: the pointer
      tracks normally over an idle Kitty window, which is the case the
      cursor-only commit exists for and the one the gate could have broken.
- [x] Read the same counters from an installed Hagia session at teardown. The
      desktop does **not** repeat the rig's zero, and the difference is the
      point: a 285-second Hagia session at `a58800c3` reported
      `cursor_only_total_msec=9416` with `cursor_only_max_msec=8` -- 3.3% of
      wall time spent in blocking cursor-only commits, each at most one 120Hz
      vblank. The rig reads zero because `glxgears` never stops drawing, so
      the gate never opens there; a desktop idles constantly, so it opens
      often. These are the commits that move the pointer on an idle desktop,
      which is the reason the commit exists and what TLC refuses the model
      without.
- [ ] Whether that 3.3% is worth removing. It is not obviously harmful: the
      gate closes the moment a client draws, so at most one commit can straddle
      the resumption of drawing, and the same session held 117.8 FPS under
      continuous pointer motion with ordinary pointer feel. The non-blocking
      event-carrying commit would remove it at the cost of teaching the
      page-flip reader to ignore cursor completions and the owner to track a
      second outstanding commit kind -- a redesign the model would need
      reworking for. Warranted only if a latency measurement, not a wall-clock
      share, shows it costing something.

Open work is tracked as t120 in `todo.md`.

## Connections

- [Pointer motion reaches the X frontend one event at a time](c4x3drli-pointer-motion-reaches-the-x-frontend-one-event-at-a-time.md) —
  the coalescing repair that removed the first cost and exposed this one.

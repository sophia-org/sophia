---
id: cnbxdj48
date: 2026-09-19
kind: investigation
status: accepted
tags: [investigation, session, wm, x11]
---
# A kitty window waits out a four-second layout budget before it opens

## Question

Why does a kitty window take seconds to appear, when glxgears launched in the
same session appears at once?

## Evidence

Live Hagia session on release `0.1.0-69ffd0900000`, three launches: two kitty
windows and one glxgears. Read from the session's reduced record, boot-relative
milliseconds.

| launch | map request | layout commits | first presented | map → shown |
| --- | --- | --- | --- | --- |
| kitty #1 | 757892083 | 757896093 | 757896129 | **4,046 ms** |
| kitty #2 | 760315955 | 760319965 | 760319972 | **4,017 ms** |
| glxgears | 760324267 | 760324276 | 760324305 | 38 ms |

Both kitty launches stalled for exactly the launch epoch's `timeout_msec=4000`
and then committed on a retry. Records were stripped by the reduction, so the
shape had to be inferred: the `sophia_live_resize_epoch schema=2` line that
opened each stall carried `transaction` and `timeout_msec` and nothing else,
and the `sophia_live_wm` line that ended it carried `preserved_layout=true` and
no status. In the source those are `status=held` (`wm/layout.rs:1039`) and
`status=layout_timeout` (`:1331`).

### What kitty does that glxgears does not

Kitty's request stream: create an input-only window, create two input-output
windows, destroy one, **Present a frame at its own size (946×1038), then
MapWindow** -- the Present is one request ahead of the map (transactions 15207
and 15210 for kitty #2). Its next two Presents, at the size the WM proposed
(1266×1398), arrive **76 ms** after the map. glxgears creates one window, maps
it, and presents 5 ms later at the proposed size.

### What happened during the four seconds

Nothing. Between 760316031 and 760319958:

- zero `sophia_live_session_present` and `sophia_live_session_scanout` records
  (seven in the four seconds before the map);
- zero `sophia_live_session_input_routing` records (four before);
- the once-a-second cadence and resource samples continued, so the owner loop
  was alive.

The first kitty's own Present 15270 -- a window that was already managed and
visible, with no part in the admission -- was accepted at 760316244 and
completed at 760320022. The owner loop composited nothing, routed no input and
took no authority work for the life of the epoch.

## Finding

Two defects, one of which is a regression from this morning.

**The owner loop livelocks behind a pending layout epoch.** The paced repaint
yields to a pending layout (`owner_loop/authority.rs`, the `layout.pending.is_none()`
guard on the repaint), and nothing in that refusal moves the pacer's deadline,
so `repaint_due` stays true. The paced preemption read `repaint_due` alone and
took every owner turn for frame service, and `take_authority_work` returned
`Service` every cycle. The epoch can only end on authority work, because the
frame it waits for arrives as an authority batch. `19ff37a0` ("Stop halving
composition whenever input keeps arriving") removed the alternation guard from
the paced path that morning; the guard had been covering this by accident, and
the livelock was what remained. Every held epoch since then -- any launch or
resize the WM had to wait on -- froze composition and input until its deadline.

**A frame presented before the map leaves a safe size behind.** The escaped
pre-map Present is skipped by production once the map is acknowledged
(`310d483b`), but the layout still observed it: `record_safe_observation` ran,
and the emitter that would have said so is guarded on `surface_requires_admission`,
which a pre-map surface fails. The launch epoch then found
`safe_size = Some(946×1038)` and held the gate on the surface instead of
deferring it out as it defers a window that has drawn nothing. That is why
kitty's epoch was held at all and glxgears' committed in the same millisecond.
Reproduced by `a_frame_presented_before_the_map_does_not_hold_the_launch_epoch`,
which failed with `Some(946×1038)` where `None` was required.

The two compound: the second defect creates the held epoch, the first makes it
unrecoverable until its deadline. With only the first fixed, kitty would open
in about 80 ms; with both, about 40 ms, like glxgears.

## Repair

- `paced_repaint_runnable(layout_settled, topology_settled)` in `owner_loop.rs`
  is read by the preemption, both wait caps and the repaint itself, so the
  three cannot drift. A repaint that cannot run is not a reason to take the
  turn from the work that would let it run, and not a reason to cap the wait
  at zero. Pinned by `a_repaint_that_cannot_run_does_not_take_the_turn_from_authority`.
- `observe_authority_batch` skips a transaction whose surface
  `present_escaped_admission`: nobody will see that frame, so it is not an
  extent to resize from. Pinned by the reproduction above and by
  `a_launch_that_never_presented_before_mapping_commits_its_epoch_at_once`
  (the glxgears control) and `a_correctly_sized_frame_satisfies_a_held_launch_epoch`.
- The reduction admits the epoch's states and counts through a new
  `diagnostics/layout_epoch.rs` table -- `held`, `layout_timeout`, `visual_armed`,
  `queue_aborted`, `surfaces=`, `deferred=`, `rollback_transaction=` and the
  rest -- so the next stall reads as a stall, not as a transaction number.

## Validation and remaining work

- [x] Both defects reproduced in unit tests before the fix and pass after.
- [x] Physical, live Hagia session on release `0.1.0-84d906f148d6`. The
      startup kitty: map 762490733, presented 762490842, **109 ms**. A second
      kitty opened by hand: map 762556111, presented 762556212, **101 ms**.
      Both launch epochs read `surfaces=0 deferred=1` and committed within
      2 ms; both resize epochs that followed read `surfaces=1` and committed
      in the millisecond they were held, `matched_surfaces=1`. No
      `layout_timeout` in the session. During the second launch the owner
      loop composited seven frames in the 300 ms after the map, where the old
      build composited none. The remainder is kitty's own render time, about
      70 ms from map to its correctly sized frame.
- [x] A third client, ghostty, confirms the repair is not kitty-specific: its
      toplevel maps at 762606946 and presents at 762607015, **69 ms**, with
      the same `deferred=1` launch epoch and no timeout. Its two input-only
      helper windows are `ClientPositioned`, so admission never governs them
      and only the toplevel is admitted.
- [ ] The evidence that a held epoch froze input and composition is
      circumstantial (record counts). A held epoch's owner-loop starvation is
      not measured directly; if a stall recurs, `sophia_live_session` tick
      counts across the stall would show it.

Closed as t123 in `done-2026-09.md`. The remaining item above is an
observation about this note's evidence, not open work.

## Connections

- [An offscreen client is never throttled and its evidence evicts everything else](12tnf6wc-an-offscreen-client-is-never-throttled-and-its-evidence-evicts-everything-else.md) --
  the session this was read from was the confirmation run for that repair.
- [Pointer motion reaches the X frontend one event at a time](c4x3drli-pointer-motion-reaches-the-x-frontend-one-event-at-a-time.md) --
  `19ff37a0` belongs to that work; the halving it removed and the livelock it
  left are two faces of one preemption decision.

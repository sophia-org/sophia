---
id: 12tnf6wc
date: 2026-09-19
kind: investigation
status: awaiting-physical-acceptance
tags: [investigation, rendering, x11, tooling]
---
# An offscreen client is never throttled and its evidence evicts everything else

## Question

While checking whether evidence volume contributes to a client halving under
pointer motion, one log rotation held 64,712 records spanning 836 milliseconds.
What produces 82,000 records a second, and does it cost the compositor?

## Evidence

Session `00000001789834828747-f28d1fc6-1467-43c1-aaad-7b5f1887da73`, on the
reference host, `glxgears` moved offscreen for a rate comparison.

One rotation, `events.1.log`:

| record | count |
| --- | --- |
| `sophia_x_present_delivery` | 57,800 |
| `sophia_x_present_submission` | 6,927 |
| everything else | 32 |

- 99.9% from one client, `client=3`.
- Submission `serial` advanced 7,553 in the same 836 ms — an independent,
  client-side counter agreeing with the timestamps. **~8,300 `PresentPixmap`
  requests a second is real**, not a measurement artifact.
- `present_accepted` (`x-authority/.../registry/present.rs:123`) is one per
  accepted present, so that is the client's actual submission rate. It matches
  the ~9,000 FPS `glxgears` reports offscreen.
- Each delivery walks four statuses (`ready`, `queued`, `write_started`,
  `written`) for each of two kinds (`complete`, `idle`): **eight evidence
  records per presented frame, by design** (`diagnostics/x_lifecycle.rs:36-40`
  admits all of them).
- All four surviving rotations are this burst. Every input-routing record from
  the pointer shake that preceded it — the evidence the check was for — had
  been evicted.

### Where the completion comes from

`CompleteNotify` is routed from the session on retirement
(`live_session/presentation.rs:232-262`), with the buffer's disposition
choosing the mode: `Copy` or `Retained`/`Flipped` for a composited or scanned
frame, and **`Skipped` for one that was not composited at all**. The present
path itself has no visibility check (`registry/present.rs`).

An onscreen client is therefore paced by composition — ~110 completions a
second on a 120 Hz head, and the client reports that. A `Skipped` present
completes as soon as it is observed, so an offscreen client is paced by nothing
and runs at its render rate. Each of those frames still costs owner-loop
present handling, an X-thread `CompleteNotify`, and nine records.

### The evidence layer

`scanout_diagnostics::layer()` (`sophia-cli/src/scanout_diagnostics.rs:11`)
filters by **target, not level**, so every `tracing::debug!` on
`sophia_application_evidence` fires at the default Info level. `on_event`
(`:22-47`) then formats the message *before* testing the record name, so the
string is built for every event on the target whether or not it is kept.
The X frontend runs on its own thread (`live_session.rs:706`), so this lands on
X protocol handling rather than the owner loop.

## Finding and resolution

**Evidence volume is not the halving mechanism.** Onscreen, the whole
apparatus is on the order of a thousand records a second — a modest cost on a
thread that is not the owner loop. The check this note began as is answered.

Two defects fell out of it:

1. **A `Skipped` present should be paced, not completed immediately.** Every
   other compositor throttles an occluded or unmapped client to the frame
   cadence; this one lets it run at its render rate and pays for every frame.
   The repair is to defer `Skipped` completions to the next frame tick, the
   same way retained ones already wait for retirement. Bounded by the existing
   pending count so a client cannot queue unboundedly against a window nobody
   can see.
2. **Retention has no per-kind bound.** One client's burst rotates out every
   other record kind in seconds. A cap per record name per rotation — the
   per-frame kinds are the only ones that can reach this volume — would let a
   burst coexist with the sparse records that explain a session.

A third, smaller: the layer could check the record name from a structured
field rather than formatting the message first. Cheap, and only worth doing
alongside the others.

## Repair

**A skipped Present is now paced to the head's refresh.** The three gates in
`drive_gpu_presentation` that establish a candidate cannot reach a screen park
it instead of settling it in the owner pass its request arrived in
(`production_present_scheduler/frame_tick.rs`, a sibling of the
first-visibility park it is modelled on). `service_first_visibility_presentations`
releases them once per owner pass, and the owner's wait was generalised from
translation deadlines alone to every frame deadline, so a session with nothing
else to do still wakes at the tick.

The throttle is the withheld Idle, not the delayed Complete. A parked candidate
keeps the client's buffer, so the client blocks on its own back buffers exactly
as a visible one blocks on retirement. That is also why the queue stays small:
Mesa keeps four back buffers, so a conforming client cannot park more than
that. `FRAME_TICK_PARKED_PER_SURFACE = 8` bounds one that does not wait, by
settling its oldest early rather than by inventing a second way to unwind a
candidate.

Parked candidates are moved to the back of the queue. `poll_gate` pushes each
newly eligible candidate to the front, so leaving them in place would have
stacked them in reverse arrival order and made both the tick and the overflow
bound take the newest first.

The display clock is not invented. A paced completion still carries the last
real display sample: X's fake vblank is a separate MSC domain that the server
reconciles per window when a CRTC appears, and Sophia has no such
reconciliation, so a fabricated MSC would break monotonicity the moment the
surface returned to a head.

**Retention now bounds each record name's share of a segment.**
`NAME_SEGMENT_SHARE` is a quarter of the 15 MiB segment, tracked per name on
the capture worker and reset at rotation
(`diagnostics/capture/budget.rs`). Two flooding kinds therefore leave half a
segment for everything else. The first record a name loses is reported where it
happens as a `sophia_session_record_budget schema=1 status=share_spent`
record; a segment that closes with refused names records the counts at the head
of the next one; and the health record carries a `suppressed=` total. A bounded
log cannot be mistaken for a quiet session.

The rotation-only account was not enough, which the confirmation run showed:
`sophia_x_present_delivery` spent its share in about half a minute of onscreen
`glxgears` -- eight records per presented frame at 118 frames a second -- and
the session then suppressed 58,376 further records without ever rotating, so no
per-name record was ever written and only the health total said anything. The
share itself is left as it is. A segment holds about two minutes of that
workload whatever the policy, so truncating the dominant kind is what buys the
other kinds their history; what was wrong was doing it quietly.

The reference is not what was copied. Xorg queues an offscreen window's Present
on a fake vblank timer that runs at **1 Hz**
(`~/src/xserver/Xext/present/present_fake.c:116-140`) and completes it `Copy`;
`PresentCompleteModeSkip` there means a *scrapped* vblank, which is Sophia's
supersession case. Pacing to the head's refresh is what "throttled like an
onscreen one" asks for, and the interval is one call if a coarser choice is
ever wanted.

## Validation and remaining work

- [x] Defer `Skipped` completions to the frame tick. Covered by
      `tests/support/present_frame_tick.rs`: a parked candidate is ineligible
      and is not layout-deferred, a burst shares one tick and settles oldest
      first, the per-surface bound settles the oldest early, and a topology
      escalation takes the parked set too.
- [x] Bound per-kind retention. Covered by
      `diagnostics/capture/budget/tests.rs`, and the existing capture bound
      test now asserts an ordinary session suppresses nothing.
- [ ] Physical confirmation: an offscreen `glxgears` reports the head's cadence
      rather than ~9,000 FPS, `sophia_live_present_scheduler schema=2` shows
      `paced_skips` in the thousands with `frame_tick_overflows=0`, and the
      window resumes its onscreen rate when dragged back.
- [x] Re-run the evidence-volume check onscreen under the synthetic shake once
      the records can survive long enough to read. Answered 2026-09-22 from
      retained post-repair sessions and a measurement at the shake's own rate;
      see the section below. The share stands unchanged.
- [ ] Still unwitnessed from the row above: that a window **resumes its
      onscreen rate when dragged back**. The scheduler counters and the
      offscreen rate are evidenced; the drag itself needs a hand.

Open work is tracked as t118 in `todo.md`.

## The share judged, 2026-09-22

The question this left open was whether a quarter of the segment is the right
size for one record name, once a shake's routing records could be read beside
the flood. Two independent answers, and they agree.

**What the repair actually did, from retained evidence.** Session
`00000001789950095665-f150c787` (2026-09-20) rotated three full 15 MiB
segments. In every one, `sophia_x_present_delivery` stops at its share almost
exactly -- 3,932,186, 3,932,192 and 3,932,350 bytes against a
`NAME_SEGMENT_SHARE` of 3,932,160, the excess being the single record that
crosses the line and is admitted in full because charging happens on admission.
`sophia_x_present_submission` reached its share in two of the three. Two
flooding kinds therefore took 7.5 MiB, exactly half the segment, and the other
half went to `sophia_live_session_present` (3.4 MB),
`sophia_live_session_scanout` (2.2 MB), `sophia_live_shell_content` (1.27 MB),
`sophia_shell_native_binding` (0.67 MB) and the sparse kinds.

Among those sparse kinds: **`sophia_live_session_input_routing`, 411 records in
one segment and 1,434 in the next.** That is the kind this investigation opened
on, when every one of them had been evicted before the check they were produced
for could read them. They now survive in the same segments in which 203,263 and
177,412 delivery records were refused. The cut announced itself where it
happened -- three `status=share_spent` records for delivery, two for submission
-- the counts followed at each rotation, and the health record closed the
session with `suppressed=457843`. A bounded log is not being mistaken for a
quiet one.

**At the shake's rate rather than a desktop's.** That session is ordinary
desktop use: delivery arrived at about 157 records a second, counting the
refused ones. The shake's client offers six times that -- eight records per
presented frame at 118 frames a second, 944 a second -- so the segment-level
result above does not settle the rate question on its own. Driven through the
real accounting at that rate, with the 239-byte entry measured from the same
session, the share is spent after 16,453 records, **about seventeen seconds**,
and routing records keep being written for the remainder of the segment, which
that session's own rotation puts at roughly twenty-three minutes. Pinned by
`the_share_buys_the_routing_records_a_whole_segment_at_the_reference_rate` and
`a_second_flooding_kind_does_not_take_the_routing_records_room` in
`diagnostics/capture/budget/tests.rs`.

**Judgement: the share stands, and nothing is changed.** Seventeen seconds of
the busiest kind is the price of a segment that still explains the session, and
the alternative is what this investigation opened on. Both tests were
mutation-checked before being believed: halving `NAME_SEGMENT_SHARE` moves the
measurement to 8,227 records and eight seconds, and pooling the per-name
accounting fails both. A measurement that cannot fail is not evidence.

**Row one's counters, found while doing this.** Session
`00000001790040308694-2854a567` (2026-09-21) records
`sophia_live_present_scheduler schema=2 ... paced_skips=8625
max_frame_tick_parked=1 frame_tick_overflows=0` -- paced skips in the
thousands, no overflow, and a parked depth of one against the bound of eight.
With the offscreen rate this task's row already records (113.3-113.9 FPS
against 117.99 onscreen on release `0.1.0-69ffd0900000`), the only part of that
row still unwitnessed is the window resuming its rate when dragged back.

**Not claimed.** No FPS or cadence measurement was taken here, no shake was
run, and no physical acceptance is offered: the shake is an operator gate
needing a tty3 login, `/dev/uinput`, display-manager takedown and DRM master.
To run it: `just glxgears-shake 1000 8` from a tty3 login with hands off the
mouse, then read `paced_skips` and `frame_tick_overflows` from
`sophia_live_present_scheduler schema=2`, and
`sophia_session_record_budget status=share_spent` with the health record's
`suppressed=` total beside them.

## Connections

- [Pointer motion reaches the X frontend one event at a time](c4x3drli-pointer-motion-reaches-the-x-frontend-one-event-at-a-time.md) —
  the halving this check was in service of, which this note rules evidence
  volume out of.
- [glxgears runs slow under dual-monitor pacing](lz1rbbmr-glxgears-runs-slow-under-dual-monitor-pacing.md) —
  the original report; its offscreen measurement is what produced this burst.

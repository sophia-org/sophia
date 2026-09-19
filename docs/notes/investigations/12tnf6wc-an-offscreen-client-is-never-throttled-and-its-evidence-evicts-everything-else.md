---
id: 12tnf6wc
date: 2026-09-19
kind: investigation
status: investigating
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

## Validation and remaining work

- [ ] Defer `Skipped` completions to the frame tick and prove an offscreen
      `glxgears` reports the cadence rather than ~9,000 FPS, with the owner
      loop's present handling dropping to match.
- [ ] Bound per-kind retention and prove a shake's input-routing records
      survive a subsequent offscreen burst.
- [ ] Re-run the evidence-volume check onscreen under the synthetic shake once
      the records can survive long enough to read.

Open work is tracked as t118 in `todo.md`.

## Connections

- [Pointer motion reaches the X frontend one event at a time](c4x3drli-pointer-motion-reaches-the-x-frontend-one-event-at-a-time.md) —
  the halving this check was in service of, which this note rules evidence
  volume out of.
- [glxgears runs slow under dual-monitor pacing](lz1rbbmr-glxgears-runs-slow-under-dual-monitor-pacing.md) —
  the original report; its offscreen measurement is what produced this burst.

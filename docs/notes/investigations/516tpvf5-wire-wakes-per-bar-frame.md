---
id: 516tpvf5
date: 2026-10-27
kind: investigation
status: investigating
tags: [investigation, performance, session, shell-files]
---
# Wire wakes per bar frame

## Question

desktop-cpu-04 measured about 12 owner passes per 119.88 Hz bar frame, of
which about 9.06 began with 9P wire readiness from the bar peer. Which of
those wakes does the protocol require, and which follow from implementation
choices that a session-side or client-side change could remove? This note is
a design for authority review. It contains no code, and it makes no CPU claim
until the pass-cost diagnostic (`continuous-pass-stage-cost-01`) prices a
wire pass.

## Source trace of one owned-ring frame

All references are to sources pinned by desktop-cpu-04: the session at
`d9116ff09`, the SDK at `0706ea2c`, and the `gpu-bar-dma` peer at `748c325`.
This trace is derived from source and has not been observed. The SDK keeps
exactly one `Tread` outstanding on `events` (`files/drive.rs`,
`maybe_read_events`). That is SDK behavior; no normative rule found so far
forbids more than one. Submissions on one attach are sequential, and each waits
for a covering ack of the previous `Submitted` (`files/submission.rs`,
`start_next_outbound`).

The peer runs one strictly sequential exchange per frame (`main.rs`,
`ring_frame` and `present`):

| Step | Peer to session | Session to peer |
|---|---|---|
| Ring upload | image channel offer (image worker wake) | status event, slot release |
| Demand | `transaction` write + `submit` write | `Submitted`, then `FramePermit` |
| Candidate | record + submit (+ covering ack) | `Submitted`, prepared, presented |
| Retire | record + submit (+ covering ack) | `Submitted`, `ResourceReleased` |

The session answers `Submitted` inside the wire turn that carried `submit`:

1. `submit_decoded` appends it to the journal (`shell_transport/files/export/submission.rs`).
2. Once the reply is written, `retry_waiting` answers the parked `Tread` (`sophia-9p/src/unix.rs`, `write`).

The domain reply (permit, outcome, release) is appended later in the pass,
once the owner has processed the inbound record. Its `flush` finds no waiting
read, because the client has not re-armed yet.

So every session-to-peer event that is not in the same `Rread` as an earlier
one costs a separate client re-arm, sometimes coalesced with an ack or the
next record. It is not established that each such write is one session wake:
stream writes can coalesce or fragment, and a late re-arm can be served by a
pass that is already running. With three records and about eight events per
frame, eleven writes and a saving of three wakes are conditional scenarios,
not protocol bounds. The measured 9.06 is consistent with them but does not
confirm them.

## What is required and what is chosen

Required by the current protocol:
- one record and one submit per record;
- the covering ack before the next record;
- one outstanding `Tread`, re-armed after each `Rread`.

Chosen by the implementation:
- **Session:** answering a parked `Tread` with `Submitted` in the wire turn, before the same pass has produced the domain reply.
- **Bench peer:** the strictly sequential order (demand only after the previous retire is released). Lom may differ, so changing only the bench peer would make the bench unrepresentative.

## Candidates, cheapest first

**W1, session only, no protocol change.** Defer event delivery only within a
synchronous owner phase, so that `Submitted` and a domain reply produced in
that same phase leave in one `Rread`. Delivery is never held for the semantic
reply, GPU completion, the next client ack or another wake.
- Predicted saving: one client re-arm, and so one session wake, per record whose reply is produced in the same pass. That is at most 3 of the 9 per frame (demand to permit, retire to release, and candidate to prepared if prepared is decided in that pass).
- Costs:
  - `Submitted` is delivered later, by the rest of the pass;
  - the client's next record waits for the covering ack, so it is delayed by the same amount;
  - the A2 deferred-ack rejection reasoning applies, so admission timing must be priced.
- The earlier publication-flush candidate A (`c1b5ecbb`) does not do this. It only makes publication-only visits call `flush_publications` (no image input) and leaves `Server::flush`, `service_wake`, `write`, `retry_waiting` and `Submitted` delivery unchanged.

**W2, protocol.** Retire the predecessor on presentation, by explicit client
intent in a versioned, negotiated candidate field. The field names the exact
grant and resource generation; nothing is inferred from the visible
predecessor. A presented successor raises a retire *request*, not an
immediate `ResourceReleased`. The existing retire and collect rules still
apply (no consumer leases or capture custody left, retained charges, terminal
publication) before settlement. Retirement and response capacity are reserved
when the candidate is accepted, so presentation cannot strand the obligation.
The design must define the outcome for rejected, superseded and
never-presented candidates, for other outputs still holding the predecessor,
and for retries and revocation. It would remove the Retire record and its
round trip.

**W3, protocol.** Recurring frame demand: one demand yields a permit per frame
until it is cancelled, which removes the per-frame `FrameDemand` round trip.
Today `StandingDemand` is consumed by `grant_demand`, so this is new
semantics. Requirements:
- bounded outstanding permits per output;
- unique IDs and epochs, with a TTL;
- fresh pacing and admission for every permit;
- reserved response credits;
- defined behavior for cancellation races, expiry and withdrawal;
- no idle permit or wake stream to a peer that stops consuming;
- no bypass of permits or credits.

Design W3 only after W1 and W2 have evidence.

S1 (a turn-only pass for wire-only wakes) is complementary. It makes the
remaining wakes cheaper, while W1 to W3 remove them.

## Authority review (root, 2026-10-27)

The review found W1 acceptable in principle, but only under the conditions
below; implementation is not approved. Custody is committed when
`submit_decoded` appends `Submitted`. No requirement for a distinct, immediate
`Rread` was found.

Ordering and custody:
- `Submitted` still precedes the semantic event in journal order, with its original identity, replay, covering-ack and credit semantics.
- Delivery drains unconditionally before any sleep or early continue, or there is a clean close on exit.
- A withheld `Submitted` is an explicit runnable obligation. `publication_pending` is not evidence: `submit_decoded` appends to the journal directly, and that flag is set only by `ShellFileWire::append` and publications.

Do not implement W1 by skipping `retry_waiting` in `sophia-9p`:
- that function also resumes other exports and backpressured reads, and runs revocation checks;
- event `Tread`s that arrive after `submit` in the same receive must also be covered;
- keep fresh `export.check` and the guarded `write_output` / delivery checks, including partial writes and revocation;
- keep Tflush, clunk and disconnect progress;
- role flushes already happen within a pass, so identify the exact batch boundary rather than assuming a single end-of-pass flush.

W2 is worth a separate design, ahead of W3.

## Device-free proof, after both Phase 0 arms finish

This step is proof only. Add an in-process export test that counts event
sequences, `Rread`s, bytes and client writes, and proves exact custody and
order at both the current ordering and W1's. Cases:

1. `Submitted` plus a synchronous reply batch.
2. `Submitted` alone, when the reply is asynchronous or absent: it must still drain.
3. A full journal, or a covering ack needed before more work.
4. A small read count, and output backpressure.
5. Tflush and disconnect.
6. Revocation between acceptance and deferred delivery.

If one record already yields one `Rread` carrying both events, W1 is dropped.
Real SDK scheduling and native owner wakes still need separate validation.
Phase 0's wire-pass body cost ranks this work but does not price it: that
full-pass CPU includes work that may move to the passes that remain. Only a
native ABBA plus the actual counters can decide CPU.

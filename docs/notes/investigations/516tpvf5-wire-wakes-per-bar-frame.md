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
`maybe_read_events`). Submissions on one attach are sequential, and each waits
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
one costs a separate client write: the re-arm, sometimes coalesced with an
ack or the next record. Each such write is one session wake. Three records
and about eight events per frame give an upper bound of about eleven client
writes. Coalescing in the SDK drive loop brings that close to the measured
9.06.

## What is required and what is chosen

Required by the current protocol:
- one record and one submit per record;
- the covering ack before the next record;
- one outstanding `Tread`, re-armed after each `Rread`.

Chosen by the implementation:
- **Session:** answering a parked `Tread` with `Submitted` in the wire turn, before the same pass has produced the domain reply.
- **Bench peer:** the strictly sequential order (demand only after the previous retire is released). Lom may differ, so changing only the bench peer would make the bench unrepresentative.

## Candidates, cheapest first

**W1, session only, no protocol change.** Hold parked event reads that become
ready during a component's wire turn until that pass's end-of-turn flush. Then
`Submitted` and a domain reply produced in the same pass leave in one `Rread`.
- Predicted saving: one client re-arm, and so one session wake, per record whose reply is produced in the same pass. That is at most 3 of the 9 per frame (demand to permit, retire to release, and candidate to prepared if prepared is decided in that pass).
- Costs:
  - `Submitted` is delivered later, by the rest of the pass;
  - the client's next record waits for the covering ack, so it is delayed by the same amount;
  - the A2 deferred-ack rejection reasoning applies, so admission timing must be priced.
- Must check first: whether the earlier publication-flush candidate (A, −1.5%) already changed this ordering.

**W2, protocol.** Implicit predecessor retire on presentation. A candidate
names the resource it supersedes, and the session releases it once the
successor is presented. This removes the Retire record and its round trip,
about 2 to 3 wakes per frame. It is an authority question: custody of the
displayed resource moves from an explicit client record to the presentation
outcome.

**W3, protocol.** Standing frame demand. One demand yields a permit per frame
until it is cancelled, which removes the per-frame `FrameDemand` round trip,
about 2 to 3 wakes per frame. Pacing and credit semantics change (the response
credits in `shell_content/candidates/demands.rs`), so this is the largest
review.

S1 (a turn-only pass for wire-only wakes) is complementary. It makes the
remaining wakes cheaper, while W1 to W3 remove them.

## Device-free verification before any code

Extend an in-process export test (the `shell_file_peer` sequence) to count
`Rread` replies and client writes per record, for demand to permit and retire
to release, at the current ordering. If one record already yields one `Rread`
carrying both events, W1 saves nothing and is dropped. Use the pass-cost
package's wire-bucket body cost per pass to turn a wake count into CPU. No
candidate is built until both exist and the authority review accepts it.

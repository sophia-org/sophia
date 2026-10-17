---
id: s5msyyv2
date: 2026-10-17
kind: adr
status: accepted
tags: [adr, shell, 9p, performance]
---
# Pipeline shell writes with bounded recovery and unchanged custody

## Context

The reusable-writer slice removes transaction setup requests. Its native cost
pilot is prepared but has not run. The remaining serial waits include the last
record write before submit and each canonical pixel chunk. Request counts do
not establish how many Session passes those dependencies cost.

The [prior-art survey](../investigations/yhsocuvc-what-prior-art-on-9p-transport-cost-says-about-the-shell-wire.md)
motivates bounded streaming. ZX uses request/reply streams and addresses latency;
it does not establish that a four-write window will improve Sophia's local wire.
Sophia would keep ordinary tagged 9P requests and its own custody boundaries.

## Decision

Implement two separately tested changes after the first slice's native capture:

- B2: queue submit behind the final record fragment on the same connection,
  after earlier fragments have successful replies. Retain both tags until their
  outcomes are reconciled; never infer no custody from the record result alone.
- B3: permit up to four outstanding pixel writes from adjacent FIFO units for
  the same live resource and writer fid. Refill credits as replies arrive, while
  retiring results and settling tickets in issue order. Stop at every record or
  resource boundary. End and the writer's closing clunk wait for all successful
  write replies; cancellation remains a separate explicit path.

Rust gathers adjacent queued upload units. C pipelines fragments of its current
borrowed upload buffer; its caller retains that buffer until upload completion.
Neither change requires a new caller API or releases storage before replies
are settled. C client storage changes its layout; rebuild consumers and foreign
bindings with matching headers and libraries. This is not ABI compatibility.

Four is an SDK sender ceiling documented by the shell profile, not a new server
pending-request guarantee or negotiated ContentLimits field. Smaller existing
tag, output, byte and work budgets lower the usable window, down to one. Keep
space for events, ACKs, cancellation and recovery. Do not increase transport
limits or add a capability bit, file node, opcode or record layout.

The [implementation plan](../plans/nfe0blnb-queue-shell-submit-and-pixel-writes-within-existing-custody-bounds.md)
defines the checks and adoption sequence. The [shell contract](../../sophia-shell-files.md#queued-shell-writes)
owns recovery, deadlines and compatibility. Implementation and native evidence
remain pending.

## Alternatives

Retaining serialization is the control and remains valid for bounded clients.
Larger chunks change memory reservations and request counts; independent-resource
concurrency changes scheduling scope. Both remain outside this proposal.
Server-side dependency tags or remembered failure chains would add protocol
state. Here later offsets are refused by existing cursor checks, with fencing
errors taking precedence; there is no new server-side cancellation group.
Holding completed chunks despite EAGAIN would change backpressure semantics and
is excluded. ACK cadence and pending-read service need separate evidence.

## Consequences

The normal path should issue the same requests with fewer dependent waits.
Error recovery can issue more requests. Extra outstanding payloads and tags must
stay within existing budgets; the fixed ring must not add per-frame heap churn.
Four outstanding requests do not imply four times the service budget or exactly
one quarter as many owner passes. Fairness, CPU/MiB, allocations/frame and latency
must be measured on both wires. The +10% ceiling and full matrix remain unchanged.

Canonical chunk success renews the existing idle deadline; queueing, partial
progress and refusal do not. Transfer and transaction assembly deadlines remain
fixed. No owner lock or new resource reservation spans the stream. An old writer
never recovers into a successor binding.

## Acceptance and connections

Proposed October 17, 2026 following niltempus's request for a consultation and
implementation plan. Accepted later that day when niltempus directed
"Implement the plan" after reviewing the capture-first sequence with B2 and B3.
The prepared first-slice capture still precedes production implementation.
This extends the [reusable-writer decision](7mls2e2q-reuse-a-shell-transaction-writer-without-changing-custody.md)
without superseding its custody rules. The [current contract](../../sophia-shell-files.md)
and [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md)
remain authoritative. No performance result is claimed.

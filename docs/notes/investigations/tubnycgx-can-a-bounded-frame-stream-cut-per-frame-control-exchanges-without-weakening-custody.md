---
id: tubnycgx
date: 2026-10-28
kind: investigation
status: investigating
tags: [investigation, 9p, performance, architecture]
---
# Can a bounded frame stream cut per-frame control exchanges without weakening custody

## Current contract and ledger gate, November 2

W2's iGPU effect comparison passed and Mason accepted its measured tradeoff
for the next experimental baseline. W3 keeps W2 enabled in both arms. The
qualified workload is the same trusted, serial, reason-1 bar producer; two
credits do not create concurrency in that producer. The concrete target is
its 2120 demand/permit pairs per run, replaced by one stream-open pair if
no expiry or reopen occurs. No wake-count or CPU saving follows from that
logical count alone.

The decisions here supersede conflicting wording in the earlier design below:

1. **No new rate gate in this experiment.** Runtime advertises
   `max_candidate_rate_millihz` but does not enforce a temporal rate limit.
   Outstanding-credit bounds are structural, not temporal: fast terminal
   rejections can cycle at the same instant. Both arms retain the same
   trusted producer cadence. There is no rate-enforcement claim.
2. **Open reserves two responses:** its reply and one lifetime close notice.
   Begin reserves Prepared plus terminal capacity before spending credit or
   removing pending work. With the serial producer, one lifetime close plus
   one candidate's two responses uses three records, as today's permit path
   does. The close notice has a reserved control record; publication may still
   wait on separate journal capacity and is retried, never lost or duplicated.
3. **Supersession needs an available credit.** With no credit, Begin returns
   Budget even if pending work could be superseded; this differs from the
   permit path and is outside the serial workload's exercised behavior.
   With credit, preflight all response capacity, admit the successor, cancel
   the pending predecessor and its W2 intent through the existing path, then
   commit its Superseded terminal/replenishment before another admission.
   Failed admission must leave the predecessor intact. In the existing store's
   accounting, subtract the replaced pending candidate's two reservations and
   require `2 + replacing` records (Prepared, terminal, and any Superseded
   event), not the old `3 + replacing` which also included FramePermit.
4. **Budget is not revocation.** A valid stream without credit or response
   capacity returns Budget, consumes candidate identity, consumes no credit,
   cancels no W2 intent and renews no deadline. Wrong stream/tuple is Stale.
5. **Modes are exclusive per output.** Open must refuse a standing demand,
   permit or assembly, and a second open. Demand/cancel on an open stream is
   Stale. Explicit close prevents admission; prior terminal outcomes and the
   reserved close notice drain before reopen. Reason-3 withdrawal is outside
   this experiment; roles needing it keep the demand path.
6. **Server commit anchors credit expiry.** Prepared returns no credit.
   Terminal publication returns at most one, with the existing permit TTL
   starting at server commit, never receipt or ACK. Spend oldest first.
   The existing expiry sweep closes on an expired available credit and emits
   one reserved close notice; no new timer or idle wake is introduced.
   With all credits outstanding there is no available-credit expiry: the
   existing candidate/assembly timeout and lifecycle paths still apply.

Bind the complete connection/content grant, output id/generation, explicit
allocation id/generation, output **facts_generation**, and stream id. Facts
changes include mode/scale/formats, not only topology. Unlike FrameDemand's
optional allocation, StreamOpen must name the real allocation from the
candidate's surfaces. The server computes one or two credits from its limits
and ring slots; a client request is only a ceiling. IDs cannot replay across
reopen or a replacement connection.

The first implementation is `sophia-runtime::frame_credits`, compiled only
with the default-off `experimental-shell-standing-credit` feature. It is a
reusable pacing ledger with reservation and publication callbacks. It has no
Session/transport caller and defines no wire record or capability. Focused
tests cover binding, exhaustion, spent refusal identities, callback failures,
oldest-first expiry, close/drain/reopen, duplicate terminal refusal, clock
regression, overflow, and the absence of a temporal rate gate.

The next implementation connects the ledger to `ContentCandidateStore`, under
the same default-off feature. Open/close notices and ordinary outcomes share
one FIFO; either typed reader stops at the other kind of record. The close
obligation counts against the real control-record budget from open onward.
Candidate refusal reporting uses the queue-length change, so a stream notice
at the head cannot hide an outcome queued behind it.

Stream terminals require `presented_at` / `renderer_failed_at` with the
owner's current commit time. Untimed terminal calls refuse a stream-owned
candidate without removing it. Prepared never replenishes credit; store clock
regression closes the ledger, and accepted work can still settle without new
credit. Revocation discards stream ownership and undelivered notices, retaining
the existing untimed backend-drop cleanup for the dead connection.

The store tests exercise budgets 3/4/5/6, failed and successful supersession,
W2 intent cancellation and last-renderer-lease release, tuple invalidation,
candidate timeout, FIFO ordering, replay refusal, clock faults and overflow.
These are local control-record and lifecycle proofs, **not a journal delivery
or wire interoperability proof**. Until codecs exist, no transport caller can
open a stream; feature-on legacy traffic creates no stream FIFO elements.
The eventual wire drain must service both record kinds in order during the
same pass, including a Presented outcome whose retirement intent waits behind
a stream notice.

One `max_open_candidates_total` ceiling covers streams and legacy
permits/assemblies: a stream holds one slot until its close and outcomes drain,
and its own assembly uses that slot. A Begin while that output is assembling,
or after stream closure/expiry, is Stale before spending candidate identity.
A valid Open-stream Budget refusal still spends identity without spending
credit. Closing again after a Timeout notice is Stale. A clock-poisoned stream
store stays failed until revocation; accepted terminals may drain without
credit, but no future admission can revive the poisoned authority.

**Wire blocker discovered by the adapter review:** an unreported candidate
error currently propagates to Session recovery, stopping the shell. There is
no synthetic candidate refusal and no candidate equivalent of the resource
store's `additional_response_credit`. Before exposing any stream opener, add
a Begin-aware reservation for a nonfatal refusal **before consuming input**,
publish the appropriate refusal, and discard the rejected candidate's trailing
parts. Exhausted credit and a Begin racing close/expiry must not kill the shell.
A blanket extra slot for every part would deadlock chunks under a fully
reserved budget; use a Begin-aware peek/admission path. Test this at capacity,
with no-credit Begin, trailing chunks, and close/expiry races. Store tests do
not discharge this gate.

**Next gate is the wire contract and codec integration, not native timing.**
Allocate the exact grammar in the protocol ledger before codec work; r13/bit18
remains proposed until that decision. C/Rust parity and the old-protocol refusal
matrix remain required before production adoption. No general rollout or
native run is authorized by these store tests.

## Question and earlier design

**November 2 disposition:** niltempus promoted this mechanism to a bounded
experiment after W2 integration under the [revised critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md#current-resumption-close-the-matched-cpu-gap).
The design below is a retained proposal, not a completed source contract.
Resolve rate enforcement, response-reservation timing, supersession/W2
cancellation, exhausted-credit semantics, cancellation/withdrawal and expiry
notification before wiring. References below to an existing enforced rate gate
must be reconciled with the source finding that an advertised limit alone
provides no such gate. Protocol allocation and C/Rust parity remain mandatory;
this note assigns no public identifier.

Can a bounded standing frame stream, designed now rather than after
every small candidate, cut per-frame control exchanges while keeping
immutable generations, exact grant/generation settlement, bounded
credit, revocation, and release-after-last-lease? This note is a
decision-ready bounded prototype design and its falsifier. It is not
a production change, a device experiment, or a CPU saving claim.

It sits under [the continuous-frame CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
and beside [the shell 9P allocation plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md).
It complements W2 (retire the predecessor on presentation, Claude's
lane, now in implementation) and does not duplicate it. W1's delayed
event delivery remains shelved; nothing here reopens
`delayedSubmitted`. Source identifiers below are as of this note's
commit on master; pinned measurement baselines are named where used.

## Evidence and its limits

Measured, retained under `~/.local/state/t331/` on crunch:

- `desktop-cpu-04`: Sophia Session 835–855 µs/frame and peer
  465–485 µs/frame, combined 1300–1340, for the concrete owned-ring
  path at 119.88 Hz. Not an architectural lower bound.
- `continuous-pass-stage-cost-01`: about 9.05 wire-started owner
  passes per completed interval, ~455 µs of whole-pass body per
  interval. Pass bodies include every service those passes ran;
  they are not removable overhead and not a savings estimate.
- `continuous-ack-coalesce-02`: combined difference
  [−62.5, +22.5] µs/frame, midpoint −1.52%. Inconclusive: it proves
  neither cheap framing nor per-record causality.
- `continuous-release-overlap-01`: inconclusive; no syscall counter
  proved fewer waits.

Withdrawn claims, kept visible: there is no measured irreducible
Session floor (flat self-time shows no hotspot, not irreducibility);
the 10–20 µs copy figure originated with Claude as a memcpy-speed
estimate, Muse repeated it, both withdrew it as a measurement. The
only isolated copy-adjacent receipt is peer-side `ring_upload` at
about 17 µs/frame in the pass-cost diagnostic; it does not price the
Session destination copy, whose cost is not separated in current
evidence. Multiple records per turn do not by themselves prove only
protocol redesign can help; record kinds, ordering, and per-turn
work matter. Low authority-check counts do not prove negligible cost
without measured per-check CPU.

Current per-frame control sequence for the continuous files panel,
simplified from the shell-files and owned-ring contracts:

1. `FrameDemand` (client to Session), then `FramePermit` event
   (grant with 250 ms TTL and role-checked rate); a candidate cannot
   submit without a current permit.
2. Owned-ring `Offer` on the control file, then `ResourceStatus`,
   then the exact `SlotReleased`; the producer may not touch the
   slot before that release.
3. `Candidate` (one complete record), then `CandidateOutcome`.
4. Explicit `ResourceRetire` for the predecessor, then
   `ResourceReleased` only after the last consumer of the immutable
   destination.

Session-side, each wake tends to run a whole files service
(`ShellFileWire::service`): export poll, 9P server turn, a second
fresh export poll, expiry, and image service. The final fresh check
before image service is a deliberate custody guarantee, not waste by
assertion.

## Finding and resolution

The design below changes only mechanism 1 and the delivery shape of
3. Mechanisms 2 (image settlement) and 4 (retirement) keep their
records, custody, and ordering exactly; W2 may later replace 4 on
its own evidence. Custody copy and authority checks are not touched
at all (authority amortization is a separate sketch at the end).

### Minimum counterfactual: standing credit with replenishment on outcome

One change: replace per-frame `FrameDemand` → `FramePermit` with a
bounded standing permit, opened once per (connection epoch, content
grant epoch, allocation, output, topology generation) tuple.

- Session grants `Credit = min(ring slots, max in-flight candidates
  for the role)` (2 for the current two-slot workload) at stream
  open. Opening is an ordinary submitted record answered by one
  event; it reserves nothing beyond the existing credit it stands
  in for.
- A candidate submitted against a standing credit consumes one
  credit atomically at admission. The exact replenishing transition
  is: **exactly one credit returns when that candidate reaches a
  terminal `CandidateOutcome`** — presented (the exact candidate
  retired on its output), rejected, or superseded. A `Prepared`
  outcome is non-terminal and replenishes **nothing**; the credit
  stays spent while that candidate is still live. The replenishment
  rides on the terminal outcome record as an additive, versioned
  field (see wire compatibility); it is not a new event.
- Rate and expiry are per-credit, not stream-global, and both run
  on the server's existing permit clock. **Initial credits become
  spendable at the stream-open/grant instant** — the moment the
  server commits the open — not at any later client observation.
  First-candidate anchoring is rejected: it would leave permission
  unexpired while idle, unlike an issued permit. A **replenished
  credit becomes spendable at the server's terminal-outcome
  credit-commit point** for that candidate (the server-side instant
  the terminal outcome and its replenishment are committed together),
  never at client receipt, acknowledgement, or wake. Every credit
  expires 250 ms after it becomes spendable, under the same bounds
  as a `FramePermit` today.
- Admission still enforces the role's **existing configured/
  negotiated rate gate** per candidate admission, independently of
  credits held. That gate keeps its own value per role/profile; it
  is not a hardcoded universal 120 Hz. Consequence: a fast
  reject/supersede cycle returns credits but cannot admit
  successors faster than today's permit-and-rate path admits them.
- Spend order is **oldest-expiring credit first**. With per-credit
  expiry anchored above, this serial workload cannot strand an
  initial credit behind replenished ones. An expired credit is
  dead: later traffic, late outcomes, or wakes never revive it.
- **Expiry closes the stream (recommended, adopted for the
  prototype).** The first credit expiry observed at an admission
  boundary closes the stream: no further candidate is admitted
  against it, candidates already admitted settle exactly as today,
  and the producer must explicitly re-open to present again. This
  is the simplest fail-closed rule and avoids a second state
  (partially-expired window) whose interactions would each need
  their own proof. Silently dropping single credits and continuing
  is not part of this design.
- Journal and response capacity are rechecked per generation,
  exactly as today. Terminal outcomes, statuses, and releases still
  consume journal sequence and held-status reservations; stream
  open reserves no journal capacity in advance, and admission still
  refuses `EAGAIN` under journal, queue, slot, or credit pressure.
  The window (`Credit = min(slots, max in-flight candidates)`)
  therefore bounds *pacing* only; it does not by itself prove that
  each generation's existing response/journal obligations fit —
  those existing per-generation checks are what prove it, and they
  are unchanged.

Everything else is deliberately frozen for the prototype: offers,
status-before-release, slot reuse, candidate contents, explicit
retire, adoption, and all budgets.

### Invariants (each must be device-free testable)

State and identity:

- Credits are bound to the exact open tuple. Any change of grant,
  allocation, output, or topology generation expires the stream;
  candidates against an expired tuple are refused, never retargeted.
- Credit arithmetic is checked and bounded: `0 <= outstanding <=
  Credit`, consumption and replenishment each exactly once per
  candidate identity. A duplicate, replayed, or foreign
  `CandidateOutcome` replenishes nothing.
- Candidate identity, ordering of outcomes per candidate, and the
  final-status-before-`SlotReleased` ordering are unchanged.

Lease and custody:

- Immutability is per generation, not per stream: an accepted
  immutable destination still cannot be altered by slot reuse, and
  `ResourceReleased` still waits for the last consumer.
- Stream credit is pacing authority only. It is not admission:
  every candidate and offer still passes the same admission,
  budget, custody, and fresh authority checks at the same
  boundaries as today.

Revocation and failure order:

- Revocation or partner exit expires the stream before any further
  candidate admission, using the same fresh checks that guard
  admission today; already-admitted generations settle exactly once
  with their existing terminal reasons, in the existing order.
- A malformed or refused candidate does not consume a successor's
  credit; a refused stream-open leaves no credit and no stream
  state. Journal loss or disconnect ends the stream with the old
  connection; no credit replays into a successor connection.
- Backpressure (`EAGAIN` for queue, slot, credit, or journal
  pressure) still spends the offer/candidate identity exactly as
  today. Standing credit never converts a refusal into a retry with
  a renewed deadline.

### Wire compatibility and negotiation

The additive replenishment field on `CandidateOutcome` is a wire
change and must be negotiated, never assumed:

- It rides the existing shell-file revision machinery: a new
  revision selects the standing-permit records; older revisions
  keep `FrameDemand`/`FramePermit` exactly. Mixed peers refuse at
  negotiation rather than degrading silently.
- Stream-open is refused (existing `Refused` record) when the role,
  revision, or tuple does not admit it, or when a second stream is
  opened on the same tuple. Opening a stream does not change which
  records are submitted versus events.
- Prototype gating follows the W2 pattern: off-by-default build
  feature plus explicit owner-local enablement plus a paired peer
  key, with mismatch refused before launch. That is a policy-wiring
  decision for review, not part of this design's core.

### Stream state machine (written, complete for the prototype)

States per (connection epoch, content grant epoch, allocation,
output, topology generation) tuple. All instants are on the server's
existing permit clock.

```
Closed
  |  stream-open submitted, tuple + revision admitted,
  |  journal/response capacity for the open reply reserved as today
  v
Open(credits[Credit], outstanding set)
  |  candidate admitted: stream Open, tuple current, fresh authority
  |  checks pass, role rate gate passes, journal/response capacity
  |  reserved for this generation as today, oldest-expiring
  |  unexpired credit exists -> consume it, candidate joins
  |  outstanding; state stays Open
  |  terminal outcome for an outstanding candidate commits:
  |  candidate leaves outstanding; if stream still Open, append
  |  one credit spendable from this commit instant (Prepared:
  |  no transition, no credit)
  |  expiry observed at an admission boundary, tuple change,
  |  revocation, partner exit, or journal loss:
  v
Closing (admissions refused; outstanding candidates settle exactly
  once with today's terminal reasons and ordering; replenishments
  for them are committed to a closed stream and discarded)
  |  outstanding empty and all settlements published
  v
Closed (explicit re-open is the only way back; no credit, lease, or
  identity carries into a successor stream or connection)
```

Refused stream-open is `Closed -> Closed` with the existing refusal
record and no state. A second open on the same tuple while `Open`
or `Closing` is refused. Admission capacity is never taken from the
window size alone: `min(slots, max in-flight candidates)` bounds
pacing credits only, while every generation still reserves its
response/journal obligations through the same per-generation checks
that exist today, and `EAGAIN` under that pressure spends the
identity exactly as today. The state machine grants no permission
to reserve unbounded responses at open or admission.

### Predicted logical counts (arithmetic, not a saving)

For the frozen 2120-frame, two-slot continuous workload, counting
logical submitted records and events per side (not syscalls, not
Twrites, not measured CPU):

Only one prediction is made, and it is conditional on no credit
expiry and no re-open during the run: the design removes 2120
`FrameDemand` submissions and the 2120 `FramePermit` events they
produce, replaced by one stream-open submitted record and one
reply. Every expiry-driven close ends that stream's credit
window; a run with `k` additional explicit re-opens still removes
2120 demands and 2120 permits but adds `k + 1` open pairs in total.
For this programme's frozen 2120-frame continuous workload
(nominal 120 Hz, actual 119.88 Hz), where replenished credits would
be spent ~8.3 ms after commit, `k = 0` is the prediction to be
falsified, not an assumption to bake into results and not a proven
property. Nothing else is
counted. In particular, `CandidateOutcome` counts are **not**
predicted: a candidate can produce a non-terminal `Prepared`
outcome before its terminal one, so outcome records per run are at
least the candidate count and depend on the path taken; `Candidate`
submissions are workload-given (2120), not a design prediction.
`FramePermit` is a server-produced event, not a record consumed by
an owner turn, so "records removed" splits by direction: 2120 fewer
demands decoded by Session, 2120 fewer permits produced by Session.

No owner-turn or CPU effect is inferred from those counts.
Coalesced records still cost CPU, and removing records can change
subsequent wake scheduling in either direction; neither "turns fall
by the removed share" nor "nothing changes" follows from counting.
Only measured per-class work (below) and then a bounded prototype
can price the change.

### First counter, and what can actually falsify this

An earlier draft proposed a "sole content of a turn" share with a
~90% build/don't-build line. That rule is withdrawn: it treated a
descriptive count as a causal test. Records consumed per pass, by
class — demands decoded, permits produced, candidates, outcomes by
kind (Prepared versus terminal), statuses, releases — remain worth
counting on the unmodified baseline, but only as description.
Readiness class is not causality, and records-per-pass alone
decides nothing: a demand drained inside another turn still costs
decode and permit production, and removing it can shift wake
scheduling for everything after it.

The November 2 sequence replaces the per-class attribution prerequisite:

1. Resolve the contract gaps and prove the bounded state machine with the
   failure cases below. State which logical demand/permit exchanges disappear;
   do not assume each exchange was a separate owner wake.
2. Implement an isolated negotiated prototype with its old-protocol control,
   retaining custody, response reservations, exact outcomes and cleanup.
   End-of-run logical counts prove that the intended path ran; no per-frame
   diagnostic clocks are needed in the performance build.
3. Compare uninstrumented combined Session + producer CPU on the matched
   workload under the owning plan's accounting and ABBA rules. Correctness
   failure, regression or unresolved CPU benefit rejects promotion. Confirm
   promising results before adoption. Dependency traces or external sampling
   may resolve a named decision; they are not a mandatory attribution project.
   No post-hoc threshold relaxation or repeat-until-pass.

### Authority amortization: separate sketch only

Sharing one custody observation across a bounded turn (the existing
default-off `experimental-custody-dispatch` scope) is a separate
design, not part of this prototype. It is admissible here only
because its shape already preserves the contract: one observation
at turn start, bounded check count and age, fail-closed on any
error except `EINTR` (which grants no new credit), and the final
fresh poll before image service retained. No cached-positive result
and no wider observation window is authorized or assumed. It needs
its own counts and CPU measurement first (plan section 4); call
frequency alone never justifies it.

## Validation and remaining work

Device-free proof matrix for the prototype, before any package:

| Case | Required outcome |
| --- | --- |
| Credit exhaustion at window edge | Candidate waits/refused with existing backpressure; no over-issue |
| `Prepared` outcome for a credited candidate | No replenishment; credit stays spent until that candidate's terminal outcome |
| Fast reject/supersede cycling | Successor admissions bounded by exactly today's rate gate; credits return but cannot be spent early |
| Credit held past its 250 ms spendable window | Expiry closes the stream at the admission boundary; later traffic/outcomes cannot revive it; explicit re-open required |
| Initial credits idle from stream-open | Same 250 ms expiry from the grant instant (no first-candidate anchoring); stream closes identically |
| Replenishment observed late by the client | Spendable instant is the server's credit-commit point, unaffected by client receipt/ACK/wake timing |
| Two credits held, one expiring sooner | Oldest-expiring credit is spent first; the later credit is never consumed out of order |
| Replenishment replay / duplicate outcome | Second replenishment refused; credit unchanged |
| Outcome for foreign grant or stale generation | Settles nothing, replenishes nothing |
| Topology or allocation change mid-stream | Stream expired; later candidates refused until re-open |
| Revocation between admission and outcome | Admitted generations settle exactly once, terminal order intact; no new admissions |
| Partner exit with credits outstanding | Stream ends with the connection; no credit or lease leaks into a successor |
| Malformed candidate inside a healthy stream | That candidate refused; stream credit accounting unchanged for successors |
| Refused stream-open (role, revision, second open) | No credit, no stream state, existing refusal record |
| Inactivity past stream expiry | Candidates refused until re-open; no deadline renewal by wakes |
| Last-lease held past stream close | Destination charged until last consumer; release still exact |
| Old-revision peer against new build (and inverse) | Negotiation refuses stream records; per-frame demand path unaffected |

Each case needs a mutation that fails with the guard removed
(credit check, tuple binding, exactly-once replenishment), in the
style of the existing W2 lifecycle receipts. Native work, if ever
authorized, follows the plan's stop gates unchanged: frozen
hypothesis and counter, isolated ABBA, no bundling with W2 or S1.

## Connections

- [Reduce continuous-frame CPU without weakening custody](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md):
  the owning plan, its contracts, and its measurement gates.
- [Reduce shell 9P allocation overhead](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md):
  baseline and preservation constraints for the same wire.
- [Independent shell upload concurrency](gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md):
  which FIFO barriers are real dependencies; this design crosses
  none of them.
- [Shell files contract](../../sophia-shell-files.md) and
  [owned ring contract](../../sophia-owned-ring.md): the records,
  settlement ordering, and custody this design must not weaken.

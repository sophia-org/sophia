---
id: tubnycgx
date: 2026-10-28
kind: investigation
status: investigating
tags: [investigation, 9p, performance, architecture]
---
# Can a bounded frame stream cut per-frame control exchanges without weakening custody

## Question

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
- Rate and expiry are per-credit, not stream-global. Each credit
  carries the instant it became spendable. A returned credit is
  spendable under the same bounds a `FramePermit` is today: it
  expires 250 ms after it becomes spendable, and candidate
  admission still enforces the role's existing rate bound
  (120 Hz candidate rate in the prototype limits) per admission,
  independently of how many credits are held. Consequence: a fast
  reject/supersede cycle returns credits but cannot admit
  successors faster than today's permit-and-rate path admits them,
  and an idle producer's credits lapse exactly as unused permits
  lapse today (re-open or re-grant, no wake renews any clock).
  Whether per-credit expiry of *unspent initial* credits must use
  the stream-open instant or the first-candidate instant is left as
  the one named open decision below; everything else above is
  specified to preserve today's bounds rather than approximate them.
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

### Predicted logical counts (arithmetic, not a saving)

For the frozen 2120-frame, two-slot continuous workload, counting
logical submitted records and events per side (not syscalls, not
Twrites, not measured CPU):

Only one prediction is made: the design removes 2120 `FrameDemand`
submissions and the 2120 `FramePermit` events they produce, replaced
by one stream-open submitted record and one reply. Nothing else is
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

What can falsify the prototype, in order:

1. Per-class work on the baseline: attribute demand-decode,
   permit-production, and outcome-encoding CPU (or stage time) per
   completed interval, with the thread and window named for every
   clock. If that attributed share is below noise for the harness,
   the design has nothing to remove and should not be prototyped
   for CPU reasons.
2. Dependency traces: show which turns a demand actually gated
   (candidate could not be admitted before it, nothing else pending)
   versus turns where admission was already available. This
   distinguishes scheduling serialization from flat per-record cost,
   which the A1 result ([−62.5, +22.5] µs, inconclusive) could not.
3. Only then a bounded prototype, whose pre-registered prediction
   is stated in turns per interval and attributed per-class CPU —
   not in total CPU alone — and is falsified if those counters do
   not move as predicted while joins, settlements, and cleanup
   receipts stay identical. Total-CPU comparison follows the plan's
   existing threshold and ABBA rules, never before the mechanism
   counters move.

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
| Credit held past its 250 ms spendable window | Credit lapsed; candidate refused until re-grant/re-open; no wake renewed it |
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

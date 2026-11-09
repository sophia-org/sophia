---
id: g5madpss
date: 2026-11-08
kind: adr
status: proposed
tags: [adr, custody, performance, session, shell]
---
# Defer the custody observation questions M1 and M2 and keep caller-fresh checks per pass

## Context

The owner thread observes launch custody with a zero-time poll of the shell
component's two kernel identities before each guarded action (obligation B6
in the per-pass map). liv-01 counted 27 such polls per frame on the lean lane.
The [custody obligation map](../investigations/px8oi1bw-which-custody-polls-protect-an-obligation-that-channel-liveness-cannot.md)
mapped each site to the Session action it guards and found that readiness
wakes the owner but does not replace a fresh observation, because a component
can exit between any two observations. The accepted authority-review request
(`sophia-custody-authority-review-request-draft-claude-02`, acceptance
`sophia-custody-authority-request-acceptance-01`) put two questions to the
operator:

- M1: can a later image-path check replace the post-turn custody observation
  on a proven-eligible entry while preserving authentication close, admission
  withdrawal, whole-wire refusal and publication behaviour?
- M2: can the owner wait supply a current, identity-bound, single-use
  authentication observation for the next entry, with fresh-poll fallback for
  a missing, stale, replaced, invalid, errored or consumed observation?

What the evidence establishes: the per-frame frequency of the polls, and a
separate tight-loop measurement of about 0.4 µs per two-pidfd zero-timeout
poll including its harness (`sophia-custody-poll-cost-01` with its two
corrections). What it does not establish: the attributable owner cost of
those polls on the lane, or a removable saving. Multiplying the count by the
microbenchmark figure is illustrative only; it is not a measured owner cost
and not a valid share of the instrumented dcp-01 pass total. Neither question
would remove all 27 polls: M1 addresses eligible post-turn checks, M2
pre-turn checks for which a usable observation exists.

The earlier custody-dispatch screen (c40, `experimental-custody-dispatch`,
default off; c41 screen 2026-10-20) added one aggregate poll and an
observation allocation per scope and then allowed bounded in-memory checks.
It showed no consistent measured benefit and was not promoted. M2 proposes
consuming an existing wait observation without those additions, so c41 is
caution for M2, not a rejection of it, and its uncaptured scope-coverage
discriminator is a prerequisite only for reopening c40.

## Decision

Defer M1 and M2. Preserve the existing custody checks and refusal behaviour
at every site. Current evidence establishes frequency and a separate
microbenchmark cost, but neither native attributable cost nor removable
saving. Additional eligibility or observation machinery is not justified
presently. Deferral establishes neither unsafety nor equivalence to
previously screened mechanisms.

For any pass consolidation, including the T1 shape in the
[four-wake note](../investigations/edc0te2v-which-action-each-of-the-four-lean-owner-wakes-owes-and-which-transition-a-consolidation-would-remove.md),
B6 remains a per-pass obligation: skipping a loop body does not authorize
dropping a fresh check before a guarded action.

Reopen when new evidence names a mechanism-specific opportunity: attributable
cost of the custody polls on the current lane, useful eligibility or
observation coverage for a named set of sites, or a selected architectural
change that already revisits these boundaries. The share of owner CPU is a
prioritization input, not an acceptance threshold, and this decision commissions
no capture to find it.

## Alternatives

- Select M1 for a device-free interleaving and refusal proof now. Rejected
  for now: Ready and Hello are states to investigate, not sufficient
  eligibility predicates; image-only Closing is not equivalent to the wire
  path today; whole-wire refusal, the early return before expiry, publication
  behaviour and an exit or withdrawal between the turn and the later check all
  have to stay correct, and nothing measured says the post-turn poll is worth
  that proof.
- Select M2 for the same proof. Rejected for now: it adds a second liveness
  path whose fallback must carry every case the fast path misses, and the
  wait's return is not a demonstrated atomic observation instant.
- Reject M2 as already screened by c41. Not taken: c40 and M2 differ in what
  they add, and the accepted request distinguishes them.
- Remove a poll site outright where no owner action lies between it and the
  previous observation. Not taken: the obligation map found no such site, and
  the before and after turn pair brackets the turn that consumes input.

## Consequences

Track A of the [CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
has no pending authority decision. The per-pass custody polls stay as they
are and remain an input to any T1 design. M3, readiness-driven reaping, stays
parked for the lifecycle map in
[rug93yha](../investigations/rug93yha-which-per-pass-obligations-can-move-to-their-own-trigger-without-weakening-custody.md).
No source change, build, native run or CPU claim follows from this record.

## Acceptance and connections

Proposed 2026-11-08 after a read-only consult with Codex, which recommended
this disposition and supplied the decision wording; niltempus agreed to the
disposition the same day. Acceptance is recorded by Codex in the plan and
`todo.md` t344. Supporting records: `sophia-custody-poll-cost-01` and its
corrections, the authority-review request draft 02 and its acceptance,
px8oi1bw, rug93yha, liv-01 and dcp-01 within their stated limits.

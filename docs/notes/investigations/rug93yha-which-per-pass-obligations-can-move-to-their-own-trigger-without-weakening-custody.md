---
id: rug93yha
date: 2026-11-08
kind: investigation
status: investigating
tags: [investigation, session, shell, custody, performance]
---
# Which per-pass obligations can move to their own trigger without weakening custody

## Question

The lean lane makes four owner passes per completed frame, one per wake source
(wire, other fd, native completion, render-result ring), and every pass walks
every component and every per-pass obligation whether or not that pass's wake
had anything to do with it. B1 (3u93ti2w) listed the obligations that are
serviced per pass by construction. Track A then showed that the largest of
them, the custody poll, is an authority question before it is a cost question,
and its decision is pending (M1 and M2 in 4rifczhw; M3 parked here). This
note asks, for each remaining per-pass obligation: which event source could
own it, which deadline bounds it if that event is missed, and what the pass
would still have to do unconditionally. It is a design input for niltempus's
selection, reviewed by Codex. It changes no source, selects no mechanism,
launches nothing and makes no CPU claim.

## Method

Source read on master `d92d4c498`. Counts from svc-01, svc-02 and liv-01
(sealed under `~/.local/state`). The pre-lean shell-work-cost figures are
labelled as such. The readiness concept is the standard: a component is ready
for a named obligation, never in the abstract, and a deadline's existence does
not by itself prove every call on its path can be gated on it.

## What S1 and the component-walk lane already settled

S1, the turn-only pass (`cpu-turn-proof`), regressed Session by 25 to 65
us/frame and its negative finding stands: skipping role closures is not
side-effect free, because the closures carry `poll_io` flushes,
`drain_file_output` journals and settlement, `export.poll`, `expire` and
image service. B3 then subscribed component descriptors in the Service wait,
and B4 reproduced the observed and blind turn counts on this lane with passes
per frame unchanged at four. Its accepted limit stands: `turns_observed`
counts a supplied `Some` before server validity, so zero blind turns is not
proof that blind probe syscalls were removed; missing or stale observations
retain the ordinary probe, and no observation does not mean no owed work. So
the remaining structure is: four passes per frame, each visiting everything.
This note does not propose skipping visits. It proposes moving obligations to
the event that makes them due, so that a visit on a foreign wake may have less
to do, and it says what each move would have to keep. That reduction is a
hypothesis; neither lower CPU nor a cheaper pass is established here.

## Table A: obligations, candidate triggers, bounds, and what stays per pass

| B1 id | obligation today | where | candidate trigger | bound if the trigger is missed | must stay per pass |
|---|---|---|---|---|---|
| B6 | custody poll per entry (27/frame) | `files.rs service`, `launch_authentication/files.rs poll/check` | pidfd readiness already in both wait sets, under the pending M1/M2 authority decision | the requested 20 ms idle cap plus scheduling delay; not a detection guarantee | every retained boundary keeps its `active`/ingress read and a fresh poll where the contract requires one. Not designed here. |
| B7 | process reap: `try_wait` over every slot from three sites per pass | `shell_component_processes.rs:196-227`, `process.rs:486-507`, callers `component_service.rs:32`, `component_lifecycle.rs:193`, `physical_input_loop.rs:397` | the wrapper pidfd, held by the supervisor only on demand (`process.rs:360-379`), registered as an owned lifecycle descriptor rather than through the wire | the termination deadline (2 s, `process.rs:511-515`) and the per-pass walk as fallback until coverage is proven for every state in Table B | the three call sites collapse to one readiness-driven visit only after Table B is closed; `has_live_launch`'s own `try_wait` before admission stays |
| B8 | settlement of revoked grants and catalog reconcile, twice per pass each | `component_session.rs:579-591 settle_revocations`, `component_catalog.rs:198-230 reconcile_connections`, called at `component_service.rs:121,153,305,346` | settlement: the ledger's own retained state (`revoked_content_grants.rs:33-55` keeps an entry until its exact cleanup succeeds; `settle_with` with no runtime returns the retained count without draining; `component_session.rs:351` blocks a new start while the ledger is nonempty). Reconcile: every mutation of the connected inventory and of publications or queued opens, which are independent sites (`component_catalog.rs:198-230`) | settlement has none and needs none if the pending state is the ledger itself, which must stay retained across visits without a runtime and be serviced when one returns; reconcile has none and is unpriced | the original four sites and their order, error and partial-success behaviour; the reconcile's whole-inventory validation (`component_catalog.rs:196-197`); no duplicate flag before the existing ledger state is shown insufficient. Unpriced: see the amendment below |
| B9a | topology publication retry | `topology_phase.rs:589-593` | the candidate that blocks publication clearing, which "clears on its own schedule" | the pass itself, as now, until the clearing event is named | one retained slot; a newer snapshot supersedes |
| B9b | WM failure cancellation every turn | `wm_phase.rs:80-86` | none: the comment states the cancellation request must persist on every turn until restoration | the turn | stays per pass by design |
| B9c | chords dispatched every turn | `lifecycle.rs:204-212` | already input-driven and clock-driven ("Held falls due by the clock"); the per-turn dispatch drains a FIFO | the chord clock | the drain of an empty FIFO is cheap and keeps the overdue-deadline guarantee; leave |
| B10 | every connected wire turns once per visit so subscribed readiness is consumed | `component_session.rs:496-499` | the observation itself, since B3; a visit without an observation on that wire has nothing to consume | the pre-turn poll and expire still run on the entry | this is M2's territory for custody and otherwise already event-shaped |
| B11 | a skipped component pass must not carry a wait forward | `physical_input_loop.rs:591` | none; it is the rule that makes any gating safe | n/a | stays |
| B12 | pre-wait `service_native` unconditional; parked candidates serviced when nothing is pending | `native_service.rs:7-11, 29, 130-132` | native fds and the ring already end the wait; the pre-wait call is the candidate (c) from B1, overlapping the inconclusive collection guard 75ff298ab | the Service wait's short cap | feedback delivery before an authority-only batch skips the tail |
| B1-B5, B13 | present clocks, ack deadlines, image accept retry, hello timeout, capture maintenance, authentication wait | as in 3u93ti2w Table B | already have their own deadline or trigger | own | unchanged |

## Table B: lifecycle map for readiness-driven reaping (M3, parked by Track A)

Track A found that the current wire registration does not cover a child in
every state. Readiness-only reaping needs an independently owned registration
and a deadline path per state. The states, from the code:

| state | how it is entered | what polls the child today | wire registration present? | what a readiness-driven design must own |
|---|---|---|---|---|
| pre-spawn | `auth.pending` set, worker thread not yet running the spawn, or `cancelled` set before it does (`spawn.rs:73-77`) | nothing: no child | no | nothing to reap; cancellation is a flag the worker reads |
| worker-owned live child | the `sophia-role-spawn` thread has run `start_with_launch_channel`; the supervisor and its child live on that thread until `SpawnResult` is sent (`spawn.rs:64-112`); `SpawnLease` keeps the thread alive because bubblewrap's parent-death signal follows it (`spawn.rs:29-41`); it is lifetime ownership, not polling | the worker: on `cancelled` it calls `request_termination`; nobody on the owner thread | no | the worker stays the owner until handoff; an owned descriptor cannot be registered by the owner before the result arrives |
| returned, not installed | `SpawnResult` in the channel; `pending.poll` has not yet taken it (`startup.rs`) | nothing until the owner polls; a dropped receiver drops supervisor and child, `retire_process` otherwise (`spawn.rs:96-105`, `startup.rs:54-72`) | no | the retire path stays; readiness has no subscriber yet |
| installed, negotiating | `started.supervisor` inserted into the slot, `begin_authenticated_negotiation` with a 5 s window (`authentication.rs:131-147`) | `visit` → `process.poll` → `try_wait` | yes, phase `Negotiating` (`shell_component_connections.rs:597-604`), but authentication fds appear only once `AuthenticatedFiles` is installed | this is the earliest point an owner-side descriptor could be registered; today `launch_custody` opens a wrapper pidfd on demand into `LaunchCustody` (`process.rs:360-379`) and the supervisor retains none; the 5 s negotiation deadline stays |
| connected | phase `Connected` | same | yes, including custody fds while `active` and ingress hold | same descriptor; exit readiness closes the connection as `visit` does today |
| failed negotiation | `begin_authenticated_negotiation` error → `stopping`, `revoke`, `request_termination` (`authentication.rs:148-153`) | `poll_termination` under the 2 s deadline, which starts only at `request_termination` (`process.rs:511-515`), never for a merely live child | no: `request_stop`/close precede termination | the termination deadline and process-group kill (`process.rs:552-590`) stay; readiness may end the wait early but cannot replace the deadline |
| revoked or disconnected | `launches.phase != Active` → `stopping` (`authentication.rs:102-109`); `request_stop` closes the connection first (`shell_component_processes.rs:178-190`) | `poll_termination` | no: the connection is closed before the process ends, and `visit_wait_fds` stops registering once `active` or ingress is withdrawn | same as above |
| stopping, awaiting exit | `termination_deadline` set | `poll_termination` each pass | no | the owned descriptor plus the deadline |
| exited, unreaped | child exit before any poll, in any of the states above | next `visit` reaps and closes the connection, or `poll_termination` if termination was requested | only if the connection was still `Negotiating`/`Connected` with `active` and ingress held; a revoked or stopping child has already lost its wire registration and stays relevant | readiness on an owned descriptor that outlives the wire registration; reap and `connections.close(key)` move to that event only for states where the descriptor exists |
| replacement | a new attempt reuses the slot after retirement (`two_protected_processes_retain_independent_stop_and_replacement_custody`) | the new supervisor | new registration | the old descriptor must be dropped with the old supervisor so a stale readiness cannot retire the new child; `replacement_cannot_reuse_the_previous_dispatch…` is the shape of that test |

Holding a pidfd does not establish progress: a child that never exits is
bounded only by the termination deadline, which must stay a clock and which
begins only when termination is requested. Ownership today is split between
the spawn worker, the startup handoff and the slot's supervisor, and no
retained owner-side readiness descriptor exists. The smallest design that
respects this table keeps every current owner until transfer, keeps the
per-pass walk, and adds readiness only as an early wake. Removing the walk or
collapsing its call sites would need device-free proof per state above, with
interleavings, deadlines and registration errors; receipts from a capture
corroborate only the states that capture exercised and cannot stand in for
that proof.

## Which move is worth designing first

Ranked by how much per-pass work it removes against how much contract it
touches, with the pre-lean counts as the only sizes on record:

- B8 settlement and reconcile serviced from their own state rather than per
  pass. Unranked by size: no measurement of these four calls exists (see the
  amendment). As a source hypothesis it is the one whose trigger is Session's
  own bookkeeping: a revocation is recorded by Session (`revoked.record` at
  `component_session.rs:348,375,407` and in `finish_poll`), so the ledger's
  retained entries already say whether settlement is owed. Whether that state
  is sufficient on its own, and whether the reconcile's independent
  publication and inventory mutations can be gated at all, is the design
  question. "No authority touched" is a condition to establish for the chosen
  sites, not a conclusion from same-thread execution. Proof shape, if
  selected: device-free tests for a revocation recorded while no runtime is
  present and drained when one returns; same-pass ordering between the four
  sites; unchanged inventory versus publication-only change versus invalid
  inventory versus replacement for the reconcile; a negative mutation that
  shows a stranded entry or a stale publication.
- B7 reap on an owned descriptor with the walk retained, per Table B. Touches
  lifecycle ownership, which Table B shows is not a single owner today.
  Dropping the walk or collapsing its three call sites needs device-free
  state, interleaving, deadline and registration-error proof; a capture in
  which the walk found nothing readiness missed corroborates only the states
  it exercised and authorizes nothing. M3 stays unselected.
- B12 the pre-wait `service_native`, candidate (c) from B1, only with a new
  hypothesis distinct from the inconclusive collection guard.
- B6 waits for the authority decision; nothing here designs it.

## Amendment: the settlement counter was for a different operation

The first version of this note ranked B8 first on "121 settlement calls per
completion, 99 percent empty" from continuous-shell-work-cost-01, and the
accepted component-walk note 3u93ti2w carries the same figure in its Table A
(line 106) and Table C (line 122) against `settle_revocations` and
`reconcile`. Codex's review of this note checked the historical host
`5e6db9f99`: `Stage::Settle` is in
`crates/sophia-runtime/src/shell_content/isb_jobs.rs:222` and wraps the ISB
image-job reconcile, not `RevokedContentGrantLedger::settle_with` or
`ComponentCatalog::reconcile_connections`, and its denominator is published
permits, not completed frames. The attribution in both notes is therefore
wrong. This note withdraws it; the 3u93ti2w rows are corrected by this
amendment trail and should be amended in that note by a separate docs
change. No measurement of the four B8 calls exists, pre-lean or current, and
none is requested here.

What this does not reach: the four passes per frame themselves. Each wake
source still produces a full pass because the Timeout tail serves native work
and the next pass serves everything else. Collapsing passes is a reactor
change that B1 and 4rifczhw have declined to authorize without a hypothesis;
moving obligations to triggers is what makes each pass cheaper, and it is
what this note proposes.

## Limits

- No size exists for the B8 calls at all; the figure the first version used
  belonged to the ISB image-job reconcile (see the amendment). Any count would
  be a new package decision, not this note's.
- Table B is read from source; the tests named are shapes to reuse, not proof
  that a future design passes them.
- The owner thread's cost of any of these calls is unmeasured; this note
  ranks by removed calls and touched contract, not by microseconds.
- Nothing here changes the pending M1/M2 decision or revives c40/c41.

## Connections

- [Which component-walk visits do useful work per ready source](3u93ti2w-which-component-walk-visits-do-useful-work-per-ready-source.md)
  supplies Table B's obligations and the candidate prices.
- [Which custody polls protect an obligation that channel liveness cannot](px8oi1bw-which-custody-polls-protect-an-obligation-that-channel-liveness-cannot.md)
  is Track A; its request and the pending decision are recorded in
  [the plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md).
- [Readiness must name an obligation](../concepts/readiness-readiness-must-name-an-obligation.md)
  is the standard applied in Table A.
- [Which Redox ring and Orbital patterns can reduce Sophia frame overhead safely](sy7lhvb8-which-redox-ring-and-orbital-patterns-can-reduce-sophia-frame-overhead-safely.md)
  supplies the pattern that obligations hang off event sources; this note is
  that pattern applied to Sophia's own per-pass list, without the ring.

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
image service. B3 then removed the blind wire probes by subscribing component
descriptors in the Service wait, and B4 showed blind turns 1800 → 0 with
passes per frame unchanged at four. So the remaining structure is: four passes
per frame, each visiting everything, with the visits now observed rather than
blind. This note does not propose skipping visits. It proposes moving
obligations to the event that makes them due, so that a visit on a foreign
wake has less to do, and it says what each move would have to keep.

## Table A: obligations, candidate triggers, bounds, and what stays per pass

| B1 id | obligation today | where | candidate trigger | bound if the trigger is missed | must stay per pass |
|---|---|---|---|---|---|
| B6 | custody poll per entry (27/frame) | `files.rs service`, `launch_authentication/files.rs poll/check` | pidfd readiness already in both wait sets, under the pending M1/M2 authority decision | the requested 20 ms idle cap plus scheduling delay; not a detection guarantee | every retained boundary keeps its `active`/ingress read and a fresh poll where the contract requires one. Not designed here. |
| B7 | process reap: `try_wait` over every slot from three sites per pass | `shell_component_processes.rs:196-227`, `process.rs:486-507`, callers `component_service.rs:32`, `component_lifecycle.rs:193`, `physical_input_loop.rs:397` | the wrapper pidfd, held by the supervisor only on demand (`process.rs:360-379`), registered as an owned lifecycle descriptor rather than through the wire | the termination deadline (2 s, `process.rs:511-515`) and the per-pass walk as fallback until coverage is proven for every state in Table B | the three call sites collapse to one readiness-driven visit only after Table B is closed; `has_live_launch`'s own `try_wait` before admission stays |
| B8 | settlement of revoked grants and catalog reconcile, twice per pass each | `component_session.rs:579-591 settle_revocations`, `component_catalog.rs:198-230 reconcile_connections`, called at `component_service.rs:121,153,305,346` | the events that record a revocation (`component_session.rs:348,375,407 revoked.record`) and change the connected inventory (negotiation completion, close) | none needed if the trigger is the recording itself; today `settle_with` on an empty set is the 99 percent-empty call of shell-work-cost (pre-lean, 121 calls per completion) | the reconcile validates the complete inventory, not one peer's view (`component_catalog.rs:196-197`); any trigger must still present the whole inventory |
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
| spawning | `auth.pending` set; `pending.poll` each visit (`authentication.rs:128-160`) | the spawn lease, not the supervisor | no: no process yet | the spawn deadline stays with the lease; nothing to reap |
| started, negotiating | `started.supervisor` inserted, `begin_authenticated_negotiation` with a 5 s window (`authentication.rs:131-147`) | `visit` → `process.poll` → `try_wait` | yes, phase `Negotiating` (`shell_component_connections.rs:597-604`), but the authentication fds appear only once `AuthenticatedFiles` is installed | a wrapper pidfd registered at insert time, independent of the wire; the 5 s negotiation deadline retained |
| connected | phase `Connected` | same | yes, including custody fds while `active` and ingress hold | same descriptor; exit readiness closes the connection as `visit` does today |
| failed negotiation | `begin_authenticated_negotiation` error → `stopping`, `revoke`, `request_termination` (`authentication.rs:148-153`) | `poll_termination` under the 2 s deadline | no: `request_stop`/close precede termination | the termination deadline and process-group kill (`process.rs:552-590`) stay; readiness may end the wait early but cannot replace the deadline |
| revoked or disconnected | `launches.phase != Active` → `stopping` (`authentication.rs:102-109`); `request_stop` closes the connection first (`shell_component_processes.rs:178-190`) | `poll_termination` | no: the connection is closed before the process ends, and `visit_wait_fds` stops registering once `active` or ingress is withdrawn | same as above |
| stopping, awaiting exit | `termination_deadline` set | `poll_termination` each pass | no | the owned descriptor plus the deadline |
| exited, unreaped | child exit before any poll | next `visit` reaps and closes the connection | yes until closed | readiness on the owned descriptor; reap and `connections.close(key)` move to that event |
| replacement | a new attempt reuses the slot after retirement (`two_protected_processes_retain_independent_stop_and_replacement_custody`) | the new supervisor | new registration | the old descriptor must be dropped with the old supervisor so a stale readiness cannot retire the new child; `replacement_cannot_reuse_the_previous_dispatch…` is the shape of that test |

Holding a pidfd does not establish progress: a child that never exits is
bounded only by the termination deadline, which must stay a clock. The
smallest design that respects this table keeps the per-pass walk as the
fallback and adds readiness as an early wake, measured first by whether the
walk still finds anything the readiness did not. That is a receipt question,
not a CPU question.

## Which move is worth designing first

Ranked by how much per-pass work it removes against how much contract it
touches, with the pre-lean counts as the only sizes on record:

1. B8 settlement and reconcile on their recording events. The call is made
   four times per pass and was 99 percent empty pre-lean (121 calls per
   completion, shell-work-cost). The trigger is in the same process and the
   same thread: a revocation is recorded by Session itself, so a "pending
   settlement" flag set at `revoked.record` and at inventory changes, checked
   where the four calls are made today, removes the empty calls without moving
   any authority. The reconcile's whole-inventory rule is kept because the call
   still runs from the same site, just not when nothing changed. No custody
   contract is touched. Proof: a device-free test that records a revocation
   and asserts settlement happens on the next pass, and a negative mutation
   that drops the flag and shows a stranded revocation.
2. B7 reap on an owned descriptor with the walk retained as fallback, per
   Table B. Touches lifecycle, not custody authority; the receipt that
   justifies dropping the walk is "the walk found nothing readiness missed"
   over a capture. Three call sites collapse only after that.
3. B12 the pre-wait `service_native`, candidate (c) from B1, only with a new
   hypothesis distinct from the inconclusive collection guard.
4. B6 waits for the authority decision; nothing here designs it.

What this does not reach: the four passes per frame themselves. Each wake
source still produces a full pass because the Timeout tail serves native work
and the next pass serves everything else. Collapsing passes is a reactor
change that B1 and 4rifczhw have declined to authorize without a hypothesis;
moving obligations to triggers is what makes each pass cheaper, and it is
what this note proposes.

## Limits

- Sizes are pre-lean and inclusive of instrumentation; no post-lean count of
  empty settlement calls exists. A counts-only receipt like liv-01 for B8
  would be the first measurement, and it is a package decision, not this note's.
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

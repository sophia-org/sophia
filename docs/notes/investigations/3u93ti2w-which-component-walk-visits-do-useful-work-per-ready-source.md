---
id: 3u93ti2w
date: 2026-11-08
kind: investigation
status: investigating
tags: [investigation, session, shell, performance, 9p]
---
# Which component-walk visits do useful work per ready source

## Question

The lean lane runs about four owner passes per presented frame, one per
wake source: a shell wire became readable, the image-worker descriptor became
readable, a native completion arrived, and the Service wait that collects the
render result. Every pass walks every shell component and every shell service
phase. This note asks, per ready source, which visits in that walk have work
to do, which obligations inside the walk must be serviced on every pass
regardless of readiness, and what the pass after a Service wait does that the
pass after a Receive wait does not. It prices the idle visits from receipts
that already exist. It selects no candidate and authorizes no native run.

## Method

Source read on master 3cd21338d. Receipts reused read-only, none re-run:

- svc-01 (2026-11-08, current master lane with A2 and A3, receipt-only, no
  CPU figures): `~/.local/state/t331/svc-01/changing`, sealed result
  `~/.local/state/sophia-t344-native-receipts-01`. Counters below are deltas
  across the reducer's interior window of 1800 completions per arm.
- cpu-syscall-census-01 (2026-11-02, pre-lean, about 11 owner passes per
  frame, audit `~/.local/state/sophia-syscall-census-01-audit-claude-01/AUDIT.md`).
- continuous-shell-work-cost-01 (pre-lean, pre-A3, diagnostic host
  5e6db9f99, thread CPU timers on; never a CPU comparison). Raw owner
  `sophia_shell_work_cost` rows reduced by
  `~/.local/state/sophia-component-walk-reduction-01/reduce.py` (SHA256SUMS
  there; the amendment `sophia-component-walk-reduction-02` relabels its
  denominator as permits with the same arithmetic, and Codex's review
  `sophia-component-walk-review-codex-01` keeps an input snapshot). Deltas between owner snapshots 1 and 12 cover 1625 server frame
  permits in sophia1 and 1624 in sophia2. The emitter's denominator is
  `server_frame_permits`, one per frame permit the server issued, not
  presented completions; "per permit" below means that.
- cpu-decomp-03 (2026-11-07, pre-A3, instrumented host d43c4feb0, about 34
  us/frame of instrumentation; audit
  `~/.local/state/cpu-decomp-03-postrun-audit-claude-01/REPORT.md`).
- cpu-collect-01 (2026-11-07, uninstrumented four-arm ABBA) for the
  baseline-versus-baseline spread.

Every number carries its source and whether it is pre-lean, pre-A3 or current.

## Pass anatomy

The owner loop body is `owner_loop/physical_input_loop.rs`. The wait sits
inside `authority.rs`, so one pass is the pre-wait phases, the wait and a
post-wait tail. Every wake returns `Timeout` to the waiting pass; the
Timeout arm runs `runtime.service_native` at once, and descriptors are served
on the next pass. No component visit is gated by readiness. Readiness only
steers how much each wire's 9P service reads: `turn_connected` hands the
wire the observation the wait recorded, and `files.rs service_turn` does
`turn_observed` when there is one and a blind `turn(Some(ZERO))` when there
is none. The Service wait subscribes relay descriptors, native descriptors and
the owner ring but no component descriptor (`authority_service_wait.rs`), so
the pass after every Service wait probes every wire blind.

svc-01 confirms the shape on the current lane (interior window, 1800
completions):

| counter delta | A2 arm r60b1 | A3 arm r60c1 |
|---|---|---|
| owner_passes | 7214 | 7219 |
| owner_waits | 7214 | 7219 |
| component_waits (waits that subscribed components) | 5414 | 5419 |
| component_ready_any | 3600 | 3600 |
| component_ready_wire / image_worker | 1800 / 1800 | 1800 / 1800 |
| owner_fd_ready | 3600 | 3600 |
| native_service_waits | 1800 | 1800 |
| owner_ring_ready | 2 | 1802 |

Per frame: four passes, three waits subscribing components, two of them
ready (one wire, one image worker), one Service wait with no component
subscription. A3 moves the Service wait's end from the timer to the ring and
changes nothing else in this table.

## Table A: visits per pass

Cost column: cpu-decomp-03 phase cost per pass, pre-A3, labelled by the wait
that ended the pass (so the "native" row is the pass after a Service wait,
whose component visit ran with readiness None, and the "Service" row is the
pass after the image-worker wake). Owner total 416 us/frame = components
161.2 + native 113.4 + other 91.4 + authority 50.5. Components per pass:
native-ended 27.0, wire-ended 45.3, fd-ended 28.0, Service-ended 60.9 us.

| visit | where | readiness that justifies work | behaviour on a foreign wake | obligation (Table B) | idle syscalls | branch |
|---|---|---|---|---|---|---|
| present_clocks.service (twice) | `physical_input_loop.rs:107-117` | own clock deadline | reads clock, nothing due | B1 | none | |
| wm.service_inspection, publish_output_status | `physical_input_loop.rs` | WM mailbox | empty | B9 | none | |
| developer_launches.service | `physical_input_loop.rs` | launch state change | empty | | none | |
| resource sampler | `resource_sampling.rs` | 5 s period | compares clock | B1 | none | |
| broker.poll | `metadata_broker.rs:99` | broker process or X route | `supervisor.poll` try_wait, `driver.poll` | B7 | wait4 per pass | |
| service_presented_policy | `physical_input_loop.rs` | policy change | empty | | none | |
| legacy metadata_shell phases | `physical_input_loop.rs:392-579` | single-shell facade | absent on the lane (`run.rs:413` needs `config.shell_process`) | | none | R2 b5eab0749 targets this facade |
| poll_authenticated / process reap | `supervisor/process.rs:496-510` | child exit | `try_wait` per supervised process | B7 | wait4 per process | |
| turn_connected: 9P turn | `shell_component_connections.rs:689-760`, `files.rs:199-250` | wire readable (observation) | post-Receive: `turn_observed`, reads only slots with revents; post-Service: blind `turn(Some(ZERO))` | B10 | turn with readiness None: one ppoll | `cpu/decomp-fix` cab686fdb counts blind vs observed |
| turn_connected: custody checks | `files.rs:208-230` via `launch_authentication/files.rs:409` and `custody.rs:41` to `launch_custody.rs:57-73` | none; caller-fresh liveness by design | `export.poll` before and after the turn, one per file check, one image check | B6 | 4.95 pidfd ppolls per `service()` entry, an aggregate average over turns and flushes (shell-work-cost boundary rows) | none; cached authority is an authority change |
| turn_connected: image channel | `files/image.rs:85-175` | listener readable, image peer readable | accept when `listener_idle` is None and outside the 10 ms backoff; `check_image_peer` custody; `receive_image_datagram` | B3, B4, B6 | accept4 EAGAIN (post-Service), recvmsg EAGAIN | |
| images.service | `metadata_shell/component_session.rs` | image worker fd | `image_alive` custody poll per retained owner | B6 | one pidfd ppoll | |
| settle_revocations, reconcile (twice each) | `metadata_shell/component_session.rs` | revocation or publication | 99.25 percent of settlement calls empty (shell-work-cost) | B8 | none | |
| bar role services | `component_service.rs:247-280`, `panel_service.rs:78-160` | content change, action, presentation | all five run; `with_service` only invokes its closure, and the role services call `poll_io`, whose `turn_files` flushes only when a publication is pending (`shell_transport.rs:586-593`); `turn_connected`'s input turn can flush a publication too | B6, B10 | pidfd ppolls per flush entry, count not measured per flush | |
| dock and launcher roles | `component_service.rs:175-240` | catalog or launch change | `catalog.publish` then `poll_io` | B6 | as bar | |
| catalog.service_execution, discard_readiness | `physical_input_loop.rs:592-594` | none | drops unconsumed readiness, uncounted | B11 | none | |
| input drain, session controls, topology, lifecycle | `lifecycle.rs`, `topology_phase.rs:589-591` | input, control, topology change | chords dispatched, topology retried, capture maintenance | B2, B5, B9 | none | |
| pre-wait service_native | `lifecycle/native_service.rs:29` | native completion or parked candidate | after a native-ended pass, re-checks what the Timeout tail just consumed | B12 | none | collection guard 75ff298ab; collect-render 2a45d8ea3 |
| wm_phase, native_retirement.poll | `wm_phase.rs:84-85` | WM result, retirement | cancellation check | B9 | none | |
| wait selection and wait | `authority.rs`, `authority_receive.rs:55-82`, `authority_service_wait.rs` | the subscribed sources | Service wait subscribes no component fd | | one ppoll | A2 1285d1609, A3 cc92b86bb |
| Timeout tail service_native | `authority.rs:878` | native completion, render result (ring) | runs on every wake; on a wire or fd wake there is no native readiness to consume | B12 | none | |

Shell-work-cost (pre-lean, pre-A3, sophia1; sophia2 within 2.4 percent on
every row, source_release 16.5 against 16.9 us being the widest):

| stage | calls per permit | CPU per call | CPU per permit |
|---|---|---|---|
| custody | 94.8 | 1.13 us | 106.7 us |
| settle | 133.7 (99.25 percent empty) | 0.58 us | 77.0 us |
| collect | 42.8 | 0.64 us | 27.6 us |
| image_alive | 19.3 | 1.06 us | 20.3 us |
| admission | 26.5 | 0.71 us | 18.7 us |
| source_release | 26.5 | 0.64 us | 16.9 us |

These are instrumented historical costs. Per-call CPU includes the two clock
reads of the instrumentation itself, and the instrumentation can perturb
scheduling and caches, so they are not a bound on an uninstrumented run.

The boundary rows split the 199448 custody checks of the whole sophia1 run
by call site: 40327 before the turn, 40327 after it, 78539 inside file checks
(1.95 per entry), 40247 inside the image check, 8 elsewhere. So a
`service()` entry averaged 4.95 custody polls over turns and flushes
together; the per-flush count was not measured separately, and the file
checks are not charged to input against flush. Over the same snapshot deltas,
entries per permit were 13.27 turns (21560 over 1625 permits, 24.6 percent
with no decoded record) and 6.0 flushes (9750); the boundary rows are
whole-run cumulatives and are not combined with these as one attribution.
The flushes come from `turn_files`, which flushes only when a publication
is pending (`shell_transport.rs:586-593`), from the role services' `poll_io`
and from the input turn alike. The emitter's `empty` means no record
was decoded in the entry (`shell_work_cost/state.rs:207`); a flush decodes
nothing by construction, so `flush_empty` says nothing about whether the
flush published, and a turn with no decoded request may still have written
replies, retried or done custody work. No cost is attributed to the flushes
here.

## Table B: obligations with their own deadline or trigger

An obligation with its own deadline or trigger can be serviced when that
deadline or trigger fires. One without is serviced per pass by construction,
and any change that skips a visit must keep it. The existence of a deadline
is not by itself proof that every call on the path can be gated on it; that
is shown per call site when a change is proposed.

| id | obligation | where stated | own deadline or trigger |
|---|---|---|---|
| B1 | present clocks before any early continue | `physical_input_loop.rs:107-117` | yes: clock deadline |
| B2 | deadlines and acknowledgments before any seat wait; ack deadline judged after input | `physical_input_loop.rs:619-620`, `shell_transport.rs:580-582` | yes: `ACK_PROGRESS_TIMEOUT` |
| B3 | one image accept per owner turn drains unauthorized joins; retry bounded | `image.rs:92-105` | yes: listener readable, `accept_not_before` |
| B4 | image hello timeout | `image.rs:112-118` | yes: `hello_timeout` |
| B5 | capture withdrawal and maintenance progress without a frame | `lifecycle.rs:93-103` | yes: maintenance period |
| B6 | custody check and image service keep caller-fresh liveness | `files.rs:224-230`, `custody.rs:41-47` | no: per entry; pidfds exist (`launch_custody.rs:62-63`) but are not the trigger |
| B7 | process reap, including paused owners | `metadata_shell/component_session.rs:320, 549`, `process.rs:496` | no: `try_wait` per pass; pidfd readiness would be one |
| B8 | settlement and revocation reconcile | `metadata_shell/component_session.rs` | no: per pass |
| B9 | topology publication retry, WM cancellation, chords every turn | `topology_phase.rs:589-591`, `wm_phase.rs:84-85`, `lifecycle.rs:206-210` | no: per pass |
| B10 | every connected wire turns once per visit so subscribed readiness is consumed | `metadata_shell/component_session.rs:496-499` | partly: the Receive wait's observation; none after a Service wait |
| B11 | a skipped component pass must not carry a wait forward | `physical_input_loop.rs:591` | no: per pass |
| B12 | parked candidates serviced when nothing is pending; feedback before an authority-only batch skips the tail | `native_service.rs:7-11, 130-132` | partly: native fds and ring; the pre-wait call is unconditional |
| B13 | authentication wait | `owner_wake/attribution.rs` reason `shell_authentication` | yes: its own cap |

## Table C: syscalls per idle component visit, one bar wire

Unit costs marked measured come from shell-work-cost (pre-lean, inclusive of
instrumentation). Unit costs marked assumption are 0.5 to 1.5 us per
zero-timeout ppoll and are not measurements.

| syscall | when | count | unit cost | evidence |
|---|---|---|---|---|
| custody pidfd ppoll | every `service()` entry: before turn, after turn, per file check, image check | 4.95 per entry averaged over turns and flushes; 94.8 per permit pre-lean | 1.13 us measured | shell-work-cost stage custody and boundary rows. The census counted 115.6 owner ppolls per completion in a different run; the two runs are not subtracted from each other |
| 9P blind probe ppoll | input turn with readiness None: after a Service wait, or a stale observation; the only term the first candidate changes | 1 per wire per such turn | assumption | `files.rs:247` `turn(Some(ZERO))`; not separable in the census |
| accept4 on the image listener | `service()` with `listener_idle` None and outside the backoff: a turn after a Service wait, or any flush (`files.rs:205-209` clears it for flushes) | 6.0 per completion pre-lean, all failed | assumption | census accept4; errno not recorded by the census (AUDIT.md:67-69), EAGAIN plausible from source; `image.rs:96-105` |
| recvmsg on the image peer | every image `service()` with state Ready | 15.1 per completion pre-lean, all failed | assumption | census recvmsg; errno not recorded; `image.rs:158` |
| wait4 per supervised process | every pass | 30.2 per completion pre-lean, about 2.7 per pass | assumption | census wait4; `process.rs:496`, `metadata_broker.rs:100` |
| eventfd and journal reads | wake clearing | read 51.3 per completion pre-lean, about half failed, errno not recorded | assumption | census; R1 9923b899d, R3 ed888c011 territory |

Projection to the current lane, stated as an assumption and not a
measurement: at four passes per frame and the same 1.7 `service()` entries
per pass, custody polls would be 34 per frame, about 38 us at the
instrumented unit cost. The A2 and A3 arms carried no shell-work-cost receipt, so this has
not been observed post-lean.

## Findings

Per ready source, the useful work is one visit. On a wire wake it is the
observed 9P turn and the bar `service_content` that follows. On an
image-worker wake it is `owner.service` inside `turn_connected`. On a native
completion and on the render-result ring it is the Timeout tail's
`service_native` in the same pass. The other visits run four times per frame;
the aggregate receipts here show their counts and costs, not that any
particular visit was empty, and the custody, publication and deadline
obligations in them remain useful on a foreign wake.

The pass after a Service wait differs from the pass after a Receive wait in
one way only: its component visit has no observation, so every wire is probed
blind and the image listener is accepted on. That costs one ppoll and at most
one accept4 per wire per frame. It does not change the custody polls, the
settlement calls or the role visits, which are per pass in both cases. Accept
attempts also happen on every flush that carries a publication, because a
flush clears `listener_idle` (`files.rs:205-209`); subscribing components in
the Service wait would remove the turn-side attempts only.

The largest component-walk term is the custody poll. cpu-collect-01 put its
two master arms at 661.3 to 661.6 and 673.2 to 673.6 us/frame on the host
alone, so two identical arms differed by about 12 us/frame inside one
package. That is observed between-arm variability, not the package's
resolution and not a proof of what can or cannot be measured; it says only
that a candidate priced below it would not be told from that variability by
one four-arm run. Every in-lane candidate below is priced under it. Custody
caching is an authority change and out of this lane.

Candidate prices, all assumptions until a receipt measures them:

| candidate | mechanism | syscalls removed per frame | price |
|---|---|---|---|
| subscribe component fds in the Service wait | `authority_service_wait.rs` adds `components.subscribe_wait`; next pass gets an observation | 1 blind ppoll, up to 1 turn-side accept4 per wire; flush-side accepts stay | 1 to 4 us |
| process reap on pidfd readiness or a retry deadline | `process.rs:496`, `launch_custody.rs:62-63` | about 11 wait4 | 3 to 8 us |
| skip the pre-wait `service_native` after a native-ended pass | `native_service.rs:29` | none; saves one empty walk of the candidate list | 6 to 9 us |
| custody caching (out of lane) | authority review | about 34 pidfd ppolls | about 38 us |

## Limits

- Shell-work-cost and census figures are pre-lean at about 11 passes per
  frame; decomp-03 is pre-A3. None of them is a CPU claim and none was
  compared with an uninstrumented arm.
- The svc-01 window is the reducer's interior window; arm totals differ
  slightly from the ratios shown in this note and are in the sealed result.
- The lane is one bar wire with a shared owned ring; a second wire or a
  separate image channel multiplies the per-wire rows of Table C.
- Per-call CPU in shell-work-cost includes its own clock reads, and its
  denominator is server frame permits, not presented completions.
- The census records failed syscalls without errno (AUDIT.md:67-69); every
  EAGAIN reading in this note is inferred from source, not observed.
- The blind-versus-observed turn split has never been read on master post-A3;
  `wire_turn_count()` counts both alike. Only `cpu/decomp-fix` cab686fdb
  separates them (`cpu/decomp-fix2` is d43c4feb0, the decomp-03 host).
- Table A's branch column names prior lanes for reuse of their shape. A2 and
  A3 are on master; R1, R2, R3, the decomp and pass-cost receipts,
  collect-render and the collection guard are unmerged and unadopted.
- Shared-ring readiness is not a per-sender causal trace; the svc-01
  counters support the wait and subscription map, not per-visit usefulness.
- Candidate prices are a prioritization under uncertainty, not a
  measurement result; candidate and CPU-run selection stay with the joint
  decision after this evidence is accepted.

## Next discriminator

If a candidate from the table is chosen, its upper price is below the 12
us/frame observed between master arms, so a CPU arm could not resolve it and
none is launched.
The mechanism is proved device-free with a negative mutation and captured in
a two-arm receipt-only package like svc-01, with the counters that must move
named in advance (for the first candidate: `component_waits` equals
`owner_waits`, blind turns per frame zero, accept attempts on turn visits
zero while flush-side attempts are reported separately, passes per frame
unchanged at four). If Table C's post-lean projection is
wanted as a measurement rather than an assumption, the shell-work-cost
receipt rebased onto master is a separate diagnostic package, never a CPU
arm. The decision, including whether to open an authority review for custody
caching, is recorded by Codex in
[4rifczhw](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md) and todo t344.

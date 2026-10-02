---
id: qvrk2298
date: 2026-10-01
kind: investigation
status: awaiting-physical-acceptance
tags: [investigation, session, x11]
---
# Remove timer polling from idle desktop workers

## Question

Can idle workers sleep until work arrives without delaying input, presentation,
or cancellation? niltempus selected idle wakeups before thread consolidation.

## Evidence

The running accepted source is 21bdf9f60 (master dd62b50c8 has the same code).
There were 84 threads, including 35 for seven X11 clients. Quiet intervals on an
empty workspace used 6.3–7.3% of one core. A visible-terminal sample used about
22%, including rendering. Voluntary context switches are not exact timer wakes.
Diagnostic record budgets mean missing log records do not prove absent activity.

Implementation evidence: `~/.local/state/sophia/development-evidence/t276-idle-wakeups-01/`.

## Finding and resolution

The baseline selects a 1 ms receive budget whenever physical input exists. Libinput
polls at 1 ms, libseat at 2 ms, the routed frontend at 1 ms, and each X11 writer
at 10 ms. An empty output spill uses a 50 ms condition-variable timeout.

### Notification and readiness waits

The implementation adds `sophia-wake`: an eventfd notification with weak
producer handles, bounded channel adapters, and polling against one absolute
deadline. Producers publish first, then notify. The consumer clears the
notification before inspecting queues. Disconnect is published before its wake;
stop uses an independent flag and wake before joining. Attaching a consumer
also rings once for work that predates attachment. Notifications confer no
authority and contain no work.

- Libinput drains its initial and full batches before sleeping on its input
  descriptor and stop notification. Events, queue saturation and worker health
  wake Session. A bounded read that leaves queued input rings again.
- Libseat waits on its connection and command notification. Device requests
  and sender loss wake it; callbacks and broker failure wake Session. Descriptor
  and dispatch failures are reported rather than retried in a busy loop.
- The three X11 writer queues use cancellable notification waits. An empty
  output spill sleeps until output or cancellation; a blocked socket uses
  writability and the existing silence deadline. Stop does not need the output
  mutex. Queue capacity changes wake deferred producers.
- The live routed X11 frontend waits on its listener and work notification.
  Session commands, routed input, control completion, lease changes, raster
  work and worker departure notify it. The listener is excluded at capacity.
- Session polls one owner notification alongside the shell sockets it serves
  inline. Input, seat, X11, WM, output-peer and control arrivals notify it.
  Inline shell wires turn on each service pass, so readiness is consumed.

An otherwise quiet normal physical Session retains a 25 ms maintenance bound
for child reaping, configuration, allocation cadence and protocol expiry. Held
input, releases, frames, topology, seat transitions, shell capture and lifecycle
work retain the short budget. Renderer-device replacement acknowledgements are
included. Frame deadlines and rollback pacing still cap the wait. Proof sessions
retain their previous short cadence.

Raw channel callers and the budgeted private frontend retain their compatibility
polling; only callers that attach every producer may use an untimed wait. The
output-peer worker's own service loop is unchanged. Thread consolidation,
application cadence, Mesa workers and diagnostic collectors are outside this
change. No wire protocol changes were needed.

### Faster teardown exposed two lifetime bugs

The X11 gate first failed because idle reclamation removed a departed recipient's
custody while another client's failed peer write still named it. Reclamation now
requires both zero controls for that client and no unresolved peer debt naming
that exact recipient. An unreadable debt table defers reclamation. The regression
waits for actual teardown and acknowledged service fences, then checks custody.
Removing the debt check fails that named control (logs 16–18).

The Session XTEST departure witness then refused the next pair's key with
`MissingQueryScope` (logs 27, 31–35). The final query client's cleanup erased
the instance's initial pointer observation; preparation published that position
only once. Polling writers had delayed cleanup long enough to hide the gap.
The private instance now owns its root pointer observation. Last-client cleanup
preserves root coordinates, clears the client window and masks, and retires the
old query scope. A replacement receives a fresh scope. Ordinary namespace
cleanup is unchanged. Both the full key witness and the lifetime control cover
this; discarding the observation again fails the named control (log 39).

### Exit fixture correction

Log 37 exposed an assertion that a connection must still be alive after producer
admission closes. `close_production` already asks the independent watchdog to
interrupt sockets, before collection begins. Log 40 reproduces the same failure
on the base source by letting that interrupt finish while collection is paused.
The fixture now forces that schedule and asserts the actual boundary: no worker
join has begun, its handle and exact recipient remain retained, and cleanup has
not discharged its obligation. Existing producer refusal and final collection
assertions remain. The complete X11 rerun passes (log 41).

## Validation and remaining work

Checks run without devices, network or Session sockets, at caller priority with
normal parallelism and a private target. All initial failures remain in the
evidence directory. Mutants use a separate source copy and target; they do not
replace canonical artifacts.

The controls cover publication between inspection and wait, pre-attachment
work, sender loss, cancellation while idle, capacity recovery, socket readiness,
absolute deadlines, full libinput batches, seat failure and slow-client bounds.
Removing disconnect notification, output-drain cancellation notification,
recipient-debt retention or instance-pointer retention fails a named control.
The evidence summary records final commands, results and source hashes.

The default workspace gate passed 4,859 tests (49 ignored); the complete X11
all-features/all-targets gate passed 2,080 (2 ignored). The affected wake, 9P,
backend, runtime and Session all-features/all-targets gate passed 2,517
(57 ignored). These gates overlap; their totals are not additive. Strict Clippy
for the six changed crates, formatting, source layout, metadata and diff checks
pass. Logs 41–49 retain the final checks, including the corrected formatting
of one new test. These checks cover the development source over `dd62b50c8`;
it has not been installed or physically accepted. Combined qualification with
t277 follows the separate feature reviews.

Physical acceptance still needs three matched 60-second samples per workload
and preserved input, frame and restoration behavior. No CPU reduction is claimed
from the device-free tests. The original 84-thread count need not fall: this
change removes idle polling, not workers. The libinput dispatch-gap statistic
now includes intentional idle time; it is not an input-latency measurement.
The live desktop is not modified by development checks. Task t276 owns acceptance.

## Later live CPU evidence, 2026-10-02

The installed `niltempus-9de41ea905db10201b9e` session (Sophia `f650e688`)
was sampled read-only for comparison with `niltempus-2adbe49302088d28c023`
(Sophia `9d3a4190`). Both releases already contain t276, so the observed CPU
reduction cannot establish this task's contribution. The
[t278 investigation](wwr7oaer-reduce-per-frame-capture-and-cpu-raster-cost-without-reusing-live-image-storage.md#live-cpu-evidence-2026-10-02)
records the measurements, corrected normalization and limitations. Raw evidence
and the reproducible comparison are in
`~/.local/state/sophia/development-evidence/t278-live-cpu-01/`.

Task t276 remains open. One sample per workload does not meet the three-sample
criterion, and these samples supply no new latency, frame-pacing, restoration
or teardown measurement. The earlier input checks are separate evidence and
do not substitute for those measurements.

## Connections

- [Sophia X authority](../../sophia-x-authority.md): queue receipts, recipient
  identity and cleanup ownership remain authoritative across the new waits.
- [Generic chording](../plans/lnvx8mly-generic-chording-lifecycle-taps-holds-and-sequences.md):
  t277 must expose its next deadline rather than restore a periodic input poll.

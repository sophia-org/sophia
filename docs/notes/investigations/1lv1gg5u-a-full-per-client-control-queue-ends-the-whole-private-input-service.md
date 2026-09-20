---
id: 1lv1gg5u
date: 2026-09-20
kind: investigation
status: resolved
tags: [investigation]
---
# A full per-client control queue ends the whole private input service

## Question

Under load, a private-input control in `sophia-session` waits for a deferred
control to go out and never sees it. The bridge reports nothing for as long
as it is asked. Is the service starved, is it stalled, or is it gone?

## Evidence

Measured 2026-09-20 on the desktop host (32 cores, a live session), with the
session library suite run under 64 CPU spinners
(`cargo test -p sophia-session --features native-session --lib`, `SOPHIA_*`
cleared). The control is
`a_control_refused_for_now_keeps_its_place_and_its_transaction`.

| stall bound | runs | failed | at |
| --- | --- | --- | --- |
| 20 s, unrepaired control | 10 | 8 | ~21 s |
| 20 s, progress-reset wait | 10 | 7 | ~21 s |
| 150 s, progress-reset wait | 3 | 1 | 151 s |

So the silence was not starvation: the delivery never arrived in 150 seconds
on a machine where the same test finishes in 4.5 seconds two runs out of
three. A probe was then added that stops the service the first time the
bridge reports `Ended` and prints what the stop returns. Six more runs, three
green in 4.5 s and three failing in 4.5 s, all three identical:

```text
invocation Failed
failure Some(Failed { error: X11SetupSocketError {
    message: "X11 route queue is full for client 1",
    client_disconnect: false, client_failure: false, service_shutdown: false }, .. })
execution Some(PrivateExecutionReading { instance: 1, availability: Abandoned })
execution_at_close Some(PrivateExecutionReading { instance: 1, availability: Retained })
interrupted false; service_thread Joined
visits: eight, all SupervisionFailed
order: turns 79 | 49 | 99, taken 1, routed 1, allowance_refusals 11 | 8 | 0,
       blocked_turns 0, unwatched_turns 0, watch_failures 0, producers_issued 6
```

The loop was iterating, the budget was not the cause, the execution
supervisor never tripped, and the invocation ended on a route error. Raw runs:
`scratchpad/t115-ended/run-{3,4,6}.log` of the session that measured it.

On the quiet machine, with the controls repaired to fail at once on `Ended`,
one library run in three still dies the same way, sixteen milliseconds into
the wait.

## Finding and resolution

**A full per-client control queue is treated as a fault the invocation cannot
survive.** The path, read from the tree at `600ebd3c`:

1. The private runner's turn takes the parked operation
   (`routing/private_terminal.rs`, `step_once`, around L470-500): it is taken
   out of `parked`, `attempted` is set, its identity is pushed to
   `outstanding`, and `run_one(operation)` moves it.
2. `run_one` routes it through `route_control_with_completion`
   (`routing/private_producer_surface.rs:190-194`), which ends in
   `route_to_client(client, incarnation, senders.control, Authority { .. })`
   and a `try_send` on the client's control channel.
3. That channel is `sync_channel(per_client_control_capacity)`, and the
   private frontend builds its capacities as `uniform(input_capacity)`, so in
   this control it is **two deep**. A client worker that is merely
   descheduled leaves two undrained items, the send returns `Full`, and
   `route_to_client` answers `ClientQueueFull`.
4. Nothing on the private path matches that variant. The step clears its
   routing attempt and returns the error; `serve_order`
   (`connection/private_service_order.rs:161`) converts it with `to_string`,
   which is why the message carries no prefix, and the loop's `?` ends the
   invocation. The operation is consumed, its credit stays outstanding, and
   nothing is pushed back.

The public broker (`routing/broker.rs`, around L875) treats `ClientQueueFull`
from `route_control` as fatal too; only the input route removes the client
instead. So this is not private-only, but the private frontend's queue depth
is what makes it reachable by ordinary scheduling jitter.

A queue that is full with a live consumer is backpressure, not failure.
Turning it into invocation death makes scheduling jitter fatal, which is the
class of defect the rest of the private instance is built to refuse. The
honest shape is that a `Full` control route parks the turn and retries on the
next one, with the control handed back the way `producer.submit` returns
`(refusal, command)`. One caution for whoever does it:
`route_control_with_completion` does work before the send --
`enter_routing_execution`, `claim_control_execution`,
`retain_control_route_source` and `route_focus_control` -- so a `Full` at the
control send can follow a focus control that already went out in the same
call. Either the check comes before those effects or the retry has to be safe
after them.

The owner is the private-input authority in `sophia-x-authority`; the work is
t130. The controls that exposed it are repaired separately under t115 so that
this death is reported as itself, in milliseconds, rather than as a bound
expiring twenty seconds later.

## Validation and remaining work

- [x] Establish what the silence is: the service is gone, by the stop report
      above, not starved and not stalled.
- [x] Locate the path and the queue depth, from source.
- [x] t130, resolved 2026-09-20 on the private path, by deferring the
      message rather than the operation (below). The session library suite
      under `LOAD=64`, five runs: the control that found this passed five of
      five, where before the fix it died in most runs; one unrelated
      live-session test,
      `profile_preparation_tests::pregraphics_policy_launch_failure_rolls_back_before_returning`,
      failed once in five under that load, which is the t131 class and is
      recorded there rather than here.
- [ ] The twelve gate-isolated workspace runs t115 records, now that the
      death is gone.
- [x] The public broker's `route_control` keeps its fatal arm: decided, and
      pinned by `the_public_broker_still_answers_a_full_control_queue_as_the_fault_it_was`.

## Resolved: a full private control channel defers the message

Built 2026-09-20 on `t130/park-full-control`, by the M6 lane; the row was
filed by the adapter lane, whose note w0p6eocj points here. The caution
above decided the shape: routing a control does work before its send, and
for a focus change the FocusOut to the previous client has already gone
out, so a retry of the operation would repeat effects. What is deferred is
therefore the exact message. The registry keeps a per-client backlog of
controls a full channel would not take, each with the connection it was
routed to; `route_control_to_client` sends what that client was owed
earlier first and then the new control, and keeps it on `Full`, so the
per-client order holds and nothing routed later overtakes it;
`flush_control_backlog` runs every service turn after the order is served.
A client whose channel has gone has its kept controls acknowledged
`ClientGone`, as the public router acknowledges a control to a departed
client, and its row removed by its own identity; a successor under the
same number never receives a predecessor's control. The backlog needs no
bound of its own: every routed control holds an accepted-item credit from
the settlement store, and a focus change adds at most one FocusOut. The
private path only: a private instance installs a control-completion
registry before exposure and the public broker never does, so the public
broker's arm for a full queue is the fault it always was.

Four controls in `tests/support/private_control_backlog.rs`: two kept
controls sent in order when the channel drains; a focus change whose
FocusOut went out at once, whose focus moved once, and whose own message
waited and went once; a kept control for a channel that is gone,
acknowledged once and never crossing to a successor; and the public broker
still answering `ClientQueueFull`.

## Connections

- [Private input controls fail on a wall clock deadline under load](yo5l2jui-private-input-controls-fail-on-a-wall-clock-deadline-under-load.md) --
  the controls whose silence this explains, and the repair that now reports
  this death by name.
- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  the instance this queue belongs to.
- [M4 private Session acceptance and what mutation showed](../milestones/pq4wr7xn-m4-private-session-acceptance-and-what-mutation-showed.md) --
  the containment properties a retry must keep.

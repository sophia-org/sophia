---
id: w0p6eocj
date: 2026-09-20
kind: investigation
status: investigating
tags: [investigation]
---
# A fifth admission ends the private service once four connections have departed

## Question

With departed connections now collected promptly (c2931f65 closed the first
half of t134), does a private instance keep admitting connections after
several have departed?

## Evidence

Measured 2026-09-20 on master 5b5da9e6 with a temporary probe at the end of
`crates/sophia-session/tests/xtest_acceptance.rs` (patch retained in the
measuring session's scratchpad as `t134-probe-5b5da9e6.patch`). One private
instance, `EnabledWithVerifiedEvidence`, `max_concurrent_clients` four,
`input_capacity` eight. Each round: connect an admitted client, send a
FakeInput with delay `u32::MAX` and a GetInputFocus behind it, read nothing
for 300 ms, drop the connection, and read the boundary until its rows close.

| connection | what happened |
| --- | --- |
| 1, parked, dropped | collected in 10 ms, open rows 0 of 0 |
| 2, parked, dropped | collected in 10 ms |
| 3, plain round trip, dropped | collected in 10 ms |
| 4, parked, dropped | collected in 10 ms |
| 5 | setup completes; QueryExtension answered by a reset |

The stop then reports:

```text
invocation Failed
failure X11SetupSocketError { message: "failed to register X11 client route:
    no retained place is available for X11 route client 5",
    client_disconnect: false, client_failure: false, service_shutdown: false }
workers [true, true, true, true]
```

## Finding and resolution

Two facts, one repaired and one not.

**Collection is repaired.** Every departure, parked or plain, first or
later, is collected within 10 ms. The first half of t134 closes on this
measurement.

**The places are not given back.** Four connections departed and four
places were retained for them; the fifth admission finds none. It is raised
as `ContinuationUnavailable` in `routing/registry.rs` (around lines 450 to
484) while registering the client's route, and nothing on the private path
treats it as a refusal of that one connection: the error reaches the service
loop and the invocation ends, with the fifth client reset after a completed
setup. This is the class t130 named, a per-client condition ending the whole
service, reachable here by four ordinary departures on an instance that
admits four at once.

**The reclamation is written and never wired.** The adapter lane read it
before the row landed, and the source agrees:
`routing/private_ordered_continuation.rs` carries
`drive_ordered_continuations` (line 628) and `return_ordered_continuation`
(line 709), complete, under
`#[cfg_attr(not(test), allow(dead_code))] // Handed over by teardown; read by
a driver that is not attached yet.` The driver has no production caller at
all; only `x11_socket/tests/routing.rs` drives it, four visits at a time.
The return is reached from the driver and from the retained drive in
`private_retained_drive.rs`, and that drive runs under the same keeper,
verified at three lines: its entry point `drive_retained_output_step` is at
`private_retained_drive.rs:271`, its only production caller is
`private_maintenance_scheduler.rs:188` inside `maintain_step`, and
`maintain_step` is what `private_input/service.rs:534` runs a bounded
number of times after the invocation has ended. So t138 is not "write
reclamation"; it is "attach the driver that was written", and the code says
so about itself.

The lane's generalisation is worth keeping beside it: reclamation in this
instance is written against a maintenance keeper that runs only after the
invocation ends, by a bounded number of visits on the way out, so every
reclamation path hung off it is dead during the run. The lifecycle sweep
was one such path, and c2931f65 gave a departing connection a full pass of
its own instead of a shared cursor unit; the continuation driver is
another, with no live attachment at all. Each path has had to be given a
live driver separately. That is three paths on that keeper, the lifecycle
sweep, the terminal visits and the retained output, and one, the
continuation driver, with no attachment; anyone taking t138 should grep for
the keeper before assuming their path is live during the run.

The owner is the private-input authority in `sophia-x-authority`; the work
is t138, taken by the adapter lane. Until it lands, an instance that has
seen `max_concurrent_clients` departures is one admission away from ending.

## Second measurement: the driver was not the whole of it

Measured 2026-09-20 on master with the same probe widened to ten rounds. The
driver is attached now -- `drive_departed_continuations` on the registry, run
by each departing connection at the end of its teardown, and again by an
admission before it refuses. With the drive instrumented, the fifth admission
reports:

```text
PROBE reserve-retry places=4 driven=0 standings=["Live", "Live", "Live", "Live"]
```

**The places are Live, not Retained, so there was never anything for the
driver to reclaim.** The drive visits only retained homes, deliberately: a
live home belongs to a connection that may still be bound into, and driving
it would close a wire out from under whoever holds it. It was right to
decline. What is wrong is upstream: four connections had gone and not one of
their homes had been told so.

`retain()` is reached from exactly two places, and neither runs here.

The first is the synchronous cleanup, which `Drop for
XServerFrontendClientRouteRegistration` runs only for a registration with no
ordered custody. A connection that started an ordered worker -- which is
every XTEST client in this probe -- takes the other branch, and
`private_destruction.rs` says what that branch does in as many words:

> THE DEFERRED BRANCH DOES NOTHING ELSE. No standing change, no fence, no
> lease transfer, no number-keyed removal, no place return: every one of
> those is the synchronous body's, and the synchronous body is not run.

The second is `run_deferred_cleanup`, and it is gated. Its prerequisites
require a `PrivateConnectionsCollected` token, and the only mint is
`connections_collected`, which requires `active_client_worker_count() == 0`
and is documented as "minted only after the wait". The two callers of
`run_deferred_cleanups` are the private service's `collect` and its `drop`.
So the discharge that would retain the home can only run once the invocation
is over.

Which means t138 is not "attach the driver that was written" either. That was
read off the `allow(dead_code)` attribute and the comment beside it, and the
comment was describing the wiring rather than the reason there was nothing to
wire. The general shape the first finding named is right and is worse than it
looked: **the reclamation is not merely hung off a keeper that runs late, it
is gated on a token that cannot honestly be minted during the run.** A
service-wide quiesce is not a fact a live path can establish, because a new
connection may be accepted at any moment -- which is exactly why the mint
sits after the wait.

## Three links, not one, and all of them end at the same place

Following it the rest of the way: everything that would give a place back is
bound to the end of the invocation, by three independent gates.

**One, the departure retains nothing.** A connection with an ordered worker
takes the deferred branch of `Drop for
XServerFrontendClientRouteRegistration`, and `private_destruction.rs` says
that branch performs no standing change and no place return.

**Two, the discharge needs a join nobody publishes during the run.**
`deferred_cleanup_prerequisites` refuses `JoinUnpublished` until the
custody's worker has been joined. The only production reaper is
`collect_attached_workers`, and its only two callers are the private
service's `collect` and its `drop`. (The `allow(dead_code) // Nothing reaps a
worker yet` on `reap` is stale -- it is reached -- but only from there.)

**Three, the discharge needs a token nobody can mint during the run.**
`connections_collected` mints only when `active_client_worker_count() == 0`,
and is documented as "minted only after the wait"; its callers are the same
two.

So a private instance reclaims a departed connection's place exactly once, at
shutdown, for every connection at once. During the run there is no path at
all. That is why the driver attached above finds nothing: it is the last link
of a chain whose first two are also missing.

## What the remaining work is

Not a wiring fix and not one decision -- a live per-connection teardown for
private instances: reap that connection's worker, discharge its cleanup,
reclaim its place, all on the departure rather than at shutdown.

The design question each link raises has an answer already in the source, and
they agree. `clear_namespace_under_number` opens the number's interval itself
and refuses when this record is not the occupant, so the number-keyed effects
do not need a service-wide quiesce to be safe against a successor. And the
same function says the rest is not number-keyed at all:

> THE ENDPOINT WORK ABOVE IS NOT PART OF IT. This connection's gate, home and
> place are its own by identity and were never reached by number, so they
> neither need this permission nor lose anything by being done before it.

The strongest evidence that the body is safe live is that it already runs
live: `run_synchronous_cleanup` performs exactly these two effects from
`Drop`, during the run, with no token at all. The deferred path differs only
in that a worker was started, which is what the join prerequisite is for.

What the collection token genuinely carries is registry identity --
`governs` is an `Arc::ptr_eq`, and a test pins that another registry's token
is no better than none. That property must survive. So the shape is a
warrant that keeps the identity check and drops the quiesce, minted per
departed connection rather than per collected service, and accepted only by
the deferred-cleanup prerequisite -- not by the other paths that take the
same token today (`private_retained_drive`, `private_terminal_drive`,
`private_control_cleanup`, `private_invocation_completion`), which have not
been examined and may well need the quiesce.

## Validation and remaining work

- [x] Measure collection after c2931f65: repaired, 10 ms in every round.
- [x] Find what remains: retained places are not released for reuse.
- [x] Attach a live driver for the continuation reclamation:
      `drive_departed_continuations`, run by a departing connection on its way
      out and by an admission before it refuses.
- [x] Refuse rather than end the service when an admission finds no place:
      `ContinuationUnavailable` is now classified a client failure, so the
      frontend disconnects the one connection. Re-measured over ten rounds:
      the invocation reports `failure None` where it previously reported
      `X11SetupSocketError { ... no retained place is available ... }`.
- [x] Find why the live driver reclaims nothing: the three gates above, all
      of which end at the invocation's collection.
- [ ] Build the live per-connection teardown: reap the departing connection's
      worker, discharge its deferred cleanup under a warrant that keeps the
      registry identity check and drops the service-wide quiesce, and let the
      place come back on the departure. Re-run the probe for at least
      `2 * max_concurrent_clients` rounds and require the fifth admission to
      be served rather than merely refused alone.
- [ ] Do not widen the warrant to the other paths that take the collection
      token until each has been examined on its own.

## Connections

- [A full per-client control queue ends the whole private input service](1lv1gg5u-a-full-per-client-control-queue-ends-the-whole-private-input-service.md) --
  the same class: a per-client condition the private turn cannot survive.
- [Private input controls fail on a wall clock deadline under load](yo5l2jui-private-input-controls-fail-on-a-wall-clock-deadline-under-load.md) --
  where the probe's shape and the t134 finding came from.

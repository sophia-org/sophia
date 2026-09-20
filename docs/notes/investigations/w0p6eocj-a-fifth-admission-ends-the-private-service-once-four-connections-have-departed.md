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
`private_retained_drive.rs`, which runs under the same keeper. So t138 is
not "write reclamation"; it is "attach the driver that was written", and
the code says so about itself.

The lane's generalisation is worth keeping beside it: reclamation in this
instance is written against a maintenance keeper that runs only after the
invocation ends, by a bounded number of visits on the way out, so every
reclamation path hung off it is dead during the run. The lifecycle sweep
was one such path, and c2931f65 gave a departing connection a full pass of
its own instead of a shared cursor unit; the continuation driver is
another, with no live attachment at all. Each path has had to be given a
live driver separately.

The owner is the private-input authority in `sophia-x-authority`; the work
is t138, taken by the adapter lane. Until it lands, an instance that has
seen `max_concurrent_clients` departures is one admission away from ending.

## Validation and remaining work

- [x] Measure collection after c2931f65: repaired, 10 ms in every round.
- [x] Find what remains: retained places are not released for reuse.
- [ ] t138: attach a live driver for `drive_ordered_continuations` so a
      departed connection's place returns during the run, and refuse rather
      than end the service when an admission still finds none; re-run the
      probe for at least `2 * max_concurrent_clients` rounds.

## Connections

- [A full per-client control queue ends the whole private input service](1lv1gg5u-a-full-per-client-control-queue-ends-the-whole-private-input-service.md) --
  the same class: a per-client condition the private turn cannot survive.
- [Private input controls fail on a wall clock deadline under load](yo5l2jui-private-input-controls-fail-on-a-wall-clock-deadline-under-load.md) --
  where the probe's shape and the t134 finding came from.

# Administrative file lifecycle model

This is a bounded design check for t254, based on Sophia `5cea0350b` and the
[administrative file proposal](../../../docs/sophia-admin-files.md). It is not
a refinement proof, a runtime trace, or an accepted administrative export.
The [brief](modeling-brief.md) names the scope and exclusions;
[coverage](spec/brief-coverage.md) maps properties to checked configurations.
The [action map](spec/instrumentation-spec.md) identifies existing ticket
boundaries and future file operations.

The final base and merged hunt each explored **970,253 distinct states**, with
3,485,086 generated states, depth 44 and an empty remaining queue. All enabled
invariants passed. The design trace consumed all eleven events and checked
their post-state fields. These finite searches do not prove arbitrary client
counts, runtime scheduling, weak-memory behavior or service restart recovery.

## Service-instance boundary extension

`Restart.tla`, based on `bfd65d84f`, adds a separate two-instance boundary check.
It preserves stop, queued cancellation, settlement, worker expiry, join and fresh
bind as separate steps. An old dispatched ticket may finish after the next
instance accepts a request with the same connection/request numbers; only its
own instance may publish that result. Session keeps its service for its lifetime;
hot replacement beneath surviving command owners is not an admitted product path.

Both `Restart.cfg` and `Restart_hunt.cfg` pass: **352 distinct states**, 702
generated, depth 23, no queued states remaining. The deliberate
`Restart_no_cancel.cfg` fails ShutdownCancels in seven states, and
`Restart_cross_instance.cfg` fails ResultIsolation in ten. The eighteen-event
`RestartTrace.cfg` design fixture passes in nineteen states; its false-settlement
variant exits 13 because expiry cannot claim owner settlement. It checks all
abstract fields but is not a runtime trace.

The runtime socket tests independently check service teardown, fresh identity,
old-fid closure, queued cancellation and late-result isolation. Neither these
tests nor this model prove whole-process crash cleanup, automatic supervision,
random-identity collision resistance or durable cross-restart exactly-once
execution. The client instead preserves uncertainty and never replays a failed
invocation. Live-session recovery and rollback remain rollout evidence.

Use the command below with `Restart.tla` and the corresponding configuration
names to reproduce the extension; use `RestartTrace.tla` for its trace fixtures.
Run these short checks sequentially, or give each JVM a private `java.io.tmpdir`:
concurrent extraction of standard TLA+ modules into shared `/tmp` caused parser
failures. Separate reruns passed without a model change; the final cancellation
control used a private Java temp directory.

## Original lifecycle controls

| Negative configuration | Expected failure | Trace states |
| --- | --- | --- |
| `MC_replay.cfg` | AtMostOnce: the same stale request is accepted twice | 5 |
| `MC_early_release.cfg` | ExecutionCustody: dispatch, timeout, premature reclaim | 8 |
| `MC_consuming_read.cfg` | ResultRetention: read erases the current status | 4 |
| `Trace_bad_settlement.cfg` | TraceMatched: a read falsely claims execution settled | 8, then stuttering |

These are deliberate faulty variants, not production bug reports. The real
ticket code retains active capacity until its settlement flag is acquired.
The file adapter must preserve that rule after publishing Indeterminate.
Newer requests may acknowledge an outcome but may not acknowledge execution.

## Reproduce

Use the repository-pinned TLA+ Tools v1.7.4 jar, SHA-256
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.
The checked local copy was `/home/niltempus/tmp/tla2tools-1.7.4.jar`, with Java
21.0.8. From `spec/`, create the ignored `output/` directory, then run:

```sh
timeout 30m java -Xmx2g -XX:+UseParallelGC -jar /absolute/path/to/tla2tools.jar \
  -deadlock -workers 8 -fp 0 -metadir output/states-hunt \
  -config MC_hunt_custody.cfg MC.tla > output/hunt.log 2>&1
```

`-deadlock` disables terminal-state deadlock reporting; the finite design may
stop after its identities are exhausted. Repeat with `MC.cfg` for structural
checks and each `MC_*.cfg` control above for its expected invariant failure.
Use `Trace.tla` with `Trace.cfg` and `Trace_bad_settlement.cfg` for the trace
checks. Keep separate output and state paths. Passing runs exit 0, invariant
controls exit 12 and the bad trace exits 13. Inspect the named failure, not just
the exit code. Logs and `source.sha256` remain under ignored `spec/output/`;
do not commit state databases or raw logs.

The combined safety hunt took two seconds with eight workers. Other final
checks used one worker each. The trace summary tool parsed the invariant
controls; its temporal-trace parser could not read the bad-trace output, so the
temporal violation and stalled cursor were checked in the raw log.

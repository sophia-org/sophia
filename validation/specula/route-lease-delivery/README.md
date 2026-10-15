# Route-lease notification lifecycle

t330 replaces two blocking frontend-to-Session notification sends. Its
[brief](modeling-brief.md) limits the change to notification custody. This is a
bounded design safety check, not a proof of native input latency or refinement
of the whole runtime.

`MC.cfg` passes all 1,034 distinct states (3,032 generated), depth 18, with no
queued states remaining. It checks bounded reservations/backlog, conservation,
unique delivery, retained effects and stale-epoch authority. The deliberate
drop mutation fails Custody; the stale-confirmation mutation fails Authority.
`Trace.cfg` consumes all eleven design steps in twelve states, including a
full channel, delayed drain and stale consumption. No runtime trace is claimed.

Use TLA+ Tools 1.7.4, SHA-256
`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`.
From `spec/`, create `output/` and run:

```sh
timeout 30m java -Xmx1g -jar /absolute/path/to/tla2tools-1.7.4.jar \
  -deadlock -workers 4 -metadir output/states -config MC.cfg base.tla
```

Use separate state directories for `MC_drop.cfg` and `MC_stale.cfg`; each must
report its named invariant violation, not just a nonzero exit. For the design
fixture, use `Trace.tla` with `Trace.cfg`. The terminal design intentionally
permits stuttering, so `-deadlock` disables terminal deadlock reporting.

The [coverage map](spec/brief-coverage.md) and
[action map](spec/instrumentation-spec.md) distinguish model and runtime
boundaries. Capacity-return wakes and nonblocking return need Rust tests.
Exhausting the separate notification reserve is an explicit service failure
before another effect; it is not a guarantee of uninterrupted availability
under arbitrary overload. Disconnect may leave a completed effect without a
delivered notification, which is reported as abandoned delivery, never as an
undone effect.

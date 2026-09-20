---
id: yo5l2jui
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, tooling, session]
---
# Private input controls fail on a wall clock deadline under load

## Question

`cargo xtask check` intermittently fails a private-input control that passes
alone and passes contained. Is the production code at fault, is the control at
fault, and was it introduced by the build-cost work that landed beside it?

## Evidence

Failures observed under `cargo test --workspace --all-features` with the gate's
own isolation (`XDG_CONFIG_HOME` redirected, `SOPHIA_SHELL_CONFIG` and
`SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE` removed), which is what
`check.rs:386` sets up:

| tree | runs | failures |
| --- | --- | --- |
| `master` at 92ca9b56 | 12 | 3 |
| build-cost branch at afda8c77 | 12 | 3 |

The same control, `a_control_refused_for_now_keeps_its_place_and_its_transaction`,
accounts for most of both. A second control,
`an_unreadable_surface_ledger_refuses_rather_than_reporting_an_empty_one`, failed
once. Run alone the first control passed 10 of 10; under the package's own lib
suite, 5 of 5; contained as part of the `private-session-lifetime` component
suite, 27 of 27.

**The rate is the same on both trees, so the build-cost work did not introduce
it.** `debug = "line-tables-only"` changes DWARF sections, not generated code,
so it can shift timing but cannot create a race.

### Mechanism

`private_input_session.rs` defines `WAIT` as 20 seconds and uses it as a
wall-clock deadline in **15 controls**, each shaped like:

```rust
let deadline = std::time::Instant::now() + WAIT;
while !delivered {
    let report = fixture.handle_mut().apply_committed(Duration::from_millis(50))?;
    // ... scan effects, set `delivered` ...
    assert!(std::time::Instant::now() < deadline, "...");
}
```

The loop pumps until the service thread redelivers. Under
`--workspace --all-features` the suite competes with several hundred other
tests across many binaries, and the service thread can be starved past the
budget. Nothing asserts that the system behaved wrongly; the control gives up
waiting.

### Why this matters more than an ordinary flake

These controls were written to kill named mutations — the first one exists to
kill M6, "discard retained bridge work". A control that fails on a timeout
reports the same outcome whether the mutation is present or the machine is
merely busy. That is the precise failure mode the M4 mutation exercise existed
to find: a control that passes or fails for reasons other than the defect it
names.

The assertion message compounds it. It reads:

```text
the deferred control was delivered under its original transaction
```

which describes the **success** condition, but only fires on timeout. A reader
is told the opposite of what happened.

### What the wait is actually starved by (2026-09-19)

Measured while adding three library tests for the XTEST injection policy, two
of which start a private input service. The run is
`cargo test -p sophia-session --features native-session --lib`, with `SOPHIA_*`
cleared, on the desktop host with a live session:

| library binary | wall time | outcome |
| --- | --- | --- |
| pristine `45aee143` | 4.06 s | 4 of 4 green |
| plus one non-starting test | 4.06 s | 2 of 2 green |
| plus a second service-starting test | 4.06 s | 2 of 2 green |
| plus a third, starting two more services | 20.1 s | failed 2 of 3 |

The two failures were not the same test: one was
`a_control_refused_for_now_keeps_its_place_and_its_transaction`, the other
`a_stop_counts_receipts_nobody_drained`. The 20-second runs are the failing
test spending its whole budget; the passing runs of the same binary finish in
4.06 seconds, so the cost is the deadline being waited out, not the suite
being slower. Cutting the added tests back to one service returned the binary
to 4.06 seconds and 4 of 4.

This answers the first open item below. The wait is **starved, not stalled**:
the same binary, the same tests and the same machine pass or fail according to
how many other services are being started beside them, and the bridge is
making progress throughout. It also decides between the candidate
resolutions. Waiting on progress rather than elapsed time is the repair that
addresses this; raising the budget moves the threshold by about one
service-start, which is the margin that was just consumed by three ordinary
tests.

## Finding and resolution

The production code is not implicated by this evidence. The defect is in the
controls: they bound an asynchronous wait with a wall clock, on a budget that
full-suite concurrency can exceed, and report the timeout in language that
describes success.

Candidate resolutions, none yet chosen:

1. **Wait on progress, not elapsed time.** Reset the budget whenever the pump
   observes any effect, so the deadline measures a stalled bridge rather than a
   busy machine.
2. **Raise the budget.** Cheapest, and it moves the failure rate without
   addressing the class; a busier machine reinstates it.
3. **Bound concurrency for this package** so the controls do not compete with
   the rest of the workspace. Hides the fragility rather than removing it.

Whichever is chosen, the 15 assertion messages should state what actually
failed.

## Validation and remaining work

- [x] Establish whether the wait is starved or genuinely stalled: starved, by
      the controlled measurement above. Adding service-starting tests to the
      binary decides it, and the bridge keeps making progress.
- [ ] Choose a resolution and apply it to all 15 deadline sites.
- [ ] Correct the assertion messages.
- [ ] Re-measure over at least 12 runs under the gate's isolation, against the
      3-in-12 baseline recorded above.

Open work is tracked as t115 in `todo.md`.

## Connections

- [M4 private Session acceptance and what mutation showed](../milestones/pq4wr7xn-m4-private-session-acceptance-and-what-mutation-showed.md) —
  records the controls these deadlines belong to, and why a control that can
  pass or fail for the wrong reason is the specific risk that milestone
  addressed.

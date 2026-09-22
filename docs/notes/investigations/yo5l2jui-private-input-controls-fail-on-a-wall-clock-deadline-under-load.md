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

### What the wait was first read as (2026-09-19)

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

This was read, on 2026-09-19, as the wait being starved rather than stalled,
because the same binary passed or failed according to how many other services
were started beside it. That reading was wrong, and the next section says what
the silence actually was. What the measurement does still show is that the
margin is one service-start wide.

### What the silence actually was (2026-09-20)

The progress-reset wait was built first and measured under a deterministic
load: 64 CPU spinners on the 32-core host, session library suite only.

| control | stall bound | runs | failed | at |
| --- | --- | --- | --- | --- |
| unrepaired | 20 s wall clock | 10 | 8 | ~21 s |
| progress-reset | 20 s stall | 10 | 7 | ~21 s |
| progress-reset | 150 s stall | 3 | 1 | 151 s |

Starvation and stall produce the same silence, so the repair could not tell
them apart, and at 150 seconds the awaited delivery had still not arrived on
a machine that finishes the test in 4.5 seconds otherwise. A probe that stops
the service on the first `Ended` and prints the stop then found the answer in
three failing runs out of six, identical each time:

```text
X11 route queue is full for client 1
```

**The service had died.** A bridge whose service has ended reports `Ended` on
every `apply_committed`, which is correctly not progress, so the wait sat on a
dead service until its bound expired and then reported the bound. The death
itself, a two-deep per-client control queue filled by a merely descheduled
client worker and a route error the private turn cannot survive, is a product
defect in `sophia-x-authority`, recorded in
[A full per-client control queue ends the whole private input service](1lv1gg5u-a-full-per-client-control-queue-ends-the-whole-private-input-service.md)
and filed as t130.

Also seen once in six under the same load, in a different family:
`policy_transport_worker_tests::rejected_profile_admission_fails_before_negotiated`
asserts `try_event()` is `Err` after the failure event and found another
event there. Filed as t132.

## Finding and resolution

Two defects, one in the controls and one in the product.

**The controls** bounded an asynchronous wait with a wall clock, on a budget
that suite concurrency can exceed, reported the timeout in language that
described success, and could not tell a starved bridge from a dead one.
Resolved 2026-09-20 in `private_input_session.rs`, all fifteen sites:

- One loop under two entry points, `wait_for` and `spin_for`, generates every
  failure message. A site names its goal as a noun phrase and supplies a
  `seen` closure that is evaluated only on failure, so the old shape, a
  sentence that reads as true, is no longer expressible.
- `wait_for` resets a 20-second stall bound on any movement of the bridge
  (`advanced`: batches observed, commits, a submitted effect, or a refusal
  that released its entry) and keeps a 120-second ceiling for a mutation
  that keeps the bridge busy without delivering. A deferred head's restated
  refusal is deliberately not movement. `spin_for` is for the waits with no
  signal at all (a delivery's terminal answer, a boundary row, an armed
  fault) and is bounded by a 60-second clock that says so.
- A bridge that reports `Ended`, or a service whose readiness is no longer
  `Ready`, is `Progress::Lost`: the wait stops the service and fails at once
  with the stop's own account. On this host that turned a 20-second silence
  into a 16-millisecond failure reading
  `the private input service ended while this waited; ... X11 route queue is
  full for client 1`.
- `Fixture::pump` now judges movement before it moves the effects into the
  harvest, and `Fixture::step` is the one pump-and-scan shape the sites use.

**The product** ends the private service on a full control queue. That is
t130, and until it lands the twelve-run bar below cannot be met: the controls
will fail at the recorded rate, but in milliseconds and by name.

## Validation and remaining work

- [x] Establish whether the wait is starved or genuinely stalled: neither.
      The service was dead, by the stop report of 2026-09-20.
- [x] Choose a resolution and apply it to all 15 deadline sites: progress
      waits with a ceiling, and `Lost` on a dead service.
- [x] Correct the assertion messages: generated, none written at a site.
- [x] Deterministic trigger before and after: 64 spinners. Before, 8 of 10
      failing at the 20-second bound with a message describing success.
      After, the same runs fail at once naming the route-queue death; on the
      quiet machine, 1 of 3 library runs, in 16 ms.
- [x] Re-measure over at least 12 runs under the gate's isolation once t130
      has landed, against the 3-in-12 baseline. Done 2026-09-22 on `03fd1437`:
      **12 runs, 12 green, zero failures**, against 3 in 12 on `92ca9b56`.
      Each run is `cargo test --offline --workspace --all-features` under the
      isolation `crates/xtask/src/check.rs:515` sets up -- a fresh mode-0700
      `XDG_CONFIG_HOME`, `SOPHIA_SHELL_CONFIG` and
      `SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE` removed -- on the 32-core desktop
      host with no added load, which is the baseline's own profile. Logs are
      retained per run in `.artifacts/t115-wait-remeasure/`.

      The three controls the baseline named --
      `a_control_refused_for_now_keeps_its_place_and_its_transaction`,
      `an_unreadable_surface_ledger_refuses_rather_than_reporting_an_empty_one`
      and `a_stop_counts_receipts_nobody_drained` -- each report `ok` in all
      twelve, so this is twelve executions of the controls and not twelve
      suites that skipped them. Runs took 88 to 90 seconds, a two-second
      spread: the old shape spent its whole 20-second budget before failing,
      so a wall-clock give-up would have stood out in the duration alone.
      Between runs the executed count varies by one,
      `gbm_backed_platform::native_gbm_backed_platform_maps_open_failure_to_unavailable`,
      which depends on the host's GBM device and is unrelated to these
      controls.

      Twelve of twelve does not prove the rate is zero; it puts it below what
      twelve runs can resolve, against a baseline that failed three times in
      the same number.
- [ ] The same wall-clock shape elsewhere is t131: `x11_socket/tests/routing.rs`
      (12 sites), `tests/support/m3_acceptance_c.rs` (9),
      `tests/connection_wait.rs` (6), the desktop comparison `workload.rs` (6),
      the xterm command (6), the shell launcher (5), and the fixed-step class
      such as
      `state_only_frozen_release_retains_original_request_then_applies_once_after_exact_thaw`
      in `tests/support/private_state_only.rs`, which pumps a fixed count and
      cannot say whether it waited long enough.

Open work is tracked as t115, t130, t131 and t132 in `todo.md`.

## Connections

- [A full per-client control queue ends the whole private input service](1lv1gg5u-a-full-per-client-control-queue-ends-the-whole-private-input-service.md) --
  what the silence was.
- [M4 private Session acceptance and what mutation showed](../milestones/pq4wr7xn-m4-private-session-acceptance-and-what-mutation-showed.md) —
  records the controls these deadlines belong to, and why a control that can
  pass or fail for the wrong reason is the specific risk that milestone
  addressed.

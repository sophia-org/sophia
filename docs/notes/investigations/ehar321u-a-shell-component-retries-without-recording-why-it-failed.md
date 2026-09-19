---
id: ehar321u
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, tooling, session]
---
# A shell component retries without recording why it failed

## Question

The Lom bar restarts once per second and never comes up. The session log says
it failed, 841 times, and does not say why. Can the cause be established from
retained evidence, and if not, what should the supervisor have recorded?

## Evidence

Session `00000001789821426620-6a16a92d-bc45-475a-80e7-108b00b0d4ca`, observed
while investigating [glxgears pacing](lz1rbbmr-glxgears-runs-slow-under-dual-monitor-pacing.md),
which recorded the loop as a separate fault found on the way.

Retry cadence, from `events.0.log`, two consecutive lines one second apart:

```text
809856  1789824616755  730746859  sophia_shell_component schema=1 status=start_failed
810452  1789824617755  730747859  sophia_shell_component schema=1 status=start_failed
```

- 841 `start_failed` records across the session, 303 in `events.0.log` alone.
- No `lom` process is running.
- The emitting statement is `component_service.rs:107`, which formats
  `status=start_failed reason={error}`. **The retained line carries no
  `reason=` field.** Evidence reduction excludes arbitrary error text by
  design, as `docs/operations.md` states: an unclassified failure "does not
  imply that the raw error was retained".

So the supervisor's own evidence cannot distinguish a missing binary, a
sandbox mount failure, a negotiation timeout, or a configuration rejection.

### A hypothesis that the source contradicts

An external analysis proposed that `OwnedProtectionFilesystem::drop`
(`supervisor/protection/owned_filesystem.rs:147`) deletes the sandbox root, so
later launches fail to mount it. The source does delete the root on drop, but
the mechanism cannot produce the described loop:

- `create_root` (`owned_filesystem.rs:188`) draws a fresh `getrandom` nonce per
  call, so every `materialize` yields a new `sophia-protection-<pid>-<nonce>`.
- The instance is held as `Arc<OwnedProtectionFilesystem>` inside the
  `ProtectionPath` that references it (`protection.rs:45`, `protection.rs:221`),
  for the lifetime of the protection-domain specification.

Either the specification is rebuilt per launch, in which case the new root is
fresh, or it is retained, in which case the `Arc` is alive and the drop has not
run. Recorded here because it is a plausible story that the retained evidence
was too thin to refute, which is the actual problem.

### A related coarseness worth separating

`component_service.rs:225` maps every error from the content pipeline to
`IndicatorServiceError::Poll`, and `Poll` logs `service_failed` and calls
`components.stop(key)`. Transient peer backpressure and a genuinely broken peer
are therefore indistinguishable at that boundary. This is a plausible trigger
for the first stop, but it is not evidence for it, and it does not explain the
repeated `start_failed` that follows.

## Finding and resolution

No cause is established, and the path that destroys it is now traced end to
end. `session_eprintln!` (`lib.rs:16`) reaches `output::stderr`, installed by
`sophia-cli/src/main.rs:28` as `session_stderr`, which calls
`diagnostics::capture_line` and falls back to raw `eprintln!` only when capture
declines. Capture does not decline: `capture.rs:44` returns `true` even when it
drops the line, so the fallback never fires and the text is destroyed rather
than downgraded.

`reduced_record` filters by a field allowlist. For `sophia_shell_component`
(`diagnostics/shell_component.rs:13`) the permitted keys are `schema`, `status`,
`role`, `gpu_mode`, `endpoint_released`, `slot`, `revision`, `device_major`,
`device_minor`, and three epochs. **`reason` is not among them**, and correctly
so: it is free text.

Three separable defects, in increasing order of cost:

1. **Two emitters drop an allowlisted field they already have.** Of the eleven
   `sophia_shell_component` emitters in `component_service.rs`, nine pass
   `slot={}`. `start_failed` (`:107`) and `poll_failed` (`:97`) do not, so their
   records reduce to `schema` and `status` alone and cannot even name the
   component that failed. The sibling `service_failed` at `:240` passes it.
   This is a one-line repair per emitter and would have made the 841 records
   attributable.
2. **No allowlisted field can carry a cause.** The status vocabulary says
   *that* a start failed, never why. An enumerated cause key — spawn, mount,
   negotiation timeout, configuration rejection — added to the allowlist and
   emitted would classify the failure without retaining arbitrary error text,
   which is what the reduction discipline actually requires.
3. **The retry is unbounded.** An identical failure repeating 841 times at 1 Hz
   is not a retry policy. After a bounded number of identical outcomes the
   supervisor should stop and say so once.

Whether the content-pipeline error classification should distinguish transient
backpressure is a fourth question, tracked with these because it shares the
boundary.

## Validation and remaining work

- [x] Name the component whose start failed — `start_next` now records the slot
      it selected and the caller reports it, cleared on entry so a failure
      raised before any selection is not misattributed (`6dd557bd`).
      `poll_failed` is deliberately left without one: it is the error arm for a
      poll across every slot, so it has no single component to name.
- [x] Space the retry — consecutive failures now widen the interval from one
      second to a sixty-second ceiling, recorded once on the visit it widens
      under a new `start_backoff` status. The spacing never becomes infinite,
      so a condition that clears on its own can still bring the component up
      (`658dad52`).
- [ ] **Find and fix why the Lom bar cannot start.** Nothing here establishes
      it. The failure originates inside `processes.start` — either
      `plan.prepare`, which builds the protection specification and
      materialises the sandbox, or the spawn itself. Reproduce with the reason
      visible: outside the supervisor no sink is installed, so `capture_line`
      returns false and `session_stderr` falls through to a plain `eprintln!`.
- [ ] Decide the approved start-failure cause codes and emit them, so the next
      occurrence is diagnosable from retained evidence rather than from a live
      reproduction.
- [ ] Separately assess whether `WouldBlock` on the panel socket should stop
      the component at `component_service.rs:225`.

Open work is tracked as t114 in `todo.md`.

## Connections

- [glxgears runs slow under dual-monitor pacing](lz1rbbmr-glxgears-runs-slow-under-dual-monitor-pacing.md) —
  where this loop was first recorded, as a fault found alongside the pacing
  question rather than a contributor to it.

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

No cause is established. The finding is that the supervisor retries an
unrecoverable start once per second, indefinitely, while recording nothing that
would let anyone say why — and that the reduction which strips `reason=` is
working as specified, so the gap is in what the supervisor classifies, not in
the log's discipline.

Two separable pieces of work:

1. **Record a classified start failure.** `reason={error}` is free text and is
   correctly dropped. The supervisor needs approved failure codes for the start
   path, the way lifecycle and guard records already carry them, so the retained
   line distinguishes a spawn failure from a mount failure from a negotiation
   timeout without retaining arbitrary error text.
2. **Bound or escalate the retry.** An identical failure repeating 841 times is
   not a retry policy; after a bounded number of identical outcomes the
   supervisor should stop and say so once.

Whether the content-pipeline error classification should distinguish transient
backpressure is a third question, tracked with the first because it shares the
boundary.

## Validation and remaining work

- [ ] Reproduce with the reason visible — run the component with
      `verbose_diagnostics`, or launch it outside the supervisor — and record
      the actual first failure, which nothing here establishes.
- [ ] Decide the approved start-failure codes and emit them.
- [ ] Bound the retry, and prove the bound with a component that cannot start.
- [ ] Separately assess whether `WouldBlock` on the panel socket should stop
      the component at `component_service.rs:225`.

Open work is tracked as t114 in `todo.md`.

## Connections

- [glxgears runs slow under dual-monitor pacing](lz1rbbmr-glxgears-runs-slow-under-dual-monitor-pacing.md) —
  where this loop was first recorded, as a fault found alongside the pacing
  question rather than a contributor to it.

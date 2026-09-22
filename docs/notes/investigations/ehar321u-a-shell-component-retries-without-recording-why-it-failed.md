---
id: ehar321u
date: 2026-09-19
kind: investigation
status: closed
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

## The cause, isolated by experiment

Changing the bar component's `gpu "direct"` to `gpu "denied"` in `desktop.kdl`
and restarting the session **removes `start_failed` entirely**. On release
`c5f064f6` the same session instead records, for slot 0:

| record | count |
| --- | --- |
| `negotiated slot=0` | 38 |
| `service_failed slot=0` | 37 |
| `process_retired slot=0` | 38 |
| `start_failed` | **0** |

So the start failure was the per-component direct GPU grant, not the binary,
the configuration, bubblewrap, or the sandbox lifetime that the refuted
hypothesis proposed.

`shell_gpu_device` (`live_session/render_devices.rs:76`) admits a device only
when the active one is available, present in the admitted inventory, and
**unambiguous** there. This host offers two:

| node | PCI | device | |
| --- | --- | --- | --- |
| `card0` / `renderD128` | 03:00.0 | `0x744c` Navi 31 | RX 7900 GRE, discrete |
| `card1` / `renderD129` | 16:00.0 | `0x164e` Raphael | integrated APU graphics |

Both are `amdgpu`. Which of the three errors that function raises applies is
still unknown, because all three travel in `reason=` and are reduced away —
the gap this investigation is about. The shell GPU grant not resolving on a
dual-GPU host is the defect to repair.

Denying the grant is not a workaround. The bar then starts and negotiates but
fails in service once per second, because the content it is there to present
needs the access it was refused. The loop moves rather than stops.

That shape also escapes the retry spacing added in `658dad52`: the backoff
counts consecutive **start** failures, and here every start succeeds. A
start-then-service-fail cycle is not spaced by it.

## Finding and resolution

The cause is established above, by experiment rather than from the log — which
is itself the finding. The path that destroyed the evidence is traced end to
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
   is not a retry policy. Spaced rather than halted in `658dad52`; note that
   it counts start failures only.

Whether the content-pipeline error classification should distinguish transient
backpressure is a fourth question, tracked with these because it shares the
boundary.

## The bar on the release now installed

On the evening of 2026-09-19 the live session
(`00000001789865605605-d9c9a7e7…`, release `d461492d`, started 20:53) runs
the Lom bar under the session's bubblewrap with `gpu "direct"`, in the same
profile (digest `aa2d41cc…`), on the same boot and the same two GPUs as the
morning. Its four retained event logs, from 21:19 on, hold no
`sophia_shell_component` record of any kind: no `start_failed`, no
`service_failed`, no `process_retired`. The negotiation itself was rotated out
with the session's first twenty-six minutes. The morning session
(`…21426620`, release `92ca9b56`, 08:37) recorded 6369 `start_failed` and
never one `negotiated`.

Between the two releases lie 56 commits. On the component start path there
are only the three diagnostics commits (`6dd557bd`, `658dad52`, `65ccbc96`);
none touches the render device coordinator, the GPU grant, or the protected
launch. So the morning's refusal was a condition of those sessions rather
than a defect the code repairs deterministically on a dual-GPU host, and its
text is gone with them. One thing the source does settle: `shell_gpu_device`
is resolved once, in `component_lifecycle::prepare`, and a refusal there
fails the session start. The morning session started, so its per-attempt
refusal came from later in the start path -- the grant's revalidation, the
protection domain, or the process layer -- every one of which now carries a
cause code. The next occurrence is diagnosable from `cause=` alone, which is
what this investigation set out to make true.

Three more things settled the same evening:

- **The cause vocabulary broke the crate without its native feature.** The
  reducer in `diagnostics/shell_component.rs` reached into the feature-gated
  `live_session` module for the admitted tokens, so `cargo check -p
  sophia-session` and the repository's plain `cargo test` had failed since
  `65ccbc96`. The vocabulary now lives at the crate root, in
  `component_start_cause.rs`, shared by the emitter and the reducer.
- **A start-then-service-fail cycle is spaced.** The count that spaces a
  slot's retry is now its consecutive failures, where a failure is a refused
  start or a stop after `service_failed`; a process that served a healthy
  tenure (sixty seconds, the ceiling) before failing begins a new count. A
  successful start no longer clears the count, because a component that
  comes up and fails within its tenure is still looping. `start_backoff` is
  recorded on the visit the spacing widens, from either path. Pinned by
  `component_session/scheduling/tests.rs` and by the scheduler test in
  `tests/shell_component_processes.rs`.
- **Backpressure does not stop the bar, by design.** Socket-level
  `WouldBlock` never reaches the service: the shell transport's write and
  read loops stop at it and keep their queues
  (`crates/sophia-runtime/src/shell_transport.rs`). The one queue-level
  signal, `ContentQueueSaturated`, is already tolerated where a presentation
  acknowledgement may lag (`observe_presentation` in
  `metadata_shell/content.rs` returns `Ok(false)`); everywhere else it means
  the peer stopped consuming its bounded ordered queue, which the policy IPC
  contract defines as endpoint failure that revokes interaction
  (`docs/sophia-policy-ipc.md`, `docs/target-resolved-input.md`). Stopping
  the component there is that contract, not coarseness. No change.

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
- [x] Establish why the Lom bar cannot start — the per-component direct GPU
      grant. Denying it removes `start_failed` outright, which isolates the
      cause by experiment rather than inference.
- [x] **The morning's refusal is a watch item, not a task.** `shell_gpu_device`
      cannot have been it: it is resolved once at session start, and the
      session started. The bar starts on `d461492d` with the same profile and
      GPUs, and every start refusal now carries a cause code, so a recurrence
      names its path in the retained record. Reopen from that `cause=`
      value; there is nothing to repair until then.
- [x] Space a start-then-service-fail cycle -- a stop after `service_failed`
      counts with refused starts, and a healthy tenure resets the count.
- [x] Decide the approved start-failure cause codes and emit them
      (`65ccbc96`, `25c3bc5b`), and let the reducer's copy of them build in
      every configuration.
- [x] Separately assess whether `WouldBlock` on the panel socket should stop
      the component: it never reaches the service, and queue saturation is
      endpoint failure by contract. No change.
- [x] **Refuse an unsatisfiable direct grant at prepare** rather than starting
      and failing in service for ever (t117). See the section below.

t114 was closed on 2026-09-19 with the watch item above; a recurrence reopens it from the retained cause.

## Refused at prepare, 2026-09-22

t114 closed with the retry spaced and the cause vocabulary in place. t117's own
clause was the one thing neither covered: a component whose declared GPU mode
cannot be satisfied should be refused **once**, not started and failed in
service at every backoff interval. Spacing that loop to a sixty-second ceiling
made it cheap; it did not make it finite.

**The wording had to be corrected against the architecture.** t117 asks to
refuse a mode that "cannot produce a matching content grant", but ADR
[mn4mzcnf](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
holds presentation and GPU execution as separate contracts -- "GPU permission
grants no content capability, and content permission grants no GPU" -- so no
mode produces a content grant at all. The code agrees: the content admission
policy is session-wide, built in `component_lifecycle.rs` from
`shell_content_enabled`, and never reads `entry.gpu`. The predicate implemented
is the ADR's own admission rule instead: implementation, operator policy and
launch resources must all agree, and *"the required launch resources must also
be established before client code runs"* -- which is exactly prepare.

**What the old prepare did.** It resolved one device for the whole selection
with `any()`, then propagated both failures with `?`. That is simultaneously
all-or-nothing and session-fatal, while the observed defect is the opposite:
prepare succeeds and every start fails afterwards. A refusal is now captured
rather than propagated, and the selection is split -- every `direct` entry is
refused with a cause, every `denied` entry is admitted and keeps the CPU
rasterize path the ADR permits, which needs no device. When nothing survives,
prepare yields no component session rather than an error: a component that
cannot get a device is not a reason to withhold the desktop. The session
shell's own path at `live_session.rs` is deliberately unchanged and stays
session-fatal, being a different contract.

Each refusal emits one `status=start_refused cause=gpu_grant slot={} role={}
gpu_mode=direct`. `start_refused` is new and was added to the reduction
allowlist; without that the record is dropped silently, which is the original
defect in miniature and is what the reduction test asserts against. The cause
reuses `GpuGrant`, whose definition already covers it. `slot` is the declared
position rather than a runtime one -- a refused component never reaches the
process layer to be assigned one -- and carrying it at all is the repair this
investigation existed to make.

**A gap found while doing it.** `shell_gpu_device`'s three refusals -- the
device unavailable, absent from the admitted inventory, or **ambiguous** in it
-- were absent from the cause table, so each would have reported `cause=other`,
the "gap in this table" the module's own doc warns about. They never reached it
before because they failed session start instead. All three are classified as
`gpu_grant` now and pinned in `component_start_cause/tests.rs`. The ambiguous
one is the dual-GPU condition this investigation opened on, and nothing had
exercised that branch.

Validation on `a506cfbf`: the whole `sophia-session` crate passes under the
isolation `crates/xtask/src/check.rs` sets up -- 44 suites, zero failures --
with `cargo fmt --check`, `git diff --check` and clippy over all targets clean.
Five new tests in `tests/support/component_prepare_refusal.rs`. Both halves
were mutation-checked rather than trusted: removing `start_refused` from the
allowlist strips the status and fails the reduction test, and dropping the
ambiguous-device arm from `classify` fails both the new test and the existing
`every_refusal_the_start_path_raises_carries_a_code`.

The run that first reported a failure here was *not* run under that isolation
and failed an unrelated WM test, `hagia_pregraphics_profile_admission_rejects_
invalid_policy_values`, by reading the developer's live desktop configuration --
the exact thing `check.rs` clears and says it clears. It passes in all twelve
retained runs of `.artifacts/t115-wait-remeasure/` and under the isolation here.
No physical acceptance is claimed, and the morning's original refusal remains
t114's watch item, reopened from a retained `cause=`.

## Connections

- [glxgears runs slow under dual-monitor pacing](lz1rbbmr-glxgears-runs-slow-under-dual-monitor-pacing.md) —
  where this loop was first recorded, as a fault found alongside the pacing
  question rather than a contributor to it.

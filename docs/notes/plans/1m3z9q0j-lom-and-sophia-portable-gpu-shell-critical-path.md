---
id: 1m3z9q0j
date: 2026-09-13
kind: plan
status: active
tags: [plan, shell, gpu, lom, daily-driver]
---
# Lom and Sophia portable GPU shell critical path

## Scope and exit

Deliver one protected Lom shell with workspace/active-output indicators, clock,
calendar popout and preserved descriptor launcher/switcher behavior. It must
work on supported stock Linux installations under the accepted
[presentation/execution decision](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md).
No custom kernel, mandatory GPU bridge, Waybridge, Vello dependency in Sophia,
or second native shell is on this path.

The CPU content implementation and Lom's persistent client are the starting
point. Production GPU admission is implemented behind a default-denied profile,
while content input and native acceptance remain closed/incomplete;
writing this plan is not evidence of an admitted desktop. Preserve previous
model, fixture and incident evidence under its exact source identity.

The exit is a paired, exact-release daily-driver acceptance, with bounded
protocol/storage obligations and honest direct-GPU availability limits. The
operator still authorizes installation and attended runs separately. Offline
validation uses the device-hidden wrapper, not merely cleared opt-in variables.

## Repository ownership and sequence

`sophia:tNNN` and `lom:tNNN` are repository-qualified identities. `depends:` in
each todo.txt file refers to that repository; `peer:lom/tNNN` or
`peer:sophia/tNNN` links counterpart work. Peers share evidence where appropriate
but do not inherit each other's completion. Stable IDs and status live in each
repository's task files; this plan and
[Lom's plan](https://github.com/sophia-org/lom/blob/master/docs/notes/plans/pf4er77j-lom-daily-driver-critical-path.md)
own scope and exits.

| Step | Sophia owns | Lom owns | Integration exit |
| --- | --- | --- | --- |
| 1 | t097: explicit launch grant, compositor charge boundary | t003: admitted device selection and asynchronous renderer | Protected launch can prove/refuse the requested resources on stock Linux |
| 2 | t098: exact presented targets, action dispatch and settlement | t004: view bindings, TEA action intake and acknowledgements | A real private protocol exchange activates only the exact presented pill |
| 3 | t099: popout composition, precedence and dismissal | t005: calendar state and complete panel/popout candidates | Open, action, dismissal and parent-loss transitions settle coherently |
| 4 | t100: output/epoch/recovery lifecycle | t006: per-output scheduling and reconnection | Fresh state after every transition; no stale action or retained obligation |
| 5 | t101: shared client workflow support and package verification | t020: combined descriptor workflows; t007: package/configuration | One shell preserves desktop controls and has verified rollback |
| 6 | t081: native-shell milestone and evidence | t008: attended matrix; t009: soak; t010: promotion | Exact installed candidate earns both repositories' acceptance claims |

Steps 1 and 2 can develop independently using contained fixtures. Step 3 needs
step 2; step 4 tests can begin earlier but must validate the combined path.
Descriptor compatibility can develop in parallel with those implementations;
packaging and native acceptance wait for all of them. Existing higher-priority
X11/input work retains its ownership and queue order. Before implementation or
integration, coordinate shared files, use separate disk-backed build targets,
and reconcile with the current master; this plan imports no unfinished branch.

For each server/client seam: specify the state/identity and terminal outcomes,
update lifecycle models under the evidence policy, retain negative controls,
implement behind the gate, and test the real protocol/production owner path.
A model pass does not substitute for code, routing or physical evidence.

## Task details

### t097

Replace the old hard-quota admission placeholder with an explicit default-denied
startup GPU grant. Implement the successor ADR's target profile and refuse the
old `gpu-memory-bytes` form with a migration message; never turn an old quota
request into direct-access consent. Keep CPU-only content clients independent
of GPU permission and keep content itself default denied.

Use the existing typed profile/protection-domain launch owners. Establish exact
child, device and grant identities before exposing one render node and releasing
client execution. Record effective permission, driver inputs and refusal reasons;
no display, input, KMS or unrelated device mounts. Missing support, stale grants,
ambiguous selection and failure midway through launch must clean up without
publishing a content grant. A reconnect cannot inherit predecessor rights.

Close compositor-owned upload/backing accounting before production admission:
reserve known peak overlap before accepting obligations, retain charges through
native retirement, and refuse saturation without losing prior accepted work.
Reconcile it with the existing CPU pool instead of inventing a second unbounded
ledger. Neither this ledger nor per-import limits count all driver VRAM.

Exit: device-hidden policy/launch negative tests and models pass; a separately
authorized contained GPU proof verifies actual selected-device access, exclusion
of other devices, matching Lom adapter, and retirement/backing accounting.
Retain a capability/prerequisite record for an unmodified distribution kernel;
report untested driver combinations. A fake cgroup/controller fixture or a bool
set to true cannot satisfy the gate. No main compositor startup is needed for
the isolated proof. Modelled recovery cannot promise a stalled kernel returns.

Implementation checkpoint: the profile migration, exact typed render-node bind,
connection-epoch grant, device-replacement revocation, and conservative
content-backing credit are implemented with deterministic tests and the
`ShellGpuLaunchAdmission` model. This does not close t097: the isolated hardware
proof, other-device exclusion, Lom adapter match, and native retirement evidence
remain separately authorized and unrun.

The next candidate adds a production-policy isolated runner and a dedicated
tty4 native gate. The isolated runner launches Lom through the same GPU policy,
mounts one render node and no display, card or input device, verifies a real
nonuniform 256x24 candidate, then deliberately settles it as RendererFailed and
requires lease/backing cleanup. The native gate uses the tracked Minimal
workspaces-and-seconds configuration and requires two presented generations per
output. These are runnable gates, not results: neither has been executed on
hardware, and t097 remains active until their retained evidence passes.

The first attended native attempt on `adc25c43` at
`.artifacts/lom-panel-native/20260913T200334Z` failed before any content output
facts or presentation. The protected shell negotiated repeatedly but exited
after each grant (704 grant epochs in 20 seconds). The proof profile carried an
unnecessary terminal binding while Hagia rejected and restarted its
configuration 389 times; the reduced record did not retain the rejection reason,
so the binding's causal role is unproved. The watchdog restored the TTY, keyboard
mode and keyd. This is failed hardware evidence, not native acceptance and not
proof of Lom's process-exit cause because the diagnostic-mode runner
intentionally discarded role stderr.

The successor gate removes the unrelated application and binding, requires the
bounded protected GPU/content proof to pass before graphics takeover, and reads
native verdicts from the structured event file rather than the wrapper log. A
failed GPU or content prerequisite now reports the client's boundary error and
stops before Sophia acquires the display. No successor hardware result has
been recorded yet; t097 remains active.

The protected preflight on `61c37741` at
`.artifacts/lom-panel-native/20260913T202120Z` stopped before graphics takeover
because no enumerated Vulkan adapter passed Lom's grant selector. This confirms
the fail-closed boundary and does not distinguish an empty adapter inventory
from a missing or mismatched optional Vulkan PCI-bus identity. The successor
candidate carries host-observed PCI vendor/device identifiers as a bounded
fallback inside the already-single-render-node domain and records bounded
adapter counts if selection still fails. Its causal role and the native bar
remain unproved until a new attended result is retained.

The successor preflight on `bb97c561` at
`.artifacts/lom-panel-native/20260913T204021Z` enumerated one adapter but zero
non-CPU Vulkan adapters. Source and binary inspection established that the
protected domain omitted `/sys`, while RADV's libdrm discovery requires the
selected render minor's sysfs DRM and PCI identity. Adapter matching therefore
never ran; carrying PCI fields could not restore an adapter the driver had
discarded. The repair generates an immutable one-device discovery projection,
preserves the actual render-minor basename and makes Lom authorize only an exact
`VK_EXT_physical_device_drm` render-major/minor match. This explains the observed
enumeration failure; a later driver initialization failure remains possible and
requires the separately authorized protected hardware proof.

### t098

Carry content target tables through production projection to the exact native
presentation. Prepared frames cannot publish targets. Bind dispatch to grant,
output, allocation, candidate, target and interaction generations; clip against
the actual presented allocation and enforce security/occlusion precedence.

Implement the existing discrete-input wire lifecycle, including bounded queues,
activation acknowledgements, duplicates, cancellation, timeout and revocation.
Route workspace intent through the existing authorized indicator action token;
do not turn a widget index into a capability or reuse unrelated toplevel actions.
Do not synthesize a general pointer stream for a toolkit.

Exit: model controls and deterministic production-owner/private-socket tests
prove one correct action and reject unpresented, stale, duplicate, revoked,
old-epoch, wrong-output and changed-meaning targets. Delayed rendering cannot
block action settlement. Test queued, routed and client-observed outcomes as
different facts. Keep descriptor and independent wire corpus tests passing.

### t099

Finish the already-specified allocation/candidate popout path in the live owner.
Use acknowledged parent-allocation physical coordinates and presentation epoch;
do not reconstruct integer logical anchors from fractional scaling. A complete
candidate carries the panel/popout resources and matching targets, with Engine
coverage, reservation and precedence rules applied together.

Outside dismissal is Engine-owned and consumed without coordinates or
click-through. Timeout triggers coherent withdrawal by Engine rather than
waiting indefinitely for client cooperation. Parent loss, output loss, security
takeover and revocation invalidate targets immediately, retaining any referenced
pixels until their independent leases retire.

Exit: owner-path tests cover open, target action, replacement, dismissal,
timeout, rejected placement, parent/epoch loss and backing retirement. Include
the ADR's fractional-scale anchor and negative-margin cases. This integrates
existing allocation machinery; it does not invent a new popup protocol.
Exercise the full panel/popout lifecycle with the independent C client as well
as Lom through the protected conformance host; shared Rust code alone cannot
prove the cross-language contract. Keep native input/presentation separate.

### t100

Complete production topology/output-fact publication, fresh reconnect grants,
pending and submitted retirement, reservation/work-area coherence, and bounded
failure recovery. Exercise rapid output removal/addition, scale/allocation
replacement, disconnect during every phase, security revocation, and client
restart with old rendering retained. Keep protocol/control progress independent
of client GPU scheduling; a slow output must not starve another output's
control obligations.

Exit: deterministic tests drive real production owners and private transport
across each transition with exact old/new identities, no duplicate settlement,
no reuse before final release, and bounded retained bytes/metadata. Account for
terminal feedback undeliverable after peer loss. Preserve unrelated input and
indicator behavior; a synthetic host-only encoder is insufficient evidence.

### t101

Support one combined content/descriptor shell in the shared client boundary,
including bounded message demultiplexing and terminal outcomes for every
negotiated workflow. Lom currently consumes content and indicators; receipt of
other negotiated records must not fill an unserviced queue. Keep the original
descriptor switcher/launcher trust and activation rules. No second Narthex
connection and no custom raster launcher authority is implied.

Package exact Sophia and Lom commits, dependency lock, binaries, profile and
private KDL assets. The verifier must distinguish requested `gpu "direct"`,
actual launch evidence and granted content/input capabilities. Retain the
selected policy client identity and a known Narthex profile/artifact for rollback
in a separate session. Coordinate existing packaging tasks rather than marking
them complete from this narrower artifact.

Exit: combined-client fixtures preserve descriptor controls through content
updates and reconnect; exact package and profile validation pass with identity,
missing-resource, old-quota and permission mutation negatives. Document the
authorized install/run/rollback procedure. No GUI, signing prompt, session
restart or install follows automatically from a passing offline verifier.

### t081

Use the paired package to prove actual native panel pixels/reservation on every
admitted output, empty-focused-output styling, workspace activation, clock,
calendar interaction/dismissal, descriptor launcher/switcher behavior and
recovery. Tie input to exact native retirement, not `Prepared`, renderer
completion or a synthetic `Presented` record. Narthex is tested as rollback,
not simultaneous native-shell coexistence.

Lom t008 owns the attended matrix and t009 the measured workload/soak; this task
retains the Sophia-side evidence and incident links. Record source, installed
binary hash, effective profile/grants, device/driver, topology, refresh and
observation duration. Define workload and latency/resource acceptance budgets
before running. Report action and frame p50/p95/p99, upload/readback/copy costs,
idle wakeups, queue depth and retained bytes. Check that old generations drain
and warmed storage does not grow with repeated actions, not merely that one
sample is below a limit. Report driver/device incidents under direct-access
limitations, never as guarantees the architecture cannot make.

Exit: attended matrix and soak pass for the exact candidate with a tested
rollback; failures reopen the relevant task. GPU or simulated-client evidence
alone cannot close this task. Broader desktop acceptance tasks retain their own
exits. Lom t010 records promotion only after both repositories agree on evidence.

### t102

Candidate only; paired with Lom t016. Measure the full CPU-byte path before
promoting memfd or DMA-BUF transport. A promoted design must retain immutable
acceptance, exact target pairing, synchronization, recipient isolation,
cross-device/format behavior, finite overlap accounting and final release.
Compare end-to-end latency and bandwidth on the same workload. Do not claim
zero-copy from a handle transfer. Lom t017 first measures reuse of existing
immutable resources/placement tables; r5 already supports tiles. Deltas remain
separately specified future vocabulary.

### t103

Candidate only; paired with Lom t019. A GPU bridge requires a concrete isolation
or performance need the direct path cannot meet. Select a bounded job/adapter
contract, enforceable accounting owner, scheduling and failure model, a second
independent client/renderer proof, and measurements before implementation.
Opaque commands, a process boundary or one job at a time do not establish hard
GPU-memory isolation. No custom kernel, mandatory Vello API or new display
server is admitted by this candidate row.

## Connections

The [implementation record](../../lom-content-implementation.md) owns historical
claims. [Content shells](../../content-shell.md), the
[wire ADR](../decisions/6ndjwffd-content-capability-design-for-sophia_shell_v1.md),
[target-resolved input](../../target-resolved-input.md), and the
[successor GPU decision](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
own the contracts. [Sophia tasks](../../../todo.md) and
[Lom tasks](https://github.com/sophia-org/lom/blob/master/todo.md) own status.

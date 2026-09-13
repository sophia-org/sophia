---
id: mn4mzcnf
date: 2026-09-13
kind: adr
status: accepted
tags: [adr, shell, gpu, portability, lom]
---
# Separate shell presentation from GPU execution permission

## Context

Sophia must support ordinary Linux distributions without requiring a custom
kernel or a particular GPU accounting controller. Lom should retain GPU
rendering and its Xilem/Masonry/Vello stack without making that stack a Sophia
dependency. A shell author should implement one presentation lifecycle, not a
Sophia GPU instruction set or another display server.

The earlier [cgroup dmem decision](odjw4jav-confine-the-lom-gpu-domain-with-cgroup-dmem.md)
made enforceable aggregate GPU-memory isolation a prerequisite for Lom. This
decision supersedes that prerequisite and its kernel/provisioner prescription.
It does not claim that a render-node bind enforces the former quota. It accepts
a different, explicit execution trust level while retaining presentation,
disclosure, resource-ownership and input constraints.

## Decision

### Presentation and execution are separate contracts

`sophia_shell_v1` admits complete visual candidates: immutable images,
allocation/output generations, reservation, interaction snapshot, and admitted
effects. Engine owns placement, composition, exact native retirement, physical
target selection, and revocation. Shell rendering produces its own pixels; it
does not confer presentation or input authority.

| Boundary | Required contract |
| --- | --- |
| Shell author to Sophia | Renderer-neutral images, complete candidates, bounded pacing, explicit outcomes and release |
| Session to shell process | Independently selected execution permissions and effective launch evidence |
| Lom to its renderer | Private Vello/wgpu adapter, bounded scheduling, completion/failure observations |
| Engine to its renderer | Existing trusted compositor integration and resource leases |

The first transport remains the existing immutable CPU-byte content protocol.
Lom renders on the GPU, performs bounded readback, and sends those bytes. This
decision changes no wire layout, capability assignment or protocol revision.
Other clients may CPU-rasterize under content permission without requesting a
GPU. They need neither Vello nor a GPU bridge. A client using a conventional
display-based toolkit still needs an appropriate downstream adapter; this
decision does not supply GTK with a hidden display endpoint.

### Explicit direct GPU permission is the first production path

The accepted target profile separates content from execution:

```kdl
shell {
    content #true
    gpu "direct"
}
```

`gpu "denied"` is the default. This is **target syntax, not implemented syntax**
at the time of this decision. The existing `gpu-memory-bytes` setting must be
retired with an actionable migration refusal when this path lands. It must
never be silently reinterpreted as advisory accounting or consent to direct
access. No installed profile changes as a consequence of accepting this ADR.

The operator grants the selected executable permission to execute GPU work.
That grant is distinct from content negotiation: GPU permission grants no
content capability, and content permission grants no GPU. Implementation,
client request and operator permission must all agree for content admission;
the required launch resources must also be established before client code runs.

Session constructs a typed launch grant, bound to the child/domain and a fresh
launch epoch. It resolves and verifies exactly one permitted render node, its
kernel device identity and required read-only driver assets. The protected
domain exposes that node only; it gains no primary DRM/KMS device, input device,
application X11/Wayland socket, host service bus, cgroupfs, or broad filesystem
access. Device selection cannot depend on inherited display variables or the
first adapter returned by an enumeration. Lom must verify that its selected
GPU matches the admitted device and refuse unsupported or ambiguous selection.

Effective-profile and launch evidence distinguish requested policy, actual
device access, missing implementation and refusal. Replacement needs a fresh
grant; old handles cannot authorize new work. Revocation stops new admission
and input immediately and initiates child shutdown. It does not pretend to
retract an already-open GPU file descriptor or cancel driver work atomically.
Referenced storage remains owned until its actual consumers finish.

This path uses upstream Linux device interfaces and available userspace
drivers. No custom kernel, `dmem` controller, distribution-specific patch,
Waybridge or private display server is a prerequisite. Missing required device
or confinement support produces a clear refusal, not a broader fallback.
Portability means capability detection and documented prerequisites, not a
claim that every kernel, driver and device combination has been tested.

### State the security and accounting limit precisely

Direct access trusts the shell with GPU-driver execution and its resource
availability consequences. The driver and kernel remain part of the trusted
computing base. There is **no portable hard aggregate VRAM quota promised by
this mode**, no hard GPU-time partition, and no guarantee that a shell fault
cannot cause device loss or affect desktop availability. Process isolation and
one-job scheduling do not supply those guarantees. An operator requiring hard
GPU isolation must keep direct access denied unless a separately supported and
verified enforcement mechanism meets that requirement.

This does not enlarge the shell's permitted information or desktop authority.
There is no authorized path to foreign application images, composed desktop
pixels, global input, blind WM state, application execution or ungranted service
data. Sampling foreign scene content remains an Engine-owned effect. The shell
wire accepts no arbitrary shaders or renderer programs.

Finite bounds still apply at owners Sophia can enforce: transfer and resident
bytes, retained generations and leases, candidate/target queues, control
outcomes, compositor-owned backing allocations, and outstanding jobs. Charge
known storage and peak copy/upload overlap before accepting obligations. Keep
charges until the last consumer retires. Do not label requested texture-byte
accounting as a measurement or limit of opaque driver allocations. The current
CPU ledger does not yet establish every compositor upload charge; closing that
gap remains a production prerequisite.

Lom retains one outstanding render job and the existing 2000 ms readback
recovery deadline as prototype limits. Move rendering off the protocol owner so
bounded socket, action and revocation progress does not wait for GPU polling.
A deadline requests recovery; it neither proves GPU completion nor bounds an
arbitrary synchronous driver call. A supervised process can isolate a stalled
renderer from protocol progress without guaranteeing that the kernel can kill
or reclaim a stuck GPU operation by a deadline.

### Optional optimization and mediation

A future image transport may admit sealed shared memory or DMA-BUF when
retained measurements justify it. It needs a separate negotiated contract for
recipient ownership, immutable acceptance, formats/modifiers, synchronization,
cross-device behavior, fallback, budget overlap and release. A fence alone does
not prevent subsequent producer writes. Importing a DMA-BUF does not establish
zero-copy composition or scanout.

A `gpu-bridge` or `gpu-embed` service is an optional execution architecture,
not the universal shell API or a kernel project. No crate name or ABI is
allocated here. A later design must identify the workloads, trust in submitted
programs, trusted renderer adapters, resource owner, scheduling, restart and
failure boundaries. Executing opaque GPU jobs in a helper does not by itself
provide enforceable allocation accounting. A broker must demonstrate its
claimed limits rather than moving the unbounded allocator to another process.
Sophia must still accept the same presentation contract from other renderers.

Keep unchanged resources reusable, render dirty outputs, use negotiated
permits/backpressure, and retain control progress under load. Measure both sides:
local render/readback, IPC bytes/copies, Engine upload/composition, and actual
native retirement; also measure action latency, idle wakeups and memory. Report
workload, dimensions, scale, refresh, device/driver and percentile distributions.
Choose optimization against a recorded acceptance budget, not GPU preference
alone. Do not make an unmeasured zero-copy path or mediated service a prerequisite
for the first useful panel.

## Alternatives

- Mandatory `dmem` and a custom kernel: superseded because these exclude the
  required generic Linux deployment. Optional verified hardening can return as
  a distinct mode with explicit guarantees.
- Mandatory broker or Vello service: defers the shell behind another execution
  API and makes a renderer choice part of Sophia's public boundary. A measured
  mediation use case can justify it later.
- CPU-only Lom: available to other shell designs, but contrary to Lom's chosen
  GPU path; it is not a silent fallback for a denied direct grant.
- Waybridge or a private toolkit display: adds a display authority the operator
  rejected and is unnecessary for Lom's non-Winit adapter.

## Consequences

Shell authors choose tools locally and implement a small, language-neutral
presentation and action lifecycle. The optional Rust client library assists
with that lifecycle; it is not required to implement the protocol. Sophia owns
the resource/authority checks it can actually enforce, without learning a
widget toolkit or claiming portable GPU denial-of-service isolation.

The current runtime still fails closed on the old GPU admission placeholder.
This ADR does not turn it on. Launch-policy migration, precise backing charges,
presented content input, recovery, packaging and attended evidence remain on the
[paired critical path](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md).
One shell slot remains the rule: Lom may combine content and descriptors;
Narthex is the independent reference and rollback client, not a second native
shell running beside Lom.

## Acceptance and connections

Accepted on 2026-09-13 by the operator's instruction to update both repositories
and synchronize the critical path around this architecture, following the
generic-Linux, GPU-preferred and renderer-independent decisions. This accepts
the design and implementation plan, not a hardware, installation or native
acceptance result. New lifecycle/authority transitions still require models,
checked invariants and conformance evidence before production promotion.

This supersedes [odjw4jav](odjw4jav-confine-the-lom-gpu-domain-with-cgroup-dmem.md)
and amends the execution prerequisite, not the bytes or finite content budgets,
of [6ndjwffd](6ndjwffd-content-capability-design-for-sophia_shell_v1.md).
[Architecture](../../architecture.md), [content shells](../../content-shell.md),
[compositor graphics](../../compositor-graphics.md) and the
[implementation record](../../lom-content-implementation.md) distinguish the
accepted contract from implemented behavior. [Lom's adoption](https://github.com/sophia-org/lom/blob/master/docs/notes/decisions/1qikt1av-use-explicit-gpu-permission-with-renderer-neutral-sophia-presentation.md)
binds its client design to this decision.

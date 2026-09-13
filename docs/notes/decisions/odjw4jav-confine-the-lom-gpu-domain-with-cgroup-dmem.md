---
id: odjw4jav
date: 2026-09-13
kind: adr
status: superseded
tags: [adr, shell, gpu, cgroup, dmem, lom]
---
# Confine the Lom GPU domain with cgroup dmem

Superseded on 2026-09-13 by
[Separate shell presentation from GPU execution permission](mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md).
The original decision and host observations below are retained as history. A
custom kernel and hard GPU-memory quota are no longer production prerequisites;
the successor explicitly accepts direct GPU execution risk without claiming
that a device bind enforces this record's quota.

## Context

[The content capability](6ndjwffd-content-capability-design-for-sophia_shell_v1.md)
admits immutable CPU pixels but deliberately does not grant a GPU. Lom renders
with Xilem, Masonry and Vello before transferring those pixels, so its protected
shell domain needs one render node. A render-node bind identifies a device; it
does not bound how much device memory an untrusted shell can retain.

Sophia's DRI3 checks bound each import but do not sum imports, and they do not
cover Vello's internal buffers. One active render job bounds concurrency, not
residency. Production content therefore stays unavailable until the launch
boundary can enforce aggregate limits.

The inspected host runs `6.18.50_1`. Its unified cgroup hierarchy exposes no
`dmem` controller and its kernel configuration says `CONFIG_CGROUP_DMEM` is
disabled. Upstream AMDGPU commit
`bd4f284df04d76fd65e57141cb1e6e7a49e4c3cb` registers the VRAM manager with
the device-memory cgroup controller and supplies bounded reclaim.

## Decision

Lom's first production GPU grant uses cgroup v2 device-memory accounting. It is
a startup protection-domain resource grant, not a shell capability bit and not
an implication of content admission.

The first limits are:

| Account | Limit | Owner |
| --- | ---: | --- |
| AMDGPU VRAM region `dmem.max` | 256 MiB | root-provisioned shell cgroup |
| process `memory.high` | 768 MiB | root-provisioned shell cgroup |
| process `memory.max` | 1 GiB | root-provisioned shell cgroup |
| process `pids.max` | 64 | root-provisioned shell cgroup |
| concurrent Vello jobs | 1 | Lom runtime |
| render/readback deadline | 2000 ms | Lom runtime |

`shell { gpu-memory-bytes 268435456; }` names the effective VRAM ceiling. The
first implementation admits exactly that value; changing the prototype limit
requires new measurement and evidence rather than a silent compiled default.
Content remains denied unless the profile also says `content #true`.

A root-owned login provisioner enables `memory`, `pids` and `dmem` top-down and
creates fixed desktop and shell child cgroups. Limit files remain root-owned.
The session owner may move only a stopped child it created between those fixed
children. The child cannot execute shell code or open the GPU before membership
is verified. The shell Bubblewrap domain does not mount cgroupfs.

For every shell epoch Sophia:

1. resolves one render node and its PCI identity;
2. finds the matching region in `dmem.capacity`, without assuming a region
   suffix;
3. writes and reads back every limit before the child runs;
4. moves the stopped child into the shell cgroup and verifies membership;
5. binds only the selected render node into Bubblewrap; and
6. releases the child to execute.

Any missing controller, ambiguous device or region, failed readback, unexpected
membership, stale epoch or retained predecessor refuses the GPU grant. A new
epoch cannot reuse the old epoch's cgroup identity or evidence. Device loss
revokes new work immediately; submitted work remains owned until the kernel and
renderer report completion or failure. Deadlines trigger recovery and never
authorize reuse.

`dmem.max` bounds VRAM residency, not every GPU-accessible allocation. The
paired normal-memory limit is part of the grant. A saturation proof must show
that Vello's VRAM and GTT/system-backed allocations increase `dmem.current` or
`memory.current`. Unexplained uncharged growth blocks production admission.

Sophia's compositor-side upload belongs to the content connection rather than
Lom's cgroup. It reserves exact pixel bytes at peak overlap in the existing
resident ledger before creating a backing, retains that charge through native
retirement and keeps one backing per immutable resource generation.

The prototype kernel is built as a separate Void package from the verified
upstream commit above or a later signed source containing it. The installed
6.18 kernel stays bootable as the fallback. A general desktop rollout requires
a signed stable kernel tag containing the AMDGPU support.

## Alternatives

CPU raster would avoid a render-node grant but conflicts with the selected GPU
renderer and is not a silent fallback. Giving Lom an unaccounted render node
would make content permission ambient device permission. A Wayland bridge or
GTK platform endpoint would add another display authority and was explicitly
rejected. Direct DMA-BUF content transfer remains a later measured protocol,
not part of this grant.

## Consequences

The kernel and login cgroup provisioner join the shell's trusted computing base.
Production content cannot run on the current kernel. Offline protocol,
compositor and Lom work may proceed, but no profile may grant the render node
until the controller, accounting probes and negative limits pass.

The first path still reads Vello output back to immutable CPU bytes. This costs
a copy but keeps the ratified content lifecycle intact and separates client GPU
completion from Sophia native presentation.

## Acceptance and connections

The operator selected kernel device-memory control on 2026-09-13 and approved
implementation of the critical path. This accepts the design and authorizes its
models and implementation; it does not claim that the current kernel or a live
session satisfies it.

The current boundaries are [content shells](../../content-shell.md), the
[content capability ADR](6ndjwffd-content-capability-design-for-sophia_shell_v1.md),
and the [Lom implementation record](../../lom-content-implementation.md).

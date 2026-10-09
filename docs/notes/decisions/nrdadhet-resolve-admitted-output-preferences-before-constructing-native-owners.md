---
id: nrdadhet
date: 2026-10-09
kind: adr
status: proposed
tags: [adr, session, rendering]
---
# Resolve admitted output preferences before constructing native owners

## Context
The accepted startup port fallback reconciles after constructing native heads.
Runtime rebuild still discovers every connected head. That ordering cannot
enforce exclusions before allocation, distinguish no-output waiting from invalid
configuration, or preserve stable identity across GPU node renumbering.

## Decision

Separate admitted probing, pure preference resolution and owner construction.
Session excludes exact stable GPU identities within the seat before opening
nodes. Backend discovery retains only probe descriptors and passive capabilities;
Session resolves the desired profile against them. Waiting releases the probes
without taking custody of suspended images or lock state. An active resolution
selects exact advertised timings and allocates CRTCs and planes only to its
chosen heads, then constructs the native owner.

Keep configured preferences separate from committed realization. Hardware can
move one workspace affinity to a fallback and restore it later, while reloads
cannot redefine the configured identity. Continuity owns suspension and retained
images; policy owns realized geometry and keys. Runtime publication must bind
those to the same transition and presentation barrier.

## Alternatives

Filtering after constructing all heads lets excluded connectors consume resources
and creates renderer contexts before policy has admitted them. Matching only
connector names or card numbers cannot distinguish duplicate names across GPUs
or survive node renumbering. Inventing a head for an empty inventory hides the
waiting state and publishes geometry that cannot present.

## Consequences

Probe and construction lifetimes become explicit, and policy tests need no
devices. Full DRM timings remain intact across the configuration projection.
Identity is revalidated before use; a changed device refuses rather than falling
back to another node. Startup, runtime rebuild, seat return and recovery now use
the boundary. Reload compares a pure resolution before retaining its current
owner or scheduling a rebuild. The realization ledger binds publication to the
current owner, notice, transition, profile and presentation; a failed adaptive
replacement gets one conservative attempt before waiting. The full isolated
gate passes on `83c68c7c4`. Native retained-image continuity and attended physical
acceptance remain open.

## Acceptance and connections

Proposed implementation of niltempus's 2026-10-09 safe-fallback decision. Keep
proposed through combined integration review and qualification.

- [t310 implementation and acceptance plan](../plans/cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md)
- [Current configuration contract](../../configuration.md#available-outputs-at-startup)
- [Incident and candidate evidence](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#t310)

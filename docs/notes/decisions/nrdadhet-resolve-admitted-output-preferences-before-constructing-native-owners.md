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
current owner, notice, transition, profile and presentation. Startup permits one
conservative attempt; after the first physical return failure, runtime retries
retain conservative settings through a bounded 250/1,000/4,000 ms series before
waiting, with a 250 ms coalescing window for notifications. The full isolated
gate passes on `83c68c7c4`. Native retained-image continuity and attended physical
acceptance remain open.

The second physical failure distinguishes an unavailable output from a refused
activation. With no output, a runtime-suspended GPU can miss a cable return;
Session therefore retains a five-second admitted discovery probe after the
short settling series while the seat is active. These waiting observations do
not consume the finite hardware-refusal allowance. No probing is added to the
ordinary active-output idle path.

The probe cadence and the finite refusal count are separate. A failed probe is
unknown availability and does not replenish the refusal count. Strict runtime
profiles wait for a missing required connector while retaining their settings;
unsupported settings remain refusals. Complete validation owns a temporary
primary-plane framebuffer on each head's card and submits TEST_ONLY with
ALLOW_MODESET, independent of leftover scanout state. Cleanup stays on that
card, and Busy/Rejected retain the kernel errno through durable reporting.

This realizes the physical part of the [persistent service contract](../plans/cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md#persistent-services-and-replaceable-display-attachments).
The logical session persists while physical attachments and grants change
generation; broader public status, namespace and capture work stays separate.

## Acceptance and connections

Proposed implementation of niltempus's 2026-10-09 safe-fallback decision. Keep
proposed through combined integration review and qualification.

- [t310 implementation and acceptance plan](../plans/cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md)
- [Current configuration contract](../../configuration.md#available-outputs-at-startup)
- [Incident and candidate evidence](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#t310)

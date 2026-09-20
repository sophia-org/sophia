---
id: esnqxpqw
date: 2026-09-20
kind: plan
tags: [plan, security, session]
---
# PIDFD and Namespace Admission Optimizations

## Scope and exit

This plan outlines the transition of Sophia's connection admission and ancestry verification from I/O-bound, race-prone `/proc` parsing to a robust kernel-level `pidfd` (Process File Descriptor) tracking model.

The measurable exits for this optimization plan are:
1. **Race-Free Lineage Tracking:** Ancestry tracking is verified as immune to process re-parenting (double-fork / orphan escapes) using `pidfd`.
2. **Sub-Millisecond Connection Vetting:** Setup connection times on the X11 socket must remain sub-millisecond even under heavy concurrent connection rates, eliminating the `/proc` parsing bottleneck.
3. **Fail-Closed Container Admission:** Clean handling of translated PIDs inside nested PID namespaces without failing due to host-level `/proc` unreadability.

## Task details

Refer to task `id:t133` in `todo.md`.

### 1. Retrieve pidfds on Socket Connection
Instead of retrieving only the peer's raw PID via `SO_PEERCRED`, retrieve the peer's actual process descriptor (`pidfd`) at the socket layer. On modern Linux, this can be done via `SO_PEERPIDFD` or calling `pidfd_open` immediately upon accepting the connection.

### 2. Descriptor-Based Lineage Verification
Replace text-parsing of `/proc/[pid]/stat` in `launch_origin::process_ancestors` with kernel-level descriptor comparisons. By comparing the connecting process's `pidfd` against the launcher's registered `pidfd`, parentage can be verified securely and instantly, bypassing re-parenting races completely.

### 3. Rate-Limiting and DoS Defense
Implement connection-frequency throttling per UID at the socket admission layer to protect the display server from high-frequency reconnection attempts.

## Connections

- Links to [Namespace and Client Admission Security Gaps](../investigations/1pv291te-namespace-and-client-admission-security-gaps.md)
- Links to [Pnut Protection-Backend Evaluation](../../pnut-evaluation.md)
- Links to [Namespaces and Portals](../../namespaces-and-portals.md)

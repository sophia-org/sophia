---
id: fp9o6pdb
date: 2026-11-08
kind: concept
status: draft
tags: [concept, architecture, portability, sft]
---
# Portable Plan 9 interfaces with native frame mechanisms

## Direction and status

niltempus requested this direction on November 8: preserve Sophia's Plan 9
heritage while learning from Redox, and support Linux, individual BSDs, Redox
and future operating systems. This is a documentation direction, not a new
wire contract, supported-platform claim, or permission to implement a transport.

Keep named services and 9P control/discovery. Compose restricted resource views
and bind authenticated connections at setup; do not repeat discovery in the
frame loop. That is already the shape of SFT stream admission. A URL-like
address could be a convenient spelling of an existing scoped name, but no URI
grammar, parser, new discovery service or global routing broker is selected.
Names, URLs, file descriptors and fids are not interchangeable credentials.
Existing grant/custody owners still decide which operations are authorized.

Separate portable transaction meaning from platform mechanisms. Linux is the
current measured implementation; the SDK policy names FreeBSD as the next
native qualification target. OpenBSD, NetBSD and Redox require their own checks.
Do not claim a whole BSD family from a compile or a single backend. Build small
platform adapters when an actual port exposes the boundary, not a universal OS
framework in anticipation of one.

## Current path and proposed alternative

```text
CURRENT
scoped namespace -> authenticated 9P connection -> admitted SFT stream
  FrameCommit / events over 9P; registered pixels on the image path
  -> private pixel capture + validation -> composition -> atomic presentation

FUTURE TRANSPORT EXPERIMENT ONLY
same 9P setup, grants, resource registration and recovery
  -> negotiated bounded submission/completion mappings + notification
  -> same SFT meaning, private capture, validation and presentation

PLATFORM BOUNDARY (direction, not an implemented interface)
identity + resource transfer + memory + wakeups + display completion
  Linux mechanisms | per-BSD mechanisms | Redox mechanisms | future OS
```

The current SFT carrier remains 9P. The alternative would change only how
commands/completions travel, not add new admission or rendering semantics.
No runtime trait, public API, feature bit, codec or alternative transport is
allocated here. A future carrier needs a separate reviewed decision before it
can change the existing SFT ADR or contracts.

## Redox rings: candidate, not dependency

Redox's rings share one submission and one completion queue between userspace
processes. Their setup uses scheme handles; notification still needs platform
support. The reported storage speedup and separate Redox kernel improvements
are not Sophia CPU/frame predictions. Do not import its buffer allocator,
executor or spinning policy just to adopt bounded queues.

Before an experiment, identify remaining transport work on the current accepted
SFT lane. Model bounded queues, admission/completion reservation, fairness,
arm/recheck/wake races, disconnect, revocation and drainage of outstanding work.
Use deterministic negative controls for stale mappings, malformed indices and
lengths, full queues, lost-wakeup windows and hostile mutation. Validate and
privately capture an untrusted descriptor before acting; cooperative atomics
alone do not make a malicious producer safe.

Queue consumption is not semantic acknowledgment. Acceptance, presentation,
source release and final resource release remain distinct. Do not replace the
covering ACK or safe reuse rule with a shared head index. Client-mutable pixels
still require the existing private capture and validation. An OS lacking the
required isolation must refuse the mode or use a separately qualified safe
alternative, never silently weaken custody for zero-copy.

Only after those gates: compare one transport treatment with identical SFT
semantics, pixels, host work and pacing. Record combined and per-process
CPU/frame, syscalls, wakeups, copies, idle CPU, latency and cadence. Tracing stays
outside qualification timing. No added broker process, mandatory spin or name
lookup belongs in the hot path without measured justification.

## Connections and ownership

- [Redox/Orbital research](../investigations/sy7lhvb8-which-redox-ring-and-orbital-patterns-can-reduce-sophia-frame-overhead-safely.md) supplies pinned source observations, not adoption authority.
- [Namespace discovery](../../namespace-discovery.md) and [namespace recipes](../../namespaces-and-portals.md) own implemented naming and admission boundaries.
- [SFT design/status](../../native-frame-transactions.md) distinguishes the current carrier from this future direction; its wire contracts stay authoritative.
- [CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md#current-resumption-november-8) owns the next investigation. This concept does not block it.
- [SDK platform policy](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md#sdk-platform-policy) separates Linux qualification from future BSD support.
- [Task ledger](../../../todo.md) t256 owns broader portability planning; t344 owns current CPU reduction. No second execution queue lives here.

No backend implementation, runtime security audit or new measurement supports
this direction yet. Naming convenience alone does not reduce CPU/frame.

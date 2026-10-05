---
id: r8z5a3sk
date: 2026-10-04
kind: plan
tags: [plan, lock, rendering, performance]
---
# Negotiated DMA-BUF lock images with bounded capture and release

## Scope and exit

The published lock repair keeps the BGRA byte-upload contract. This candidate
would remove GPU readback and CPU transport of lock pixels by offering DMA-BUFs
on a separately negotiated descriptor channel. Sophia would still capture an
immutable renderer-owned snapshot. This is GPU-only pixel transport with a GPU
copy, not a promise of direct scanout or no copies anywhere.

Claude owns the proposed lock contract and consumer work. Design review remains
pending. Recording this task does not approve the nine open decisions or admit
implementation. Task state and priority live in [todo.md](../../../todo.md).

## t304

The draft is `t302-integration-01/DESIGN-lock-dmabuf-phase1-01.txt`, REVISION 2,
under `~/.local/state/sophia/development-evidence/`. It proposes a second admitted
SEQPACKET endpoint bound to the lock epoch, same-device format negotiation,
provider-completed rendering, bounded offers, and explicit source release.
Descriptors must never travel on the 9P stream.

Resolve these before changing the contract: capability versus protocol revision;
channel admission and revocation; Accepted, SourceReleased and Superseded
semantics; synchronization; capture failure and fallback; same-device scope;
renderer-image identity allocation; and shared store budget fairness. Keep
covering, authentication, input and unlock independent of provider progress.
The existing byte upload remains a required fallback.

The renderer integration (draft Step D) waits for t289's snapshot storage and
import reuse decision. It must use the accepted ownership and budget rules,
without changing capture/import code concurrently. The draft's other steps are
contract, C SDK, receive/validate with explicit refusal first, Kleis support,
and end-to-end qualification; their order is subject to the review.

## Evidence and acceptance

At matrix-fps 60, profiles 19 and 61 in `t302-integration-01` measured combined
Sophia and Kleis CPU at 128.9% and 70.5% of one core, with 57.1 and 58.3 fps per
output. These are whole-release comparisons from the completed repair, not an
estimate of this candidate's benefit. The remaining path still reads pixels
back, transports them in byte writes and uploads them for composition.

Acceptance needs descriptor and epoch ownership tests, malformed-offer controls,
bounded storage and GPU work, source-release safety through rejection and
teardown, byte fallback, and proof that stalled image work cannot delay unlock.
Require pixel checks on the real rendering path and a matched before/after lock
workload reporting both process CPU costs, frame delivery and input response.
QEMU correctness and real-device performance remain separate evidence.

## Connections

- [Completed lock repair](qrstyyjn-restore-lock-animation-and-input-responsiveness.md#closed-2026-10-05)
  establishes the byte-path baseline.
- [Sophia CPU plan](7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md)
  owns t289's shared renderer changes.
- [Renderer import boundary](../../renderer-import-boundary.md)
  defines capture and renderer-image residency.

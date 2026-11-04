---
id: dwm37bn6
date: 2026-11-04
kind: adr
status: proposed
tags: [adr, architecture, performance, shell]
---
# Submit native frames as bounded transactions over the existing shell transport

## Context

The refreshed changing-strip comparison found Sophia at 1032.5–1075 combined
µs/frame, Niri at 415–455 and XLibre at 180–220. Small local optimizations have
not closed that gap. These are measured lane-specific accounting bounds, not
universal compositor rankings or a cost breakdown of individual protocol steps.

On 2026-11-04 Mason authorized protocol design changes and TLA+ modeling first,
with atomic tear-free rendering and security preserved. The objective is to
beat the measured Niri lane by eliminating redundant frame coordination, while
keeping the existing image and transport foundations.

## Decision

Name the protocol **Sophia Frame Transactions (SFT)**. `FrameCommit` names the
per-frame submission, not the whole protocol. SFT also defines stream admission,
finite credits, private capture and completion. This name refers to the proposed
architecture; it does not imply enablement, a new renderer or a measured speedup.

Propose one bounded `FrameCommit` on the existing authenticated shell files
transport. **Keep 9P for non-pixel control, metadata, admission, intent and events.**
For the first CPU path, pixels stay in the registered owned ring/image path and
Session privately captures and validates them. No new transport is introduced.

Submit source identity, complete scene intent and predecessor-retirement intent
together. Session reserves finite credit, physical work and response capacity
before consuming input, then joins capture with candidate admission internally.
This removes the producer-side image-settlement prerequisite, per-frame
demand/permit exchange and separate predecessor-retirement request. Source
release remains independent of presentation and resource release.

The proposed contract, ASCII path, performance budget and remaining gates live in
[Sophia Frame Transactions (SFT)](../../native-frame-transactions.md). It is
the design document; current shell/image specifications remain authoritative
until an explicit negotiated amendment is accepted.

## Alternatives

Keep r10 and only overlap independent prerequisites: smaller, but retains the
per-frame demand/permit and retirement exchanges. It remains a fallback, not
evidence that the larger CPU target is reachable.

Replace 9P or move to Vulkan: neither follows from the measured control-path
question. Keep the existing transport and GL implementation while changing the
transaction boundary. Do not skip custody checks or validate shared mutable
pixels directly to obtain a lower number.

## Consequences

Expected benefit: fewer rendezvous and repeated owner service passes. The net
saving is unmeasured; mandatory capture/validation/rendering and new reservation
bookkeeping remain. The 350 µs combined target is an engineering budget, not a
forecast. Batching must not add unacceptable latency or weaken fairness.

Mason also requires deliberate Rust optimization during implementation:
ownership and bounded reuse, compile-time fixed work, vectorizable kernels and
measured SIMD specialization with safe feature dispatch/fallbacks. The
[implementation criteria](../../native-frame-transactions.md#rust-implementation-and-optimization-criteria)
define the correctness and measurement obligations. Compile-time optimization
does not replace live authority checks; compiler-profile experiments stay separate
from the architecture comparison.

Credit must cover accepted asynchronous copying. Cancel cannot settle before
the worker stops reading. Stream close stops admission/replenishment, while
accepted work can finish; facts changes and revocation invalidate authority.
Prepared precedes Presented and returns no credit. Old physical owners drain
across epoch replacement. Per-output atomic presentation is required; concurrent
monitors are not promised simultaneous flips.

The [TLA+ checkpoint](../../../validation/specula/native-frame-transaction/results.md)
passes its finite safety/progress configurations and seven negative controls.
Its [brief](../../../validation/specula/native-frame-transaction/modeling-brief.md)
explicitly excludes several implementation gates. In particular, wire admission
must reserve a nonfatal refusal before consuming FrameCommit; existing candidate
errors do not automatically have that reply path. General resource reclamation,
close/reopen, codec/SDK parity and runtime trace conformance still need work.

## Acceptance and connections

**Proposed, not adopted.** Mason's authorization permits design and modeling; it
does not establish a final wire grammar, implementation acceptance or speedup.
No revision, opcode or capability bit is allocated by this record.

The [continuous-frame CPU plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
and t289 own the next implementation/qualification work. The
[bounded-credit investigation](../investigations/tubnycgx-can-a-bounded-frame-stream-cut-per-frame-control-exchanges-without-weakening-custody.md)
supplies the earlier W3 reasoning; the new composed path is not that checkpoint
already proved end to end. [Session composition ownership](adr0002-session-owns-desktop-composition.md)
and the [accepted 9P direction](1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
remain unchanged. [Shell files](../../sophia-shell-files.md) and the
[image channel](../../sophia-image-channel.md) retain their current contracts.

Evidence: `~/.local/state/sophia-native-frame-model-01`, SHA256SUMS
`5b212d7c05e6526e44afbdde75b908df77f6a5b53508d2af0cf6bb29e74d094c`;
matched reference `~/.local/state/sophia-igpu-reference-result-01`, SHA256SUMS
`2b1d8658c677d414e1007230ec79c131aa77839e5ae23cab9d0a2d02fc077886`.

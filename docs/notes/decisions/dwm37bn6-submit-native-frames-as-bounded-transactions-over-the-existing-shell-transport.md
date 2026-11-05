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

The [independent SDK codec checkpoint](../../native-frame-transactions.md#independent-sdk-codecs)
now imports signed C and Rust implementations of the seven r13 records and
submission receipts. Sixteen golden vectors and 12,056 mutation cases agree;
Prepared-credit negative controls fail independently in both implementations.
This closes passive codec parity only. Host wire admission, owning clients,
real Lom/reference producers, implementation traces and native qualification
remain open; no stream is exposed or enabled by importing the codecs.

**Proposed, not adopted.** Mason's authorization permits design and modeling; it
does not establish a final wire grammar, implementation acceptance or speedup.
The [separate allocation ledger](../../../protocol/sophia-shell-allocations.md)
now assigns r13 / bit 18 and the initial SFT record kinds for default-off
integration; this is not enablement or acceptance.

The default-off runtime store now accepts complete scene intent with optional
owned-ring copying and predecessor retirement, reserving before input transfer
and finishing with fresh owner context. Private-image and retirement ownership
join at terminal publication. This is device-free implementation progress; the
wire/SDK/Lom integration, runtime traces and native qualification remain open.
The [current implementation gates](../../native-frame-transactions.md#complete-record-intake)
and `~/.local/state/sophia-frame-complete-intake-01` record its scope.

The [native CPU profile entry](../../native-frame-transactions.md#native-cpu-profile-admission)
now fixes one outstanding credit and rejects non-two-slot copying or GPU-backed
scene/predecessor resources before reserving work. Legacy composition remains
available; producer drain/fallback and the wire opener are still unimplemented.
Evidence: `~/.local/state/sophia-frame-native-profile-01`.

The [wire contract](../../sophia-frame-transactions-wire.md) reuses neutral
Candidate, D4C1 Offer and CandidateOutcome bytes inside bounded SFT records.
The [contract checkpoint](../../native-frame-transactions.md#wire-contract-checkpoint)
pins its layouts and literal vectors without exposing an opener or changing
SDK pins. C/Rust implementation parity and production admission remain required.

The [file-custody checkpoint](../../native-frame-transactions.md#file-custody-and-owner-admission)
preserves the distinction between submission custody and owner admission:
whole SFT requests and events retain their identity through the export, Prepared
returns no credit, and Open/Close refusal room is reserved before owner admission.
The [response-custody path](../../native-frame-transactions.md#response-custody)
now transfers whole responses into the shared FIFO under existing capacity checks;
Prepared returns no credit, and terminal transfer commits the owner's current
deadline and resource settlement once. Production negotiation still cannot expose
r13. The [Open/Close owner entry point](../../native-frame-transactions.md#open-and-close-owner-entry-point)
now reserves before taking real file input, refuses absent owner context without
spending the stream identity, and leaves an earlier FrameCommit ahead of Close.
The [complete-frame source handoff](../../native-frame-transactions.md#complete-frame-source-handoff)
now moves whole scenes into the existing copy owner, with provisional source
response reservations and unchanged status-before-release ordering. The
[owner completion path](../../native-frame-transactions.md#fresh-context-finishing-and-timed-completion)
now finishes copied scenes against fresh context and routes timed native completions
through the shared FIFO, preserving full-budget retry and exact retired-grant
settlement. The [owning SDK import](../../native-frame-transactions.md#owning-sdk-import)
now brings paired C/Rust stream owners and atomic copied-source queue admission
into Sophia's verified snapshots. A real Rust client/file-owner test covers a
retained frame's credit lifecycle with supplied negotiation and completion.
Production opt-in negotiation, independent C-to-host copied-source interoperability,
real producers, runtime traces and native qualification remain required; these
device-free checks establish no speedup.

The [retained-scene fallback seam](../../native-frame-transactions.md#retained-scene-fallback-dispatch)
now resumes demand/permit candidates after SFT drains on the same connection,
preserving accepted resources and display leases. Legacy candidates cannot spend
an open stream's credit. The paired SDKs now also bridge a worker-filled image
to ordinary Offer only after frame and source drain. The host independently
refuses legacy source admission while the stream remains open or draining.
The imported Rust SDK test now traverses the real file server, image FD channel
and copy worker, verifies two distinct immutable pixel payloads, and presents
the second through fallback without reconnecting or dropping the first display
lease. Authentication verdicts and native completions are supplied. The actual
producers were unjoined at that checkpoint; it was not the whole fallback or native gate.
Evidence: `~/.local/state/sophia-frame-image-sdk-import-01`.

The [initial-image and Lom checkpoint](../../native-frame-transactions.md#initial-images-and-lom-producer-checkpoint)
adds paired explicit pre-Open uploads (Rust `b483e1ee`, C `0df720b0`) and tests
establishment, retained/copied SFT scenes and fallback against the real host
owner with supplied negotiation/authentication/completion. Lom `332c9c4a` now
implements default-off whole-frame admission and exact drain/fallback while
preserving regions, source/resource custody and ordered input. Its real CPU
worker uses a scripted owner in five device-free controls; that does not prove
Lom-to-host/native interoperability. The ordinary CLI remains r10. Reference
producer, production opt-in, trace validation and native qualification are open.
Evidence: `~/.local/state/sophia-frame-initial-image-import-01` and
`~/.local/state/sophia-lom-frame-producer-01`.

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

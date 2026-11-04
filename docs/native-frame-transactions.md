# Bounded native frame transactions

**Status: proposed architecture, 2026-11-04.** The operator authorized protocol
design changes and TLA+ modeling to reduce native CPU/frame while retaining
security and atomic tear-free rendering. This is not a shipped wire contract,
protocol allocation, implementation acceptance or measured speedup. See
[ADR dwm37bn6](notes/decisions/dwm37bn6-submit-native-frames-as-bounded-transactions-over-the-existing-shell-transport.md).
The [shell files contract](sophia-shell-files.md),
[image channel](sophia-image-channel.md) and
[owned-ring contract](experimental-owned-ring.md) still define current behavior.

## Keep 9P; change the frame transaction

### First implementation profile

Mason selected the bounded fast path: one logical output and one outstanding
frame per stream, zero or one new CPU image plus retained layers in a complete
scene, at most one predecessor-retirement intent, and the existing two-slot ring.
Multi-region updates requiring several new images, GPU images and other
unsupported modes continue through the existing path. No scene is flattened or
feature disabled to make it eligible. If an update stops fitting, drain and close
the stream before falling back, preserving current display and live resources;
keep that output binding on the existing path to avoid repeated mode switching.
Reconsider eligibility on a new binding. Both real Lom and the reference peer
must use this contract; benchmark-only implementation is insufficient.

### Transport

**9P remains the control plane.** Admission, grants, metadata, frame intent,
acknowledgements and lifecycle events continue through the existing authenticated
shell files transport. `FrameCommit` is a proposed record on that transport, not
a new socket protocol or a replacement for 9P. Other desktop roles retain their
existing interfaces.

For the initial CPU path, pixels remain in the registered owned ring/image path.
Session captures them into private storage and validates the captured bytes
before rendering. `FrameCommit` references the source identity, slot and version;
it does not inline a frame's pixels into the 9P control record. Existing byte
upload and other image paths are not removed by this proposal. Registration and
descriptor custody keep the image channel's rules.

The change is to submit complete frame intent once and let Session coordinate
its internal dependencies. The producer no longer needs to wait for image
settlement before expressing candidate intent, negotiate demand/permit for each
frame, or issue a separate predecessor-retirement request.

```text
SETUP / CONTEXT CHANGE                         EXISTING TRANSPORT
  producer <---- grants, facts, registration ----> Session over 9P/image channel
       proposed stream binding + finite credits + reserved close response

PER FRAME
  PRODUCER                     SESSION                            OUTPUT
  fill registered pixel slot
       |
       +-- FrameCommit (9P) --> check authority; reserve bounded work
           source identity     credit + copy/worker + response capacity
           complete intent                 |
           predecessor intent  private copy + pixel validation
                                           |
       <-- SlotReleased (9P) -- worker has finished reading the source
       source can be reused                |
                               recheck binding; whole-scene handoff
       <-- Prepared (9P) ------ renderer owns the admitted candidate
                               prepare all required head compositions
                               existing GL cache/damage/render --> submit
                                                                  fence/flip
                               exact native completion <----------+
       <-- Presented + credit  terminal transfer; commit retire intent
       <-- ResourceReleased    after last lease and release publication
```

Coalesce work and responses that are already ready. Do not delay source release
until presentation simply to produce a single reply. Several events and writes
may remain; fewer records do not imply a measured number of eliminated syscalls.
The owner should service ready obligations in bounded batches. Descriptor-less
deadlines and fallbacks remain until a real wake/deadline covers each obligation.

## Authority, ownership and atomic presentation

- Bind connection/grant, output, allocation, facts, stream, ring, slot and
  generation identities. Facts include mode, scale and formats. A context change
  invalidates admission; stable metadata is not cached authorization.
- Reserve credit, copy/worker capacity, response capacity and applicable resource
  budgets before consuming input. Insufficient response capacity leaves the input
  pending. Legitimate exhausted-credit and close/expiry races need bounded,
  nonfatal refusals; invalid authority still fails closed.
- Keep accepted asynchronous copies charged. Cancellation cannot release a
  worker's read ticket or return its credit before handoff. Epoch replacement
  cannot erase old physical work or move its completion into the new grant.
- Validate the private captured bytes. Producer-controlled generation words do
  not authenticate hostile pixels or prove a coherent producer snapshot.
- Admit complete scene intent or none. Fresh checks apply after capture and at
  handoff/submission. Keep the current display if admission fails. Cancellation
  stops at renderer handoff; native owners already submitted must drain.
- Close stops new admission and replenishment; it does not revoke authority from
  already accepted work. Facts changes and revocation do invalidate that work's
  authority to make new submissions. Prepared never returns credit.
- Distinguish source-slot release, terminal credit return, accepted-resource
  release and native-buffer retirement. Predecessor retirement commits only with
  exact Presented transfer; reclamation still waits for the last relevant lease.

Atomic and tear-free means a complete validated composition replaces the visible
frame at the native completion boundary **per physical output**. Prepare-all does
not make multiple monitors flip simultaneously. Current primary-head logical
presentation and lagging-head ownership must remain explicit; cross-head
simultaneity would require another backend contract.

## Why this should be faster, and what is unknown

The intended saving is structural: fewer producer/owner rendezvous, fewer
per-frame control records, and fewer repeated dispatch passes around the same
pixel operation. Private capture, validation, upload, GL composition and native
completion still cost CPU. New reservation bookkeeping and queueing can consume
part of the saving. Longer batching can harm latency. Neither the protocol shape
nor TLA+ can determine the net cost.

The fresh matched changing-strip reference measured these combined client/server
CPU accounting bounds, in microseconds per completed frame:

| Lane | Combined CPU/frame |
|---|---:|
| Sophia | 1032.5–1075 |
| Niri | 415–455 |
| XLibre | 180–220 |

These are accounting bounds, not confidence intervals. The workload is a changing
1920×24 CPU strip on the same iGPU at 1920×1080/60 Hz, with 2000 counted intervals
per arm. XLibre had no separate compositor; the external servers also opened the
other GPU, while their selected rendering/output was qualified on the iGPU.
The figures price the measured processes, not the whole machine. They do not
establish a universal comparison with Smithay applications or other workloads.

**350 µs combined CPU/frame is the engineering target**, initially budgeted as
280 Session + 70 producer. It is not a prediction; it requires roughly two thirds
less CPU than today's mean. Qualification must put the candidate's upper bound
below a fresh matched Niri lower bound, with unchanged pixels, security, cadence
and an explicitly accepted latency comparison. XLibre's interval is a separate
stretch target. Use one outstanding frame first; do not buy throughput by adding
buffering latency. Qualify retained multilayer content separately.

## Rust implementation and optimization criteria

Mason requested deliberate use of Rust's performance facilities on 2026-11-04.
Apply them to the actual frame path from the first implementation, while retaining
the measured, bounded design:

- **Ownership and layout:** move frame payloads and reservation tokens instead
  of cloning them; borrow slices for reads; keep hot metadata compact and
  contiguous. Reuse bounded buffers after their leases end. Avoid new per-frame
  allocations, reference-count traffic and synchronization where ownership can
  eliminate them. Record unavoidable operations instead of assuming an API is
  allocation-free. Buffer reuse must preserve initialization, short-read handling
  and immutable accepted pixels.
- **Compile-time work:** use `const`, `const fn`, fixed tables and, where useful,
  const generics for truly fixed layouts, sizes and bounded algorithm choices.
  Encode ownership/state distinctions in types where that simplifies the runtime
  path. Keep negotiated limits and untrusted input checks at runtime. Generation,
  authority and revocation checks cannot be compiled away or cached as constants.
- **Vectorization:** first write simple contiguous loops the compiler can
  vectorize and reuse existing optimized kernels/library routines. Inspect the
  optimized code for measured fill, copy, scan and conversion hotspots. Add
  explicit SIMD only where it demonstrates a useful improvement beyond that
  baseline; memory bandwidth or extra passes may dominate instead.
- **Portable specialization:** use features supported by the pinned toolchain.
  Architecture-specific kernels need correct feature detection/dispatch when
  the deployment baseline does not guarantee the instructions, plus a tested
  fallback. Cover short inputs, unaligned data, vector tails, overflow and invalid
  dimensions; compare outputs and refusals against the reference implementation.
  Any `unsafe` needs a small documented safety boundary and appropriate executable
  checks. SIMD must preserve validation results and pixel-format semantics.
- **Compiler choices:** prefer concrete hot-path types/static dispatch where
  practical. Check code size and instruction-cache effects before adding
  specialization or forced inlining. Treat LTO, codegen-unit changes, PGO and
  CPU-target changes as separately controlled experiments. Keep build settings
  identical between architecture-control and candidate arms unless a build-setting
  change is the declared treatment; do not assume a machine-local build is portable.
- **Evidence:** check allocations/copies and optimized kernel code, run focused
  correctness/performance tests, then qualify combined native CPU/frame, cadence
  and latency. A faster inner loop or fewer instructions alone is not acceptance
  of the frame architecture. No additional profiling campaign is needed for a
  branch that is not on the measured path.

These are implementation criteria, not a requirement to add SIMD, generics or
compiler flags everywhere. Remove unnecessary work first; specialize the remaining
hot loops where the evidence supports it.

## Design evidence and implementation gates

The [modeling brief](../validation/specula/native-frame-transaction/modeling-brief.md)
defines the finite scope; [results](../validation/specula/native-frame-transaction/results.md)
record three safety configurations, one fair-progress configuration and seven
guard-removal counterexamples. The model checks bounded lifecycle properties;
it does not prove CPU savings, a correct implementation, unbounded operation,
general resource reclamation or physical GL/DRM behavior.

Before implementation enablement, finish the input/refusal reservation and
resource-lifetime proof seams, including close/drain/reopen, fragmented input,
byte limits, exact callback identities and predecessor-intent conflicts. Reuse
the W3 credit and W2 retirement foundations rather than create duplicate engines;
their composed FrameCommit path is new and unproved. Allocate grammar/revision
only after that contract is complete. Keep C/Rust parity and the generic path.
Integrate the real Lom producer as well as the reference peer. Runtime traces,
adversarial tests, then a matched native comparison are separate gates under the
[continuous-frame CPU plan](notes/plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md).

Evidence on crunch:

- `~/.local/state/sophia-igpu-reference-result-01`, SHA256SUMS
  `2b1d8658c677d414e1007230ec79c131aa77839e5ae23cab9d0a2d02fc077886`.
- `~/.local/state/sophia-native-frame-model-01`, SHA256SUMS
  `5b212d7c05e6526e44afbdde75b908df77f6a5b53508d2af0cf6bb29e74d094c`.

The model is based on current source `8359816f0`, experimental W3 checkpoint
`f639d21d2` and W2 contract `892bd1436`. Those separate checkpoints are not a
claim that W2/W3 are integrated on the current source.

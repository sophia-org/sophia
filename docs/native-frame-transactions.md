# Sophia Frame Transactions (SFT)

**Sophia Frame Transactions (SFT)** names the bounded native frame protocol.
`FrameCommit` is its per-frame submission: complete scene intent, an optional
new CPU image and optional predecessor retirement. Stream admission, credits,
capture and completion make up the rest of SFT's contract. SFT uses the existing
9P control transport, registered pixel path and native renderer; it is not a new
transport or rendering API. The name makes no performance claim.

**Status: default-off implementation, 2026-11-04.** The operator authorized protocol
design changes and TLA+ modeling to reduce native CPU/frame while retaining
security and atomic tear-free rendering. The initial wire contract is allocated
for integration; paired C/Rust owning clients and their source-admission joins
are imported, while production exposure, real producers and qualification
remain incomplete. This
is not implementation acceptance or a measured speedup. See
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
shell files transport. `FrameCommit` is an allocated record on that transport, not
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

The first Rust foundation reuses the reviewed W3 `FrameCredits` ledger from
`33ff35990`, behind runtime-only `experimental-native-frame-transaction`.
Its credit algorithm is unchanged; additional tests check each ledger key field,
owner refusal of reopening before external drain, and old completion replay.
The native store opener fixes its allowance at one credit. The ledger's existing
two-credit capacity does not authorize the native adapter to issue two.

This is not a FrameCommit implementation or an enablement switch: no wire
opener, Session/CLI flag, C/Rust codec or producer calls it. Ring/slot/callback
identity, copy/resource reservations and typed wire refusals must be joined to
it before the adapter can be exposed. Passing the ledger tests alone does not
discharge those gates.

### Store adapter checkpoint

The default-off store adapter now reuses W3's candidate lifecycle and W2's
retirement ownership on the current resource store. Existing pixel versions,
GL cache/damage behavior and the legacy wire path remain in place. The W2
retirement payload is an internal Rust type here, not a new wire allocation.

Terminal queueing does not replenish credit. `transfer_frame_event_at` uses the
owner's current time and returns credit only after its publisher successfully
transfers the complete terminal; that same transfer commits any predecessor
intent. Backpressure preserves both obligations. Ordinary outcome readers cannot
silently consume a terminal that carries stream credit. Prepared remains a
nonterminal and returns none. This intentionally tightens the older W3 adapter,
which settled the ledger while queueing an outcome.

`admit_frame_begin` is the store's typed input boundary. With no available reply
slot it leaves input and identity untouched. With a slot, exhausted credit or a
closed/stale stream produces a bounded request refusal, distinct from an accepted
candidate's terminal; it never fabricates credit or retirement. Valid Budget
refusals spend the candidate identity, while Stale refusals do not. Accepted
Chunk/End continuations use their existing reservations even at a full budget.

This checkpoint does **not** establish transport admission. Its caller must
provide connection-wide record and encoded-byte capacity, consume exactly the
peeked input and discard its trailing parts on refusal. Actual FrameCommit
wire admission, complete-record decoding, codec, SDKs and producer integration
remain unwired. No stream opener is exposed by
Session or the wire. Store tests do not substitute for composed-path tests or
runtime trace validation.

### Copy-worker join checkpoint

`OwnedCopyImageJobs::reserve_frame_copy` joins frame credit and response reservations
to the existing owned-ring resource/worker reservation before consuming the
peeked header. The caller must also hold the source-release journal reservation
and freshly check custody. A copy-bearing admission needs four additional
control records (Prepared, terminal, resource status and eventual resource
release), besides existing lifetime and journal obligations. Refusal leaves
frame credit available and rolls back an unused worker slot. Stale stream/source
tuples fail before spending the frame identity; an invalid resource reference
under a valid frame identity is Malformed and spends that identity.

The candidate retains an exact job binding during copying. Expiry marks it
stopped but cannot queue its terminal or return credit; End cannot admit the
scene while that binding remains. The real job settlement releases the binding
only after resource reconciliation. Cancellation keeps its original cause;
closure suppresses replenishment. Wrong job/grant completions refuse without
discarding the settlement, and disconnected completions never target a new
grant. Existing ring/slot identity checks and private-byte validation are reused.
This adds fixed metadata to the existing four job slots, not a second queue,
thread or per-frame allocation.

This header-only seam remains below the wire boundary. The complete-record path
below adds scene intake and predecessor ownership across copying. The real
codec/opener and C/Rust/Lom integration remain required before enablement. The
wire opener must call the fixed one-credit native entry point below; the reused
general ledger still supports two for its existing tests.

### Private frame-image ownership

A joined copy now reserves its destination for the exact grant, output and
candidate generation. Resource acceptance alone does not make it available to
ordinary leases, explicit retirement or another frame. Scene admission can lease
its own private image and ordinary retained images, and must include the new
image. Both End paths wait for the exact copy handoff without rejecting the
assembly merely because copying is still in progress.

`transfer_frame_event_at` owns resource settlement as well as credit settlement.
Only a successful Presented transfer promotes the private image to an ordinary
retained resource and commits the predecessor intent. A refused publication
changes neither. A rejected, expired or superseded frame abandons its private
image; the original resident/backing charge remains until the last renderer
lease ends and ResourceReleased is published. Cleanup does not require new
retiring headroom. Revocation drains old storage without delivering into a new
grant. These are fixed fields in the existing resource owners, not another pool.

Device-free tests exercise real copying, blocked publication, exact frame
identity, omitted-image refusal, expiry, failed rendering with a full retiring
budget, combined promotion/retirement and disconnect. This closes the private
image cleanup gap. No wire opener or native claim is introduced.
Evidence: `~/.local/state/sophia-frame-image-ownership-01`.

### Complete-record intake

The internal `ContentFrameCommit` carries the complete existing candidate value,
zero or one owned-ring image request and an optional predecessor intent.
`OwnedCopyImageJobs::admit_frame_commit` takes the decoder's owned input only
after the existing credit/response, predecessor and optional resource/worker
reservations succeed. Zero reply capacity leaves the input intact; recoverable
refusal consumes it with a reserved typed response. Foreign source ownership
refuses without consumption. Worker/resource refusal drops the temporary retire
token and leaves the predecessor's existing owner intact.

Scene rows move into the existing assembly without cloning. The existing row
grammar, count limits and candidate byte ceiling apply before copying; the actual
wire decoder must additionally enforce its complete encoded-record limit.
The copied image must be referenced, and the scene cannot draw the predecessor
it retires. Subsequent chunks or a different End retirement intent cannot replace
accepted intent. Retained-only scenes need no ring and remain admissible when
the copy worker and all four job slots are full.

Copy handoff makes a complete scene ready for the owner's
`finish_frame_commit`, which performs existing fresh facts, allocation,
interaction and resource validation. It does not require a peer End. Validated
rows move into the candidate instead of the previous End-time vector clones.
Expiry drops the predecessor token immediately while retaining an active copy's
terminal debt until handoff. Close still permits accepted work to finish but
forbids replenishment. Presented transfer remains the joint promotion/retirement
point described above.

Nine device-free tests cover complete copying and retained-only scenes, input
backpressure, refusal rollback, missing/foreign source, stale tuples, conflicting
intents, row/byte bounds, expiry, close and fresh-facts rejection. Three removed
guards produce the expected failures. Evidence:
`~/.local/state/sophia-frame-complete-intake-01`.

This is a composed runtime-store path, not a shipped protocol. Connection-wide
encoded-byte admission, fragmentation/codec and negotiation, the wire
opener, owner-loop wiring, C/Rust SDKs, real Lom/reference producers, runtime
trace conformance and native qualification remain required. No performance
claim follows from moving vectors or passing these tests.

### Native CPU profile admission

`open_native_frame_stream` reuses the existing context, lifecycle and reply
reservation checks with exactly one credit; it takes no peer-selected allowance.
The general `open_frame_stream` and ledger remain available as lower-level
reusable mechanisms, not as native wire entry points. Prepared, native completion
and blocked terminal publication do not release that credit. Only successful
terminal transfer permits the next frame.

Complete input with a new image now requires a two-slot source. Before reserving
copy work or predecessor retirement, the store checks that every retained layer
and optional predecessor is an ordinary CPU-backed resource accessible to that
frame. It borrows resource metadata without acquiring extra leases. The image
captured for this frame is validated by the existing private-copy path, and scene
handoff checks CPU backing again through the frame lease boundary. Missing reply
room still leaves input pending. Unsupported ring geometry or GPU backing gets a
reserved Malformed refusal for this profile, without changing the existing
display, taking a source read ticket or holding predecessor retirement.

This does not make those resources invalid on the legacy path: tests compose the
same GPU-backed resource through the ordinary candidate API after SFT refusal.
Retained-only SFT input needs no ring and is independent of a registered ring's
geometry. The future producer must still drain/close and select legacy when an
update does not fit, retaining legacy selection for that output binding; this
checkpoint does not implement that switching policy or negotiation.

Four device-free tests cover native opening/current context, credit through
publication backpressure, ring geometry and GPU rejection at both intake and
handoff. The GPU fixture supplies retained storage, not proof of GPU execution.
The pre-change regression admitted a GPU layer; removing each of four eligibility
guards also fails its check. Evidence: `~/.local/state/sophia-frame-native-profile-01`.
No opcode, SDK pin, wire opener or native enablement changes here.

### Wire contract checkpoint

The [initial SFT wire contract](sophia-frame-transactions-wire.md) and
[KDL layouts](../protocol/sophia-frame-transactions-v1.kdl) now assign r13 / bit 18,
three submitted kinds and four event kinds through the allocation ledger.
FrameCommit embeds the existing neutral Candidate and optional complete D4C1
RingOffer, followed by an optional predecessor intent. FrameOutcome retains the
existing outcome body and adds its exact stream binding and optional credit
deadline. No existing record changes bytes and no new transport is introduced.

Sixteen literal vectors cover fixed control events, all four image/retirement
flag combinations, and terminal credit/no-credit outcomes. Contract checks pin
identifier disjointness, field extents, the existing 256-byte control budget,
and byte equality with the currently pinned candidate/offer/outcome codecs.
Legacy decoders continue to refuse every new kind. These are specification
checks, not implementation of the SFT codec or proof of fragmented transport
admission. Evidence: `~/.local/state/sophia-frame-wire-contract-01`.

Independent C/Rust codec/refusal parity is recorded below; host admission and
producer integration remain next. The SDK worktrees start from the exact prior
vendored revisions; no vendored snapshot is edited directly. Negotiation must explicitly
grant SFT and owned-ring authority and choose exactly r13. The new contract does
not grant draw, GPU or the ordinary W2 Candidate trailer.

### Independent SDK codecs

The verified imports now pin Rust `bf8dc0142f5ee305779c78916edc3a960db19cc0`
and C `1d4ab9db683c055914846d36dbaeb2f73fd21bd9`. Both implement the seven
explicit-r13 record shapes and typed submission receipts, without changing
client defaults. Import checks compare the SFT prose, KDL and shared vectors
against this repository, in addition to verifying the immutable signed source
snapshots. The SDKs were changed and signed independently before importing.

Sixteen exact vectors, 12,056 differential mutations, maximum-row cases and
independent negative controls establish passive codec agreement. Rust's full
SDK suite has 939 passing tests and strict Clippy passes; C's 75 test executables
and parser ASan/UBSan pass. Strict SDK rustdoc still reports seven pre-existing
link/markup warnings in unchanged files. Evidence:
`~/.local/state/sophia-frame-sdk-codecs-01`, SHA256SUMS
`ff5a8b8cb6b05fb705bf30511211e249585de49702af02a6d6bd15339215d5ed`.

These codecs establish neither authority nor admission. No wire opener, runtime
default, native launch or performance claim follows from this import.

### File custody and owner admission

The default-off store now reserves typed Open/Close refusals before consuming
control input. Prepared retains its SFT identity without returning frame credit;
only terminal publication can settle credit, private images and retirement.

The file export accepts whole SFT records only with explicit r13/bit-18 selection,
content and owned-ring capabilities, valid limits, and a CPU-only image channel.
It retains image revision 10/source kind 4. Fragmented input, granted byte limits,
submission retries, receipt backpressure and all request/event shapes are tested.
A submission receipt or its ACK transfers no stream credit and does not consume
the owner's queued input. Embedded offers are never separately admitted.

Production negotiation still cannot select r13. At the file-custody checkpoint,
complete-frame input still needed its registered-copy owner connection; that join
is described below, preserving fresh custody, source-slot release and
terminal-credit-retirement atomicity. Owning C/Rust
clients, real Lom/reference producers, runtime traces and native qualification
remain open. Evidence: `~/.local/state/sophia-frame-prepared-routing-01`,
`~/.local/state/sophia-frame-control-admission-01` and
`~/.local/state/sophia-frame-file-custody-01`.

### Response custody

SFT controls and outcomes now share the existing typed output FIFO and its
control-budget accounting. Each event is validated and charged before transfer;
backpressure leaves the exact store head and resource obligations intact.
Prepared carries stream identity without credit. Terminal transfer computes the
credit deadline from current owner time, then settles credit, private images and
predecessor retirement through the existing store callback. Queueing a terminal
in the store is not that transfer; later journal ACKs do not repeat it.

The existing transport `content_prepared` method selects this publisher only for
an SFT connection. Controls, Prepared and terminals retain one FIFO order, including
close-before-terminal cases. Golden event bytes/charges and full-budget transfer
are checked without native execution. Evidence:
`~/.local/state/sophia-frame-response-transfer-01`.

This response-custody checkpoint alone does not expose production r13 or connect
the registered-copy owner, timed native completion callers or owning clients.

### Open and Close owner entry point

Open/Close now have a real-file owner entry point. It services the existing wire,
reserves connection-wide response room before removing the exact queued request,
and publishes through the shared response FIFO. Zero room defers without consuming
input; one room can carry an Open refusal, while successful Open reserves its reply
and lifetime close. Close also reserves room for a refusal if expiry won the race.

Only fresh owner-supplied output context can admit Open. An absent context produces
a typed Stale refusal, while a foreign grant fails before consumption. Controls
cannot overtake an earlier whole FrameCommit and obey the existing per-service
limit. Real 9P tests cover receipt ACK, deferred input, retry without spending a
stream identity, expiry/Close ordering and journal responses. Evidence:
`~/.local/state/sophia-frame-control-wire-01`.

This entry point is not yet selected by production Session negotiation. The
fresh-context finishing and timed completion joins are described below; owning
clients and real producers remain subsequent gates.

### Complete-frame source handoff

The existing persistent copy owner now accepts a whole decoded FrameCommit from
file custody. Scene vectors move into the candidate store without cloning. A
retained-only frame needs no ring or worker job. A copy-bearing frame reserves
both ResourceStatus and SlotReleased journal records before taking its source
read ticket, then uses the same bounded worker and release path as an ordinary
owned-ring offer.

The source reservation is provisional until frame admission succeeds. Deferral
leaves the complete input queued; refusal returns both journal holds without
advancing the offer or slot generation. Stream/frame identity and frame credit
remain governed by the existing admission ledger. Exact source ownership errors
fail closed. No embedded offer is fabricated as a separate legacy request.

Device-free checks cover real file staging, a registered memfd and worker,
retained-only admission, response/worker backpressure, stale stream identity and
ResourceStatus before SlotReleased. The copied image remains private until the
existing terminal-transfer boundary. Evidence:
`~/.local/state/sophia-frame-source-reservation-01`.

This establishes the input/copy handoff. The following checkpoint connects scene
finishing and completion; production opt-in negotiation and owning clients/producers
remain separate gates.

### Fresh-context finishing and timed completion

The existing candidate service now finishes ready SFT scenes against the owner's
current output, allocation, facts and interaction context. Copy completion only
removes the read gate; it does not supply authority. Missing output context rejects
the ready scene without bypassing a pending copy. Open/Close and scene finishing
share the existing per-service bound; output backpressure preserves queued work.

Native completion callers now pass current owner time through the existing
transport facades. The owner samples the next service timestamp after draining
presentation results, so a completion cannot make the subsequent service timestamp
regress. Presented and renderer failure keep exact grant/generation checks and
reserve response capacity before changing submitted state. Terminal transfer uses
the shared FIFO to settle credit and resources once. Closed or clock-poisoned
streams can drain admitted work without returning credit; disconnected completions
settle only their retained epoch, never a successor's journal.

Device-free tests exercise a real 9P retained scene through Prepared and Presented,
changed/missing context, copy handoff, full-budget retry, completion before Prepared,
close/clock-fault draining and exact retired-grant routing. Guard-removal mutations
fail for stale-stream reconciliation and completion before capacity. Evidence:
`~/.local/state/sophia-frame-owner-completion-01`.

r13 remains unavailable from production negotiation. Owning clients, real producers,
implementation traces and native qualification are still required; this checkpoint
establishes no native behavior or performance improvement.

### Owning SDK import

Sophia now imports the signed Rust SDK revision
`2c12c66b09af9c73743b9bbec67402608e73702c` and C SDK revision
`1fe1b929a6c2e01e3c8598981a8fb7e2732dcf9c`. The immutable snapshots retain
their upstream commit/tree and contract checks. These revisions add explicit r13
transport, a one-credit stream owner and joined copied-source submission to the
existing clients; they introduce no new transport or default negotiation mode.

Both clients reserve a whole submission before publishing the ring generation
and spending frame credit. A pre-admission queue refusal leaves those identities
unspent. After publication, source custody and frame credit remain separate:
Prepared and terminal frame events cannot release a source slot, while matching
ResourceStatus and SlotReleased settle it. A definitive refusal can release only
an unadmitted source; conflicting admission evidence or uncertain transport
closure keeps it unavailable. The paired SDK evidence records the independent
tests, sanitizer checks and guard-removal failures:
`~/.local/state/sophia-frame-client-source-join-01`.

The host's real file server and owner lifecycle are exercised with the imported
Rust `FrameStream` for a retained scene through Open, Prepared, Presented and
Close. That test supplies negotiation and renderer completion; it does not prove
protected launch, copied-source interoperability, production selection or native
presentation. Host-import evidence is in
`~/.local/state/sophia-frame-owning-sdk-import-01`.

Real Lom/reference producers still must join these owners to their presentation
and resource lifetimes, including drain-before-fallback. Production r13 selection
remains unavailable. Runtime trace validation and matched native qualification
remain required; importing tested clients makes no performance claim.

### Retained-scene fallback dispatch

The r13 owner now services buffered legacy candidates after its SFT work, within
the same per-pass limit. A legacy Candidate cannot consume an open stream's
credit: it requires demand/permit mode for that output. Once Close and admitted
work drain, the existing demand/permit path can present retained resources on
the same connection. It does not reconnect, renegotiate or drop the old display
lease. The imported SDK/file-owner regression reproduces the former stall
(permit granted, candidate never serviced), then completes both presentations;
removing the open-stream guard makes the negative control fail.

The host also refuses a legacy owned-ring Offer while that output's stream is
open or still draining, before scheduling any pixel copy. It uses the existing
bounded ResourceStatus/SlotReleased refusal, so the source is not stranded.
A regression proves refusal before drain and normal copy admission afterwards.
Upstream Rust/C clients now provide an explicit same-binding fallback upload
operation which separately checks frame and source drain; the ordinary legacy
entry points remain blocked on r13. Neither the client nor host change reconnects,
drops retained resources or introduces another pixel copy.

Evidence: `~/.local/state/sophia-frame-image-fallback-01` for the paired SDK
bridge and `~/.local/state/sophia-frame-image-fallback-host-01` for the owner
admission guard. The worker-fill and drained-image APIs are now imported from signed Rust
`452887f` and C `32ea31a`. The real Rust SDK/file-owner regression submits a
worker-filled ring image through SFT, verifies the host's immutable copied bytes,
then closes/drains, uploads another image through legacy Offer and presents it
on the same r13 connection. Both display leases remain held. It uses real 9P,
image-channel FD transfer and the copy worker, with supplied authentication
verdicts and renderer completion. This does not qualify protected launch or
physical presentation. Import/interop evidence is
`~/.local/state/sophia-frame-image-sdk-import-01`.

At that checkpoint real producer fallback policy and C-to-host copied-frame
interoperability remained open. Production negotiation, runtime traces and native
qualification still remain open. Earlier retained dispatch evidence remains in
`~/.local/state/sophia-frame-retained-fallback-01`.

### Initial images and Lom producer checkpoint

The paired SDK imports now pin Rust `b483e1ee` and C `0df720b0`. Their explicit
initial-image API admits ordinary owned-ring Offers before the first stream
Open, without creating credit or a stream owner. After an owner has opened,
only the existing exact drain/fallback API permits ordinary image admission.
Both keep the connection and allocation binding; generic legacy image APIs
remain blocked on r13.

The real Rust SDK/file-owner test establishes three immutable images, then
submits a copied SFT image together with a retained placement. After Close and
source drain it uploads a different ordinary image and presents both placements
through legacy admission on that same connection. Host bytes and both display
leases are checked. This still supplies authentication verdicts, negotiation
and renderer completion; it is not a protected/native run.
Evidence: `~/.local/state/sophia-frame-initial-image-import-01`.

Lom's default-off producer is published on `protocol/native-frame-transaction`
at `332c9c4a`. It preserves the region planner: initial multi-upload establishment
uses ordinary admission, then eligible updates send one FrameCommit with zero
or one new image. Unsupported active updates close/drain and stay on the legacy
path for that binding. Source reuse, predecessor release and exact Presented
input meaning remain separate. Five real CPU-worker/socket tests cover copied,
retained and full-panel frames, fallback, explicit refusal, delayed releases
and input ordering; two removed guards fail their tests. The locked repository
gate passes against the published SDK. The peer in these tests is scripted;
Lom-to-Sophia interoperation is not established by composing the two test claims.
Evidence: `~/.local/state/sophia-lom-frame-producer-01`.

The ordinary Lom CLI still requests r10; host production r13 selection remains
unavailable. Reference-producer integration, real producer-to-host controls,
runtime trace validation and native qualification remain required. No measured
speedup or native atomic/tear-free acceptance follows from these checkpoints.

### Independent C client against the host

The pinned C SDK now runs the same copied-image lifecycle against Sophia's real
file server, image socket and copy worker. It compiles as a separate strict-C99
process and uses only its own public SDK APIs: authenticated attach, image
registration, three initial uploads, Open, one copied-plus-retained FrameCommit,
Prepared/Presented, Close, exact source release and ordinary demand/permit fallback
on the same connection. The shared owner checks all uploaded pixels and retains
both display leases. A deliberately changed copied payload fails that host check.
Prepared returns no credit; frame completion alone never makes the C client reuse
its source slot. The SDK snapshots and production code are unchanged.

This closes copied-source C-to-host interoperability for the supplied-authentication,
supplied-negotiation and supplied-renderer fixture. It does not qualify protected
launch, actual Lom-to-host operation, native presentation or CPU/frame. Evidence:
`~/.local/state/sophia-frame-c-host-interop-01`.

### Model and remaining integration gates

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
their composed FrameCommit path is new and unproved end to end. Implement the
allocated grammar with C/Rust parity and keep the generic path.
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

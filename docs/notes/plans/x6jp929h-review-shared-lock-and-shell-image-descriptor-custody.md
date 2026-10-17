---
id: x6jp929h
date: 2026-10-17
kind: plan
tags: [plan, shell, lock, rendering]
---
# Review shared lock and shell image descriptor custody

## Scope and exit

On October 17 niltempus directed the descriptor design review after the matched
B2/B3 pilot. This admits t337 planning. It does not accept a new wire contract,
promote t338/t339 implementation, change t331's gates or change the installed
desktop. Codex owns this shared design; consumer ownership is assigned when
implementation opens. Claude confirmed that no newer lock draft or active
consumer work conflicts with it.

The review starts from Sophia `924ed07f2`, the proposed
[descriptor ADR](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md),
and the October 4 revision-2 lock draft at
`~/.local/state/sophia/development-evidence/t302-integration-01/DESIGN-lock-dmabuf-phase1-01.txt`.
That draft is historical input, not the contract. The recommendations below
supersede its suggestions only if accepted.

The t337 exit is an accepted common ownership model, exact proposed record and
object layouts with golden cases for both roles, and a compatibility/SDK review.
Normative KDL, generated rows and production implementation follow acceptance;
this note deliberately does not allocate production opcodes. The first review
must resolve the lifecycle before making those layouts normative.

## Recommended answers to the nine lock questions

| Question | Recommendation |
| --- | --- |
| Capability or revision? | Keep the 9P dialect and existing byte records. Introduce revision-gated role capabilities for sealed memory and DMA-BUF, with new record kinds; never reinterpret a reserved byte in an old record. Propose shell role r9 and lock role r2, subject to the exact codec review below. |
| Channel admission? | A separate nonblocking Unix SEQPACKET endpoint, bound to the authenticated live 9P connection, immutable principal, launched assignee and a fresh channel generation. A path or an epoch value alone grants nothing. |
| Release and supersession? | SourceReleased is a separate event. Candidate Superseded never releases a source by itself. A superseded offer that has not reached the GPU may be cancelled; an in-flight capture must finish or reach a proven safe teardown before release. |
| Capture failure? | Use a bounded standalone renderer capture job. It publishes an immutable image before resource acceptance. A failed job rejects that resource and leaves the previous image or cover intact. Do not weaken X's existing FailFrame policy. |
| Synchronization? | First DMA-BUF phase accepts only producer-completed buffers, no sync_file. Provider-side completion may wait on its worker; no wait may block Session, covering, input or unlock. Import/capture remains asynchronous to those owners, and its completion must be proved. |
| Devices and formats? | Same composing device only, with an exact per-output generation and advertised format/modifier pairs. No presumed LINEAR support, cross-device bridge, resampling or implicit conversion. Unsupported cases use bytes. |
| Renderer identity? | An owner-tagged, generational allocation in the renderer-image registry, separate from X transaction identity. Do not assume the high bit is free: current forward and reverse helpers accept every u64. |
| Store share? | Enforce the existing global image ceiling plus per-role/per-epoch reservations. Reserve source, destination and overlap before admission; no eviction of live X or shell images to admit lock work. If a reservation cannot be made, retain the cover or previous image and refuse the offer. |
| Acceptance point? | Admitted reserves bounded work. Accepted means immutable validated content is available: a checked sealed mapping, or a completed renderer-owned capture. Presentation remains a later candidate outcome. SourceReleased and ResourceReleased have separate meanings. |

The standalone capture decision replaces the draft's flip-dependent acceptance
path. Requiring Accepted before a candidate while waiting for that candidate's
flip to finish capture would create a cycle. DMA-BUF work therefore remains
behind t289's renderer storage/ownership decision; the sealed-memory path need
not wait for a new GPU capture implementation.

## Proposed contract text for review

### Discovery, negotiation and connection custody

Add a read-only `image-channel` object with its own format version. It describes
supported source kinds, the endpoint basename, current connection epoch, channel
generation, message/FD limits and per-output formats generation. The endpoint is
resolved only within the admitted component directory. Keep the existing `api`
line byte-for-byte compatible; its `fd_transfer=none` continues to describe the
9P stream. The new object explicitly describes the separate channel.

New records require the corresponding negotiated capability and existing content
permission. DMA-BUF also requires the independently granted render device;
sealed memory does not grant or require GPU execution. Shell r9 must extend the
bar/launcher/dock profile matrix explicitly, preserving their existing minimum
requirements; a higher revision cannot turn one role into another. Lock r2
retains the rendering-only role. Old clients continue using old revisions and
byte records. Unknown or required-but-unavailable capabilities remain a refusal.
An application that permits byte fallback may select it after discovery; the
SDK must never silently remove a required capability. ENOENT means the optional
object is absent; access, staleness and malformed-object failures do not.

The channel's first packet is Hello, with no descriptors, carrying the protocol
version, role, 9P connection epoch and channel generation. The receiver checks
supervised peer credentials and liveness against the same launched assignee,
then joins them to the already authenticated 9P principal and current grants.
Recheck custody on admission of each offer. A legacy unauthenticated comparison
connection cannot acquire this authority. One channel is active per connection;
a second live channel is refused, never a replacement of the first. After Hello
is checked, a ChannelReady event on the 9P journal names the channel generation.
The client consumes that event before ImageBegin; otherwise the two sockets
could race during setup.
Unauthorized connection attempts are rejected without disabling another peer's
valid channel or spending its channel generation.

A 9P disconnect, epoch replacement, revocation or assignee death stops new offers
and closes the channel. A channel-only failure disables descriptor admission for
that epoch; the still-authorized 9P byte path may continue. Old received work is
drained or cancelled according to its actual ownership, not declared released
merely because the socket closed. A new 9P epoch gets a new channel generation
and cannot resume an old offer.
This deliberately makes recovery conservative: a transient failure of the
authorized channel disables descriptors for the rest of that 9P epoch. The SDK
uses bytes, where the caller permits fallback, and leaves a full 9P reconnect to
the application. ChannelClosed on the still-live journal names the generation;
ImageBegin also refuses with `image_channel_unavailable` while disabled. Reserve
one ChannelClosed event slot when admitting Hello. Offers admitted but unsent
are cancelled with a pre-use release; already submitted GPU work drains safely.

### Offers and the two-stream ordering barrier

A new `ImageBegin` control record uses the ordinary 9P transaction/submit/ACK
path. It names source kind, resource ID and generation, output/allocation and
topology generation, dimensions, format, declared bytes and a fresh offer ID.
Its acceptance as a transaction is not resource acceptance. The owner reserves
budget and journal space, then reports ImageAdmitted with the offer ID and exact
metadata. Only then may the producer send the descriptor packet. Wire order on
one socket says nothing about delivery order on the other socket.

Offer IDs must increase strictly within a channel generation. Reject ImageBegin
with an ID no greater than the highest admitted ID; advance that high-water mark
only on admission. Zero is reserved and wraparound requires a new connection.
Keep that mark and the bounded live-offer set. A packet above the mark is
unadmitted and faults the channel. An ID at or below the mark that is no longer
live is stale: drain and close its FDs without an event or channel fault. This
also discards skipped IDs; the mark cannot prove that every lower ID was admitted.
A second Source for a still-live offer faults the channel. This distinction
allows expiry to race with delivery without retaining an epoch-long ID history.
Both SDKs must allocate descriptor offer IDs under this rule. Existing lock
resource transaction IDs are caller-supplied, so increasing submission IDs alone
do not establish compatibility; the source check below records the distinction.

The descriptor packet repeats the admitted identity and metadata. There is at
most one successful packet per offer. A duplicate closes its received descriptors
and cannot replace the first source, advance custody or reset a deadline. Unknown,
expired or cross-epoch offers are refused. Late packets cannot attach to a reused
resource ID. A second Source for a live offer is a protocol fault; a retired ID
is discarded by the high-water rule above. Retrying after an uncertain send requires an authoritative outcome
or a new offer; it is never blind replay.

For the first version, the channel carries only Hello and image-source packets;
all acknowledgements, admitted-offer refusals, cancellation and releases use the
9P journal. A packet above the admission high-water mark is closed and counted
without a per-packet journal event; the first such protocol fault closes the descriptor
channel. A duplicate or late packet cannot generate another release event.
Each packet has a fixed little-endian header with total length, version, kind,
role, connection epoch, channel generation, offer ID and descriptor count. A
sealed source carries one FD, zero mapping offset, dimensions, tight row stride
and byte length. A DMA-BUF source carries one to four plane FDs, format/modifier
and checked offsets/strides; fence count and all reserved fields are zero.
The proposed offset tables and provisional kind numbers appear below. Golden
records must follow those tables, not Rust struct layout.

Receive with close-on-exec and bounded ancillary storage. Reject truncated data
or ancillary data, unexpected ancillary types, extra/missing descriptors and
noncanonical lengths. Close every received FD on every refusal path. Do not
recover a malformed packet by guessing which FD belongs to which offer. Reserve
terminal journal capacity before ImageAdmitted: two event slots per offer, for
final ResourceStatus and SourceReleased, in addition to ordinary admission and
transaction events and the channel's reserved ChannelClosed slot. If these
cannot be reserved, refuse new admission. Do not stop receiving merely because an offer expired: consume
and close late packets within the receive budget, or close the faulty channel
and let socket teardown release its queued rights. Expiry must not leave FDs
pinned in an unread backlog indefinitely. Bound both socket backlog and
per-dispatch packet/validation work so another role can make progress.

### Sealed memory first

The source is one sealable file of exactly the admitted tight BGRA8 byte length.
Require F_SEAL_WRITE, F_SEAL_GROW, F_SEAL_SHRINK and F_SEAL_SEAL before mapping.
F_SEAL_FUTURE_WRITE alone is insufficient. Validate seals and size before creating
a read-only mapping, use checked width/height/stride arithmetic and enforce the
existing premultiplied-pixel rules. Validate in bounded chunks, without a whole
image scan on the input/lock owner thread. The validation worker processes
64 KiB units under a wall-clock deadline; these units are not paced one per
Session pass. `max_packets_per_turn` bounds receive work, not validation progress.
The worker queue and retained mappings remain bounded by admitted offers.
A read-only mapping alone is not an
immutability proof. No extra copy is promised until the storage representation
actually supports a retained sealed mapping.

After validation, transfer the mapping into the resource's immutable backing and
report Accepted. Retire or revocation prevents new references; outstanding
renderer leases keep the mapping and its charge alive until released. Closing a
producer FD does not remove Sophia's mapping. SourceReleased means Sophia needs
no further producer action and holds its own sealed backing reference; it does
not mean the file has become writable. A sealed file is never a reusable mutable
render target. ResourceReleased marks the eventual end of resource custody.

The SDK creates a fresh sealed source per immutable resource, or uses a separate
mutable scratch buffer and copies into it. Both costs belong in the benchmark;
sealing must not be presented as a free reusable-buffer optimization. Measure
page faults, fresh-page zeroing and both processes' mapping/population work;
do not infer those costs or fault counts from image size alone.

### DMA-BUF completion and release

The first phase requires provider-completed rendering, same-device import and
an exact advertised format/modifier pair. A source remains producer-owned and
must not be modified while offered. Copying into Sophia-owned storage isolates
future composition from later producer writes; it does not authenticate pixels
or prevent a malicious producer racing the capture. Such a race must not change
geometry, ownership or memory bounds, and a resulting image cannot bypass pixel
format/premultiplication requirements.

The capture worker holds source descriptors and destination reservation until a
real GPU completion or proven safe rollback. It then publishes the immutable
renderer image and sends Accepted plus SourceReleased in journal order. The
standalone job must not require a candidate or a display flip to complete.
Timeout can reject presentation interest, but is not proof that GPU reads ended.
A timed-out GPU job retains a bounded quarantine charge until safe cleanup; once
that bound is reached, stop admitting descriptor work and keep fallback/input
alive. Do not busy-wait for completion or free backing still used by the GPU.

After a pre-import refusal, no GPU read exists and release can be immediate.
After submission, cancellation, supersession, topology change and revocation
still wait for safe source release, even though they forbid publication. If the
journal connection has died, no release event can be promised. The SDK reports
unknown source custody, suppresses replay and never reuses that mutable buffer;
it may drop its own reference, but a socket close is not a completion signal.

SourceReleased is recorded once for every admitted offer on a live journal,
including one cancelled or expired before a source arrived. A pre-use release
permanently fences that offer: a later packet is closed without reading pixels,
importing or generating another release. Maintain this fence with monotonic,
never-reused offer IDs and bounded live-offer state, rather than an unbounded
tombstone list. ResourceReleased
is separate and occurs only after all immutable resource consumers are gone.
A resource rejection alone never authorizes mutable source reuse.

### Budget, fairness and failure invariants

Keep shell and lock resource budgets separate. A shell component cannot borrow
lock capacity or use a descriptor to bypass the shell's resource-size limits.
Charge immutable sealed bytes across staging, resident and retiring ownership,
transferring rather than accidentally duplicating a charge when ownership moves.
Charge DMA-BUF source references, pending capture and owned destination overlap
until each is released. The global renderer table remains bounded; a role quota
is a subdivision, not additional capacity. Logical byte accounting is not a hard
quota on a GPU-granted client's VRAM or on opaque driver allocations.

Use at most the existing role transfer count for outstanding offers and no more
than four FDs per offer in the first phase. The role's existing transfer deadline
bounds descriptor arrival and validation; packet retries never renew it. Numeric
renderer reservations, capture deadlines and per-pass work budgets must be
published in the proposed limits object and justified against supported topology
before implementation. Zero available budget means refusal and byte fallback,
not eviction of another role's live image. Covering and unlock work never wait
for an image reservation, renderer job or provider acknowledgement.

## t337 delivery and review order

1. Review the nine recommendations and lifecycle above against both existing
   contracts. Resolve the renderer's safe-completion and bounded-worker seam with
   t289 before admitting DMA-BUF implementation. Keep the sealed path separate.
2. Specify exact layouts for discovery, limits, ImageBegin, ImageAdmitted,
   SourceReleased and descriptor packets, including refusal codes, event-reserve
   accounting and optional-discovery compatibility. Produce positive and negative
   golden records outside the normative KDL until the decision is accepted.
3. Review independent C/Rust APIs for owned/borrowed FD lifetime, send EAGAIN,
   ambiguous send, cancellation, terminal status, reconnect and idle wake. Keep
   low-level C clocks caller-owned. Record public C layout changes and rebuild
   requirements. Native-client tests belong with the clients, not Sophia.
4. On acceptance, update ADR qujwzrdz and both normative contracts, generate exact
   rows/vectors, and admit receive-and-refuse fixtures first. Promote implementation
   separately; t337 is not complete merely because this design note exists.

Planning is allowed before t252 and t304 close: making completed t304 a dependency
of the design that t304 needs would be circular. Lock renderer implementation
still depends on the shared design and t289; shell/lock release gates stay intact.
The user-selected planning work does not make t331 complete.

## Measurement and red controls

Keep the frozen byte/IPC pilot and its gates. Descriptor qualification is a new
comparison against the same byte path on identical content, dimensions, cadence,
output and hardware. Report producer plus Sophia CPU/frame, allocations/frame,
bytes transported, source-copy/seal/readback time, GPU copy/upload work, end-to-end
latency, input responsiveness, memory and FD high-water marks. CPU per uploaded
MiB alone is meaningless when pixel bytes no longer cross the socket. Use
balanced repeated trials; distinguish paired-run variance from counter bounds.
For real shell/lock clients include rendering and fallback costs, not just the
synthetic peer. Idle/wakeup and x13 power comparisons remain t336's measurement
scope; this pilot did not measure them.

Required controls include wrong principal/epoch/generation, duplicate and late
offers, short/extra FDs, ancillary truncation, unsealed or wrong-size memory,
existing writable mappings, overflow and malformed pixels, EAGAIN without replay,
terminal status during capture, topology replacement, GPU stall, exhausted
budgets/journal, replacement connection, and release exactly once. Assert no
cross-role resource alias and no lock/input delay. Run old/new peers and equivalent
C/Rust cases independently. Device-free fixtures establish ownership, not GPU
completion or real-device performance; those need separate attended evidence.

## Source checks and connections

- At pinned Rust SDK `01474a9` and C SDK `5246f96`, lock resource transaction
  identities come from the caller: Rust `upload::Upload::begin` retains the
  supplied `LockResourceBegin`, and C `sophia_lc_upload_begin` copies its supplied
  begin record. End and Cancel reuse that identity. Separately, Rust
  `submission::Submission::admit` and C `sophia_lc_internal_queue` allocate
  increasing outer submission IDs and refuse exhaustion. Neither allocator
  assigns the resource transaction identity. Descriptor APIs must allocate or
  enforce increasing offer IDs in both SDKs without changing legacy byte-upload
  callers; include repeated/decreasing IDs and exhaustion in their fixtures.
- Claude independently decoded all 32 draft vectors against the offset tables
  and found no remaining proposed-layout findings. This validates the draft
  bytes, not real SCM_RIGHTS, seals, SDK custody or renderer completion.
- `crates/sophia-runtime/src/lock_files/transport.rs::poll_accept` now has an
  authenticated path; the old draft's pidfd-only description is incomplete.
- `crates/sophia-backend-live/src/presentation.rs` maps every raw transaction ID
  directly to/from a renderer image. A high-bit partition needs a registry change.
- `crates/sophia-renderer-live/src/native_scanout/owned_mixed_export.rs` currently
  propagates capture failure; it is not a per-role nonfatal worker seam.
- [Renderer ownership](../../renderer-import-boundary.md#generational-renderer-image-residency)
  requires bounded immutable captured images and safe generation transitions.
- [Linux sealing semantics](https://man7.org/linux/man-pages/man2/F_GET_SEALS.2const.html):
  F_SEAL_WRITE cannot be installed while a shared writable mapping exists;
  F_SEAL_FUTURE_WRITE permits existing writable mappings and is not a substitute.
- [Unix ancillary data](https://man7.org/linux/man-pages/man7/unix.7.html)
  documents descriptor transfer and ancillary truncation; receiving a file
  reference is not an authorization or GPU-completion proof.
- [t304 lock plan](r8z5a3sk-negotiated-dma-buf-lock-images-with-bounded-capture-and-release.md).
- [Measured B2/B3 result](../investigations/2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md#b2b3-native-pilot--october-17).

## Proposed byte layout, version 1

This is a review draft, not an allocated production namespace. All integers are
unsigned little-endian; unknown versions, flags, reserved values and kinds are
refused. Proposed role capability bits are shell 13 (sealed) and 14 (DMA-BUF),
gated at r9, and lock 2 (sealed) and 3 (DMA-BUF), gated at r2. Existing role
header/API version 1 remains unchanged. Descriptor support is never inferred
from a higher role revision alone.

### Channel packet header

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u32 | total bytes: 40 for Hello, 200 for Source |
| 4 | u16 | channel version = 1 |
| 6 | u16 | kind: 1 Hello, 2 Source |
| 8 | u16 | role: 1 shell, 2 lock |
| 10 | u16 | descriptor count: 0 for Hello, 1–4 for Source |
| 12 | u32 | reserved = 0 |
| 16 | u64 | authenticated 9P connection epoch, nonzero |
| 24 | u64 | channel generation, nonzero |
| 32 | u64 | offer ID: 0 for Hello, nonzero for Source |

Hello is exactly the header. Source appends the 96-byte metadata below and four
16-byte plane rows. Every unused plane row is zero. The header descriptor count,
metadata descriptor count, plane count and actual SCM_RIGHTS count must all
agree; any mismatch is refused. No fence FD is permitted. FD order is the plane-row order,
and each used row's fd_index equals its zero-based row number. Repeated backing
objects are allowed only through separately received references that are each
counted and closed; they do not create extra resource budget.

### Common image metadata

Offsets in this table are relative to the metadata, not the channel header.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u64 | offer ID; equals the Source header ID |
| 8 | u64 | resource ID, nonzero |
| 16 | u64 | resource generation, nonzero |
| 24 | u64 | shell content-grant epoch or lock epoch at admission |
| 32 | u64 | output ID, nonzero |
| 40 | u64 | allocation ID, nonzero |
| 48 | u64 | topology generation, nonzero |
| 56 | u32 | source kind: 1 sealed memory, 2 DMA-BUF |
| 60 | u32 | source format: sealed = 1 (premultiplied BGRA8); DMA-BUF = advertised fourcc |
| 64 | u32 | width in pixels, nonzero |
| 68 | u32 | height in pixels, nonzero |
| 72 | u32 | sealed row stride = width × 4; DMA-BUF = 0 (per-plane rows supply strides) |
| 76 | u16 | plane count: sealed = 1, DMA-BUF = 1–4 |
| 78 | u16 | descriptor count = plane count |
| 80 | u64 | declared logical image bytes = checked width × height × 4 |
| 88 | u64 | modifier: sealed = 0; DMA-BUF = advertised modifier |

First-phase DMA-BUF formats are advertised 32-bit RGB formats with a defined
alpha/premultiplication rule; planar YUV and arbitrary conversions are excluded.
An alpha-bearing format cannot be advertised until validation of the captured
owned pixels proves the same premultiplication rule as the byte path. Validating
the mutable source before copying is insufficient. An opaque XRGB format can
avoid that scan only if its sampling semantics force alpha to one. Until the
alpha-validation seam is proved, transparent clients keep the byte or sealed
path; no client feature is disabled to obtain a faster result. A sampled pixel
check is insufficient. Alpha validation must cover the complete owned image,
for example with a GPU reduction and bounded result readback, and must not
reintroduce a full pixel readback. Until that exists, translucent bars and docks
are outside the DMA-BUF first phase. For XRGB, forcing alpha to one happens in
the capture job and belongs in its measured cost.
A modifier's allocation footprint may exceed logical image bytes. The renderer
must bound and reserve its actual owned allocation separately before capture;
logical size is not an estimate of driver VRAM use. Unknown allocation bounds
mean refusal, not an unchecked reservation.

A plane row is `offset:u64 @0`, `stride:u32 @8`, `fd_index:u16 @12`,
`reserved:u16 @14 = 0`. Sealed memory uses offset zero and the metadata stride;
its file length equals declared bytes exactly. DMA-BUF offsets and strides must
pass checked arithmetic and the advertised import validator. Metadata must match
the admitted record byte-for-byte. A topology change cancels a pending offer;
an already accepted resource keeps the existing role's lifetime rules, and any
later candidate must still satisfy current allocation/epoch checks.

### Proposed 9P records

Use the role's existing 32-byte record header and its normal transaction, submit,
journal and ACK rules. Provisional kind numbers below are for draft vectors only;
check the complete namespace again when making the KDL normative.

| Kind | Direction | Body | Meaning |
| --- | --- | --- | --- |
| 28672 (0x7000) ImageBegin | candidate | 96-byte metadata | Reserve one descriptor offer; no source FD exists yet. |
| 28673 (0x7001) ImageAdmitted | event | exact 96-byte metadata | Reservation and terminal journal space exist; source may now be sent. |
| 28674 (0x7002) SourceReleased | event | 32 bytes below | The admitted offer no longer needs producer cooperation, whether or not its source arrived. |
| 28675 (0x7003) ChannelReady | event | 16 bytes: channel generation u64 @0, granted source-kind mask u32 @8, reserved u32 @12 = 0 | Hello passed; ImageBegin may follow. |
| 28676 (0x7004) ChannelClosed | event | 16 bytes: channel generation u64 @0, reason u32 @8, reserved u32 @12 = 0 | Descriptor admission disabled; no source release is implied. |

Existing ResourceStatus reports accepted/rejected/cancelled with its existing
meaning; ResourceReleased remains the last-consumer event. For lock records,
the metadata offer ID is the resource transaction identity used by subsequent
ResourceCancel and status records. It is distinct from the outer submission ID.
A descriptor offer has no ResourceEnd: validation or capture determines its
completion. Sending ResourceEnd against it is refused without changing custody.
ResourceRetire applies only after immutable acceptance, as on the byte path.

SourceReleased body: offer ID `u64 @0`, resource ID `u64 @8`, resource generation
`u64 @16`, disposition `u32 @24`, reserved `u32 @28 = 0`. Dispositions are
1 retained immutable backing, 2 capture complete, 3 refused/expired before use,
4 cancelled before import, 5 drained after GPU submission and cancellation.
A live journal promises exactly one release for every admitted offer, including
one whose packet never arrived. Losing the journal leaves release unknown to
the producer. Disposition does not replace ResourceStatus or ResourceReleased.
ChannelClosed reasons are 1 peer disconnect, 2 protocol fault, 3 Hello timeout,
4 descriptor grant revoked. If 9P itself is gone, no ChannelClosed delivery is
promised. Closing the channel does not prove that a capture has finished.

The discovery file is bounded ASCII with a fixed first line
`sophia-image-channel version=1`, followed by one key/value per line: role,
connection_epoch, channel_generation, endpoint, source_kinds,
max_packet_bytes=200, max_fds=4, max_offers, max_packets_per_turn=1,
hello_timeout_ms=250, validation_timeout_ms=2000,
validation_chunk_bytes=65536, capture_interest_timeout_ms=500, and
formats_generation. Integers are unsigned decimal, source_kinds is a numeric
bitmask (bit 0 sealed, bit 1 DMA-BUF), and endpoint is a single basename with no
slash or dot component. Refuse duplicate, missing or unknown fields in version 1.
The separately versioned formats object publishes only this assignee's admitted
outputs/device, with at most 256 exact format/modifier rows per output.
The 250 ms Hello and 500 ms capture-interest values are provisional design
defaults, not qualified bounds; accept them only after loaded-startup and
supported-topology review. Exceeding a capture-interest deadline never asserts
GPU completion.

`max_offers` is min(4, the role's transfer-slot count). Those offers include
quarantined captures: a timeout never returns the slot until cleanup is safe.
Advertise DMA-BUF only when a renderer quota and safe-completion seam exist;
unavailable capacity is not hidden behind a promise to allocate later. Role
resource/live/retiring limits continue to apply. A separate immutable limits
object must publish per-role renderer entry/byte reservations before DMA-BUF is
advertised; its numeric grant is assigned from actual admitted topology and
remaining global capacity, not a new fixed global allowance.

### Draft golden cases

The first encode/decode review uses these canonical values: shell role 1,
connection epoch 1, channel generation 2, offer 3, resource 4, generation 5,
content epoch 6, output 7, allocation 8, topology 9, a 2 × 1 sealed BGRA image,
stride 8 and declared length 8. Hello has no ancillary data. The Source packet
has one sealed 8-byte FD and one used plane row. Equivalent lock cases change
role to 2 and interpret metadata epoch 6 as the lock epoch; no layout changes.

Required negative byte vectors independently alter total length, version, role,
epoch, generation, offer, source kind, reserved fields, plane/FD count and used
plane row. Ancillary fixtures separately test actual FD count/truncation and
seals; hex bytes alone cannot establish SCM_RIGHTS or kernel seal behavior.
Thirty-two draft hex vectors, including ChannelReady, ChannelClosed, malformed
Hello packets and sealed-layout arithmetic cases, are retained in
`~/.local/state/sophia/development-evidence/t337-design-20261017/draft-vectors.json`,
SHA-256 `d76d9b3f84854dabb04a223c522bd6001af2b85a0fc73b2987de16e1d6b6e71a`.
The metadata encoding agrees between contiguous packing and an independent
offset-by-offset construction, and packet lengths match these tables. This is
a layout check, not independent SDK conformance or actual ancillary/seal testing.
The C/Rust codec and custody fixtures remain required before implementation can
be adopted; the draft vectors allocate no normative opcodes.

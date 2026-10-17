# Shared shell and lock image descriptor channel — version 1

Accepted 2026-10-17 under [ADR qujwzrdz](notes/decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md),
after independent C and Rust lifecycle/custody review. This is the normative
ownership and layout contract for the separately negotiated image channel.
The [design and evidence](notes/plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
record the decisions, exact fixtures and remaining implementation gates.

**Implementation status:** production and both SDKs still advertise the existing
shell r1–r8 and lock r1 paths. Passive image codecs do not expose a live channel
or enable its records on those connections. t338 owns sealed-memory implementation;
t339/t304 own DMA-BUF after the renderer safe-completion seam. Contract acceptance
is not a runtime release or performance result. The existing 9P `api` line stays
unchanged: `fd_transfer=none` describes the 9P stream, not this separate socket.

The shell KDL's ResourceStatus ceiling includes r9 value 13. Explicit
revision-aware C and Rust codec entry points admit it only for Rejected at r9;
the independent Go oracle also checks the negotiated revision. Legacy entry
points and Sophia's existing emission path retain 12. All other reason fields
retain their bounds. Image-kind KDL rows describe passive codec support, not
negotiated production service; the exact tables here govern both.

## Namespace and reviewed vectors

The October 17 namespace audit at Sophia `eea44c4d2` found no collision: the
shell KDL has 56 unique kinds through 278; lock has 19 through 263. No role KDL
reserves 0x7000–0x7006. Shell capabilities 0–12 and lock capabilities 0–1 are
already occupied; this contract allocates shell 13/14 and lock 2/3. Capability
masks are per role. The parked overview branch's informal shell r9/bit-13 claim
is withdrawn; it must seek fresh allocation if revived.

The [54-case corpus](../protocol/golden/sophia-image-channel-v1.tsv) is preserved
byte-for-byte from Rust `8f7dc5d`, SHA-256
`b7bdc6723e1eceda94da0bbb13868571590c3d47cd0b7fc976c2393026da29d8`.
Its historical first-line label says proposed; acceptance changes its status,
not its bytes. It names role, actual descriptor count, accepted/refused outcome
and refusal category for the fixed sealed-only fixture. C `b2ca17d` independently
decodes the same cases from `src/tests/image_channel_draft_vectors.h`, SHA-256
`cc0f226b72d0310568d2c6a3acd56cd57772d10ddc6ac31a957056074e4d664d`.
The source evidence files are `draft-vectors.json`, SHA-256
`d76d9b3f84854dabb04a223c522bd6001af2b85a0fc73b2987de16e1d6b6e71a`,
and `draft-object-vectors-r2.json`, SHA-256
`8444e92f1f1db24165f2576aa02376a4f3f46825169cc4792fc517325771775b`,
under `~/.local/state/sophia/development-evidence/t337-design-20261017/`.
The later one-row/short-object control brings their combined 53 cases to 54.
Hex vectors prove no kernel custody; the independent lifecycle peers provide
the separate local SCM_RIGHTS and sealed-file evidence.

## Custody and lifecycle

### Discovery, negotiation and connection custody

Add a read-only `image-channel` object with its own format version. It describes
supported source kinds, the endpoint basename, current connection epoch, channel
generation, message/FD limits and per-output formats generation. The endpoint is
resolved only within the component endpoint directory containing its role's 9P
socket (the supervised lock-provider socket directory for lock). Keep the existing `api`
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
ImageBegin uses the ordinary transaction submit and ACK barrier. The ImageBegin candidate's Submitted
receipt names ImageBegin (0x7000), admitted only by the explicit shell r9 or
lock r2 receipt codec. Earlier revisions and legacy codec entry points keep
their existing candidate sets; no other image kind is a valid receipt target.
Unauthorized connection attempts are rejected without disabling another peer's
valid channel or spending its channel generation.

A 9P disconnect, epoch replacement, revocation or assignee death stops new offers
and closes the channel. A channel-only failure disables descriptor admission for
that epoch; the still-authorized 9P byte path may continue. Channel loss withdraws
interest in every live offer. Unsent or synchronously held sources release with
disposition 4; validation already in flight retains custody until its worker
returns, and submitted GPU work releases with disposition 5 only after proven
safe cleanup. Socket closure alone never proves that cleanup.
A new 9P epoch gets a new channel generation
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
The offset tables and allocated kind numbers appear below. Golden
records must follow those tables, not Rust struct layout.

Receive with close-on-exec and bounded ancillary storage. Reject truncated data
or ancillary data, unexpected ancillary types, extra/missing descriptors and
noncanonical lengths. These packet-shape errors, foreign packet identity and
metadata that differs from the live offer's admitted metadata fault the joined
channel with ChannelClosed reason 2. Zero offer IDs and disagreement between
header and metadata offer IDs are shape errors. Check packet shape before the
stale-ID drain; an expired ID cannot hide malformed framing or plane rows.
Close every received FD on every refusal path. Do not
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
existing premultiplied-pixel rules. Once a packet has valid shape and matches its
admission, missing seals, unsupported seal queries, wrong backing size or invalid
pixels refuse that offer with a rejected ResourceStatus and SourceReleased
disposition 3; they do not fault the channel. Missing or extra descriptors are
packet-shape errors, not this source-content refusal. Validate in bounded chunks,
without a whole image scan on the input/lock owner thread. The validation worker processes
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
On a live journal, a retired backing whose last lease has ended retains its slot
and byte charge until ResourceReleased fits. Journal ACK must retry that deferred
collection. A dead journal owes no event; safe cleanup still waits for leases.

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
published in the limits object and justified against supported topology
before implementation. Zero available budget means refusal and byte fallback,
not eviction of another role's live image. Covering and unlock work never wait
for an image reservation, renderer job or provider acknowledgement.

## Byte layout, version 1

This namespace is allocated for shell r9 and lock r2. All integers are
unsigned little-endian; unknown versions, flags, reserved values and kinds are
refused. Role capability bits are shell 13 (sealed) and 14 (DMA-BUF),
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

### 9P records

Use the role's existing 32-byte record header and its normal transaction, submit,
journal and ACK rules. The kind numbers below are shared by both role contracts; direction is explicit,
not inferred from the numeric range.

| Kind | Direction | Body | Meaning |
| --- | --- | --- | --- |
| 28672 (0x7000) ImageBegin | candidate | 96-byte metadata | Reserve one descriptor offer; no source FD exists yet. |
| 28673 (0x7001) ImageAdmitted | event | exact 96-byte metadata | Reservation and terminal journal space exist; source may now be sent. |
| 28674 (0x7002) SourceReleased | event | 32 bytes below | The admitted offer no longer needs producer cooperation, whether or not its source arrived. |
| 28675 (0x7003) ChannelReady | event | 16 bytes: channel generation u64 @0, granted source-kind mask u32 @8, reserved u32 @12 = 0 | Hello passed; ImageBegin may follow. |
| 28676 (0x7004) ChannelClosed | event | 16 bytes: channel generation u64 @0, reason u32 @8, reserved u32 @12 = 0 | Descriptor admission disabled; no source release is implied. |
| 28677 (0x7005) ImageLimits | object | 104 bytes below | Fixed descriptor limits for this connection. |
| 28678 (0x7006) ImageFormats | object | 32 bytes plus format rows below | Format snapshot tied to topology. |

Existing ResourceStatus reports accepted/rejected/cancelled with its existing
meaning; ResourceReleased remains the last-consumer event. For both shell and
lock records, the metadata offer ID is the resource transaction identity used
by subsequent ResourceCancel and status records. It is distinct from the outer
submission ID. Shell ResourceCancel names the current connection epoch and the
metadata admission epoch as its content-grant epoch; both roles name the exact
metadata resource ID and generation.
A descriptor offer has no ResourceEnd: validation or capture determines its
completion. Sending ResourceEnd against it is refused without changing custody.
ResourceRetire applies only after immutable acceptance, as on the byte path.

ResourceCancel may follow a submitted ImageBegin before ImageAdmitted arrives.
The owner processes that offer's ImageBegin before its cancellation. Locally,
the producer checks submission capacity before recording cancellation; a busy
refusal changes nothing. Once cancellation is recorded, it permanently prevents
that offer's unsent Source from being sent, even if ImageAdmitted arrives later
or the cancel submission is definitively refused. Once admitted, neither the
local mark nor ResourceStatus closes producer custody: the offer settles only
on SourceReleased, or becomes unknown on journal loss. A cancellation that wins
before acceptance reports Cancelled and releases according to actual source
custody. An acceptance that already won remains authoritative. Owner-initiated
Cancelled status follows the same release rules without requiring a local mark.
After a final status the producer refuses a new local offer cancellation;
accepted resources use ResourceRetire instead.

SourceReleased body: offer ID `u64 @0`, resource ID `u64 @8`, resource generation
`u64 @16`, disposition `u32 @24`, reserved `u32 @28 = 0`. Dispositions are
1 retained immutable backing, 2 capture complete, 3 refused/expired before use,
4 cancelled before import, 5 drained after GPU submission and withdrawal of
interest. Disposition 5 includes explicit cancellation, channel loss, expiry and
capture-interest timeout after submission; none permits release before proven
safe cleanup. A successful uncancelled capture uses disposition 2.
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
formats_generation and limits_generation. Integers are unsigned decimal, source_kinds is a numeric
bitmask (bit 0 sealed, bit 1 DMA-BUF), and endpoint is a single basename with no
slash or dot component. Refuse duplicate, missing or unknown fields in version 1.
The separately versioned formats object publishes only this assignee's admitted
outputs/device, with at most 256 exact format/modifier rows per output.
The 250 ms Hello and 500 ms capture-interest values are provisional design
defaults, not qualified bounds; accept them only after loaded-startup and
supported-topology review. Exceeding a capture-interest deadline never asserts
GPU completion.

Hello, validation and capture-interest timeouts, validation chunk bytes and
max packets per turn must be positive. Zero means neither an unlimited wait nor
disabled service; source availability is expressed by the kind mask and its
capacity fields. The limits object follows the same rule.

`max_offers` is min(4, the role's transfer-slot count). Those offers include
quarantined captures: a timeout never returns the slot until cleanup is safe.
Advertise DMA-BUF only when a renderer quota and safe-completion seam exist;
unavailable capacity is not hidden behind a promise to allocate later. Role
resource/live/retiring limits continue to apply. A separate immutable limits
object must publish per-role renderer entry/byte reservations before DMA-BUF is
advertised; its numeric grant is assigned from actual admitted topology and
remaining global capacity, not a new fixed global allowance.

### Discovery and companion objects

These are additional objects; the existing role objects keep their layouts.
Expose three read-only files in the authenticated role root: `image-channel`,
`image-limits` and `image-formats`. No file grants authority by being readable.
Legacy exports may omit all three; ENOENT on `image-channel` selects the byte
path only when the caller permits fallback. Once it exists, a missing companion,
malformed object or access error is a failure, not optional discovery.

`image-channel` is at most 2048 bytes, ASCII with LF line endings and a final LF.
The first line is exactly `sophia-image-channel version=1`. Remaining lines use
`key=value`, in this order, once each, with no blank lines, spaces or comments:

```
role=1
connection_epoch=1
channel_generation=2
endpoint=image.sock
source_kinds=1
max_packet_bytes=200
max_fds=4
max_offers=4
max_packets_per_turn=1
hello_timeout_ms=250
validation_timeout_ms=2000
validation_chunk_bytes=65536
capture_interest_timeout_ms=500
formats_generation=9
limits_generation=1
```

These values form the sealed-only shell fixture, not a universal capacity grant.
Lock changes role to 2 and uses its own limits. Integers are canonical unsigned
decimal (no signs or leading zeros except `0`), checked in their destination
width. `role` is the channel contract-family code (1 shell, 2 lock), not the
profile-name value in the adjacent `api` file. Role is u16; epochs and generations are nonzero u64; all other integers
are u32. `source_kinds` is a nonzero subset of bits 0 and 1. Endpoint is 1–64
bytes from ASCII letters, digits, `_`, `-` and `.`, excluding `.` and `..`.
It is resolved beneath the component endpoint directory containing the role's
9P socket, never as an arbitrary path. Version 1 requires max_packet_bytes=200
and max_fds=4 exactly; a reader refuses any other values.
Unknown, reordered, missing or duplicate fields are refused in version 1.

The three files are immutable snapshots per open fid. Reads at different offsets
on that fid cannot mix versions. Discovery names the limits and formats
generations; their connection epochs and body channel generations must match
discovery. The authenticated role root supplies the contract family; the common
record header does not repeat it. After a
mismatch, close all three and wait for the next caller drive pass. Acquire at
most one complete set per pass, including the initial acquisition,
under the existing connection/startup deadline. Do not spin or send Hello from
an incoherent set. A completed snapshot is discovery, not an admission promise:
ImageBegin checks current permission, topology, format support and budget again.
An output change does not authorize reading another role's outputs.

#### Image limits, 136 bytes

All integers are little-endian. Exact length is required. Reserved fields must
be zero. Use the existing 32-byte role record header: total=136, API version=1,
kind=0x7005, connection epoch, submission=0 and sequence=0. This grant and its
limits_generation are fixed for the entire connection; topology that requires
larger limits needs a new connection. Counters below are ceilings, not current
free-space reports. Runtime admission still reserves within them.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | 32 bytes | common role record header |
| 32 | u64 | channel generation |
| 40 | u64 | limits generation |
| 48 | u64 | max logical bytes per resource, no greater than the role limit |
| 56 | u64 | max retained sealed backing bytes, including retiring leases |
| 64 | u64 | max pending DMA source logical bytes |
| 72 | u64 | max owned renderer allocation bytes, including retiring images |
| 80 | u32 | max owned renderer entries |
| 84 | u32 | max offers, matching discovery and bounded by role transfer slots |
| 88 | u32 | max validation jobs, queued plus running |
| 92 | u32 | max capture jobs, queued plus running plus quarantined |
| 96 | u32 | max admitted outputs, 1–16 and no greater than role limit |
| 100 | u32 | max format rows per output, 1–256 |
| 104 | u32 | max width, no greater than role limit |
| 108 | u32 | max height, no greater than role limit |
| 112 | u32 | validation timeout in ms, matching discovery |
| 116 | u32 | capture-interest timeout in ms, matching discovery |
| 120 | u32 | validation chunk bytes, matching discovery |
| 124 | u32 | max packets per turn, matching discovery |
| 128 | u64 | reserved = 0 |

Epochs, generations, resource bound, dimensions and max offers are nonzero.
Validation jobs must be between 1 and max offers when sealed memory is advertised;
sealed backing capacity must then cover at least one maximum-size resource.
Otherwise both sealed fields are zero. DMA-BUF requires positive source capacity,
renderer byte/entry reservations and capture jobs (at most max offers); otherwise
those four fields are zero. These are separate bounds, not additive permission
to exceed the global image ceiling. Source bytes are logical accounting only;
driver allocations are not bounded by this number. Reserve actual destination
allocation before capture. Retiring resources can exhaust capacity even when
no offer is pending. Publication must refuse inconsistent grants.

#### Image formats, 64-byte header plus 40-byte rows

The first 32 bytes are the common role record header, with total=64+40*rows,
API version=1, kind=0x7006, connection epoch, submission=0 and sequence=0.
The next 32 bytes describe the snapshot. This reuses the normal object framing;
only ASCII discovery has a separate grammar.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | 32 bytes | common role record header |
| 32 | u64 | channel generation |
| 40 | u64 | formats generation, matching discovery and role topology generation |
| 48 | u64 | limits generation, matching discovery |
| 56 | u32 | row count, 0–4096 |
| 60 | u32 | reserved = 0 |

Each row is `output:u64 @0`, `device_major:u32 @8`, `device_minor:u32 @12`,
`fourcc:u32 @16`, `flags:u32 @20`, `modifier:u64 @24`, `max_width:u32 @32`,
`max_height:u32 @36`. Output, fourcc and dimensions are nonzero. Device numbers
identify the already granted composing device; they grant no right to open one.
Flag 1 means opaque with alpha forced to one during capture; flag 2 means full
owned-copy premultiplied validation is supported. Exactly one flag is required;
phase one advertises only 1. Width and height cannot exceed the limits object.

Rows are ordered by unsigned `(output, device_major, device_minor, fourcc,
modifier)`, with no duplicate key. Enforce both per-output and total bounds from
the limits object, checked multiplication, and exact length `64 + 40 * rows`
(maximum 163904 bytes). Sealed-only discovery has zero rows: sealed BGRA is
defined by the source layout, not a fabricated GPU format. DMA-BUF rows describe
only authorized outputs on the granted device. An empty DMA list means no
currently usable import pair, not permission to assume LINEAR. A format change
requires a new role topology generation; formats_generation equals that value.
Publish the updated format snapshot before announcing the topology through the
role's existing topology notification. Even a format-only change advances that
topology generation. Clients re-read discovery and formats on that notification;
an already-open snapshot stays unchanged. Fixed limits do not change during this
retry. There is no independent unannounced formats update or new ObjectPublished
vocabulary in version 1. A stale ImageBegin topology is refused as shell Stale
(reason 1) or lock topology (reason 7), never as malformed. A client re-reads
after that refusal too, since its offer may race the topology notification.

This coupling has a cost. A format-only topology change makes shell components
fetch current output facts and rebuild/resubmit candidates. While locked, it
stales the provider's earlier allocations and requires new whole-output images
and presentation. This contract accepts that disruption for renderer/device
reinitialization: format rows belong to outputs on the granted composing device,
so a changed import set must be handled as a topology-level event. This is a
design requirement, not evidence that every driver format change currently
produces such an event. Do not bump topology for an unchanged format set.

The shell and lock contracts include this broader topology trigger. A format-only
change must be tested while shell candidates and lock images are live, including
retention of the lock cover throughout replacement.

#### Admission refusal

`image_channel_unavailable` is ResourceStatus Rejected reason 13 for shell r9
and reason 9 for lock r2. Shell reason 13 is valid only for Rejected under r9;
all earlier revisions and all other shell reason fields retain their existing
bounds. The codec must enforce that distinction, not widen the shared reason
helper. Lock retains its open reason vocabulary. A disabled
channel refuses ImageBegin before ImageAdmitted, so no SourceReleased is owed.
Other permission, size, stale-allocation and budget refusals retain their role's
existing meanings. Malformed transaction bytes keep ordinary submit errors.
ChannelClosed is the prompt state-change notification; this rejection covers
the race between observing readiness and submitting the next offer.

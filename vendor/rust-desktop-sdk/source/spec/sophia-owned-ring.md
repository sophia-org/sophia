# Shell owned-ring image contract, version 1

Normative amendment, 2026-10-21. Shell revision 10 uses owned rings as its sole
CPU pixel-upload source. Immutability comes from a Session-owned copy. The
producer's ring is mutable; neither generation checks nor copying authenticate
pixels. Lock r2, DMA-BUF and metadata-only shell r1–r8 are unchanged.

## Negotiation and discovery

The client explicitly requests revision 10 and capability bit 15 (`OWNED_RING`),
together with `CONTENT_SURFACE` (bit 7) and its role profile. Revision alone
grants neither capability. Check the role profile against its existing r8 base
after removing image capability bits. Shell sealed bit 13 is retired. If image
negotiation cannot select r10, return UnsupportedRevision; if r10 is in range
but bit 13 is required, refuse with that denied capability. DMA bit 14 retains
its separate availability rules and is not implied by bit 15.

Discovery remains `sophia-image-channel version=1`, with the same fields.
For shell r10, `source_kinds` and ChannelReady's granted mask are exactly 4.
Lock masks remain subsets of 1|2. The existing authenticated image-channel
socket and 40-byte Hello are reused. ImageLimits and ImageFormats retain their
layouts and identity checks. Source kind 4 requires CPU validation capacity;
it grants no DMA or renderer capacity. The field name
`max_sealed_backing_bytes` is retained, and at r10 includes ring source backing
and immutable owned copies. No byte or sealed fallback is permitted.

Expose the writable `owned-ring-v1` file only for a live authorized content
grant negotiated at r10 with bit 15, after discovery and ChannelReady granting
4. Readiness never grants authority. ImageBegin, ImageAdmitted, SourceReleased
and the old kind-2 Source packet are not used for shell r10 CPU uploads.

## Framing and registration

[sophia-owned-ring-v1.kdl](../protocol/sophia-owned-ring-v1.kdl) defines the exact
little-endian byte layouts. Every record has a u32 total length, four magic
bytes and u64 words. Unknown kinds, noncanonical fields and inexact lengths
are refused. Identity is the nonzero connection epoch, content grant epoch,
channel generation, ring id and ring generation. Parsing never proves admission.

Each control-file write contains one D4C1 Register (72 bytes) or Offer
(152 bytes). Register fixes 1–4 slots and a per-slot capacity: nonzero,
divisible by four, at most 4 MiB and at most the granted max_resource_bytes.
Each slot occupies `4096 + round_up(capacity, 4096)` bytes. Its first eight
bytes hold a little-endian generation; pixels start at offset 4096. The source
file size equals slots times slot extent exactly.

One registration is allowed per connection. An exact repeat of the last
accepted control record succeeds without effect; other registrations after
the first return EALREADY, including after RingRefused. A new connection must
register its own source; no identity or custody crosses connection epochs.

Reserve the source extent against Session-wide headroom, separately from and
in addition to the role's resource envelopes. Registration also requires
`source_bytes + capacity <= max_sealed_backing_bytes`, with checked arithmetic.
Do not inflate global budgets to satisfy a request. Temporary budget pressure
may wait under the fixed registration deadline; failure publishes RingRefused,
which is final for this connection. Well-formed geometry is not a promise of
simultaneous admission. An SDK may default to one slot at max_resource_bytes
rounded down to a multiple of four, but must still handle refusal.

RingAdmitted means source budget is held. Only then send one D4F1 packet
(72 bytes, identical registration words) and exactly one SCM_RIGHTS fd on the
ready image channel. The original registration deadline bounds fd arrival.
The fd must be a regular file of the exact size. Linux admission requires
F_SEAL_GROW|F_SEAL_SHRINK; other hosts require their own proved source policy
before support is advertised. Recommended producer hardening is a writable
mapping followed by FUTURE_WRITE|SEAL and MADV_DONTFORK. FD custody must end
before its source charge is returned. RingReady means offers may begin.

On r10 channels only, bytes 4..8 equal to D4F1 select this packet before
ordinary channel-header decoding. D4 cannot collide with the u16 version 1.
Other packets retain their existing framing and authorization checks.

## Offering a slot

An Offer names the registered identity, an increasing nonzero offer id, a
slot in that ring, an increasing nonzero even generation for that slot, and
the complete resource, allocation, output and topology identities plus width
and height. Dimensions must satisfy negotiated limits and checked
`width * height * 4 <= capacity`. Pixels are tightly packed premultiplied BGRA
under the existing CPU image validation rules. Copy and charge exactly that
resource extent, never the unused slot tail.

Write generation and pixels before the Offer. The producer must not change
the slot until the matching SlotReleased. Session checks generation before
and after reading; a mismatch rejects the offer with changed_generation set,
without silently retrying it into a presented frame. The check detects a
cooperating producer's generation change, not malicious pixel races. Such a
race cannot change trusted geometry, identities, ownership or memory bounds.

Admission rechecks allocation, topology, authorization, expiry and all content
limits. The owned destination is a distinct resource keyed by id and generation,
retained until ResourceReleased after the last consumer. Slot reuse cannot
alter an accepted resource. Source storage and each retained destination have
separate charges. At most min(4, max_offers) slot offers are live; each slot and
resource identity has at most one live offer. Offers consume their id and slot
generation when accepted by the control file even if later refused.

Control errors are EACCES when undisclosed, EINVAL for malformed or over-limit
requests, ESTALE for mismatched identity, EALREADY for a non-increasing offer
or slot generation, and EAGAIN for temporary queue, slot, credit or journal
pressure. A retry never extends an existing deadline. Use the existing bounded
retry and connection-failure rules.

## Settlement and release

D4E1 events (120 bytes) share the shell journal's monotonically increasing
sequence, acknowledgement and custody. Dispatch their magic before ordinary
shell-envelope decoding, only on r10 with bit 15. RingAdmitted, RingReady,
RingRefused and SlotReleased have the canonical fields in the KDL. Slot zero
is valid. Changed generation requires a nonzero reason; zero is never a claim
of authenticated pixels.

For every admitted Offer, its final resource status precedes SlotReleased.
Success is ResourceStatus 2 followed by release reason 0. Refusal or cancellation
publishes its terminal reason before release. A cancellation may put the cancel
transaction on ResourceStatus instead of the offer id. Match settlement by
grant and resource identity with store provenance; failed control replies do
not settle the transfer or spend its held final-status reservation. Do not
infer final settlement from a transaction number alone.

Only the exact SlotReleased permits producer slot reuse. ResourceReleased
describes the separate immutable destination lifetime. An SDK exposes a ring
image as settled/adoptable only after both successful ResourceStatus and the
matching successful SlotReleased; native-surface adoption preserves all its
existing resource, grant and allocation checks. A refusal is never adoptable.
Disconnect, revocation and journal loss end the old connection's custody;
there is no replay into a successor connection. Queued readers and destination
consumers retain their charges until their actual lifetime ends.

## Conformance and migration

The [shared corpus](../protocol/golden/sophia-owned-ring-v1.tsv) contains shape
and contextual capacity cases; Offer cases use capacity 184320. Diagnostic
categories describe the rejected condition, not a wire error enum. Decode and
admission are separate checks. C and Rust SDKs consume this same contract and
run matching memory, codec, adapter, cancellation, reconnect, adoption and
release-order scenarios before Sophia updates both pins.

Lom and Provlita migrate their shell image producers to r10 and persistent
rings before sealed shell ingress is removed. Narthex remains metadata-only.
Kleis remains on its separate lock protocol; this amendment makes no lock-ring
claim. Historical sealed/ring benchmark evidence remains attached to the exact
binaries and protocol revisions that produced it.

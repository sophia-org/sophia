# Sophia Output Authority v1

**Status:** experimental major 1 revision 1. This role inherits the
[native family contract](sophia-policy-ipc.md). Its
[KDL schema](../protocol/sophia-output-v1.kdl) owns packed layouts, kinds,
limits and values; [generated tables](generated/sophia-output-v1-wire.md)
make those layouts readable. Schema extraction does not declare stability.

## Admission and negotiation

Session creates an owner-only `SOPHIA_OUTPUT_SOCKET` and admits one authorized
supervised UID/PID. Output is an independent role even when the same supervised
process also owns the WM role. It conveys opaque head/mode identities and
connector-neutral display labels, never DRM handles or raw input. Output
authority does not confer application placement, focus or shell metadata.

The peer sends `ClientHello` (kind 64) with transaction zero, a nonzero ordered
revision range containing 1, and capability bits. `observe` (bit 0) is required;
`configure` (bit 1) is optional. Unknown requested bits are intersected away.
The peer must inspect the selected capabilities before sending proposals.
The server answers `ServerWelcome` (65), transaction zero, revision 1 and a
nonzero server-owned connection epoch. Missing observe, an incompatible range,
repeated negotiation or an unexpected frame refuses the connection; revision 1
has no handshake-error message. Codec acceptance alone is not negotiation.

Welcome announces effective maxima for heads (16), logical groups (16), modes
per head (128) and heads per group (4). A client obeys the announced limits;
current Session announces the schema ceilings. Labels have a fixed ceiling of
64 UTF-8 bytes, without a terminator. Limits are counts, not authority grants.

## Complete facts and candidates

`Snapshot` (66) is one complete frame, carrying a nonzero transaction, connection
epoch, topology epoch, primary logical output, head descriptors and groups.
The snapshot transaction identifies that publication; proposals choose their
own nonzero transaction and correlate through the connection and base topology
epochs. They do not echo a snapshot transaction as the WM does.

Each head has a unique nonzero identity and generation, nonempty label and
nonempty mode list. Modes have unique nonzero IDs within the head, positive
pixel dimensions and nonzero millihertz refresh. An enabled head must be
connected and name a current mode in that list. Transform flags use bit
`(transform enum - 1)`; the set must be nonempty. Boolean fields accept only 0 or 1.
Group IDs and generations are nonzero and unique; members reference known heads
and a head belongs to at most one group. Every enabled head is grouped. The
primary output must be one of the published groups.

`Proposal` (67) requires configure and supplies one complete topology candidate:
base topology epoch, validate-only or apply intent, zero-based primary group
index, head targets and logical groups. Head generation and mode must match the
current connected head. Rotation and VRR must be supported. Every proposed head
belongs to exactly one proposed group; groups may contain one head (extended)
or several (mirrored). Connected heads omitted from the candidate are disabled.
A nonzero group output ID preserves a current identity; zero asks Engine to
allocate one. Fit, cover and exact are explicit per-member mappings.

Logical x/y/width/height are signed 32-bit fields. Revision 1 admits nonnegative
x/y and positive width/height, and candidate groups must not overlap. A negative
origin can be decoded in a proposal but is rejected by topology validation;
decoding a snapshot also validates its complete structure. Decoding proposals
cannot validate against facts that only the owner possesses. There is no scale
field or client-supplied native target on this wire.

All reserved fields are zero and every payload must be consumed exactly.
Unlike WM and shell content, output revision 1 needs no begin/chunk/end messages:
even the independent maximum nested counts occupy at most 52,128 payload bytes,
below the common 65,536-byte payload ceiling. Every snapshot and proposal is
atomic at the framing boundary. Socket fragmentation is not a protocol chunk.

## Settlement and recovery

Proposal transactions cannot be reused within a connection epoch, including a
proposal that reached semantic validation and was rejected. The runtime retains
that history until disconnect. One candidate is active and one complete latest
successor may wait; replacing the queued candidate requires an explicit stale
outcome for that identity. Promotion must revalidate against the then-current
topology, since the predecessor or hotplug may have changed it.

`Outcome` (68) echoes the proposal transaction and connection epoch and names the
current topology epoch. Its closed outcome enum is validated, committed, stale,
rejected, rolled back or failed. Validated settles validate-only work without
applying it. The reduced reason is an open u16 diagnostic code: known values
are in the schema, and an unknown reason never changes the meaning of the
outcome. This intentional exception to closed enums preserves the existing
revision-1 codec. Admission failures currently report rejected/invariant;
clients must not require every stale condition to use the stale outcome.

For apply, the physical owner prepares targets and the first frames before
changing hardware. Partial apply enters rollback. It publishes the new logical
topology only after every new output has presented; until then the previous
coherent topology remains authoritative. Preparation, head loss, apply,
first-presentation and rollback failures have explicit reduced reasons. No
wire timeout duration is negotiated; deadlines are owner policy, and a failed
or disconnected transaction cannot be assumed committed from elapsed time.

The transport accepts partial local-stream frames without presenting partial
work. Its retained input byte budget is 65,560 bytes, including the header;
clients must not rely on arbitrary pipelined bursts beyond that budget. The
worker has bounded socket I/O waits; a truncated disconnect retires the peer.
Disconnect or reauthorization abandons active/queued work and advances the
connection epoch. A replacement negotiates again and receives complete current
facts. Committed hardware state survives peer loss; if an apply is in progress,
the physical owner settles or rolls it back before publishing replacement facts.

## Evolution and evidence

Revision 1 has no extension-record area. Unknown kinds, transforms, mappings,
intents, outcome values, flag bits, reserved values or trailing bytes fail
closed. New vocabulary requires an explicitly negotiated revision or capability
with an outbound gate; it cannot be appended silently. The diagnostic reason
exception above grants no new operation.

`cargo run --offline -p sophia-policy-protocol-gen -- --check` checks schema
artifacts; `cargo test --offline -p sophia-protocol --test output_schema`
compares generated samples and malformed vectors with the handwritten codec.
Runtime output IPC, transport and service tests cover negotiation, queuing,
fragmentation and peer replacement. The live Session output owner and native
apply/rebuild path exist; these deterministic tests do not establish physical
display acceptance. Independent full-lifecycle interoperability and immutable
compatibility evidence remain part of the family conformance/stability gates.

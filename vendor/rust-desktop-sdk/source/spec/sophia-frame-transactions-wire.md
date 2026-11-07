# Sophia Frame Transactions: initial wire contract

This is the SFT implementation contract, not enablement or performance acceptance.
The [architecture](native-frame-transactions.md) owns the security, capture,
presentation and qualification requirements. The [allocation ledger](../protocol/sophia-shell-allocations.md)
owns identifiers. The byte layouts below extend the existing shell file envelope;
9P, afid/custody, submission assembly, journal acknowledgements and image-channel
registration retain their existing contracts.

## Negotiated scope

SFT is shell revision **13**, capability bit **18** (`FRAME_TRANSACTIONS`,
`0x40000`). Selecting r13 does not grant it. The initial native profile requires
the ordinary content role and explicit owned-ring bit 15 as well as bit 18.
The host build, owner permission and peer request must all opt in. Required bits
cannot silently disappear. Unknown revisions or unsupported combinations refuse;
do not implement this as a `revision >= 13` check.
The first opt-in host profile accepts `minimum_revision = maximum_revision = 13`.
An ordinary r10 request stays r10 even when the owner permits SFT.

The r13 owned-ring image mapping is image revision 10, source kind 4 and the
existing D4 records. SFT requires a two-slot registration when a frame copies
pixels. Retained-only frames need no source. No r9 sealed-upload path, GPU source,
draw capability or W2 bit 17 is implicitly enabled. SFT's predecessor field uses
W2's retirement lifetime semantics inside FrameCommit; it does not authorize the
separate r12 Candidate trailer. r11 draw and r12 W2 identifiers remain reserved
and retain their allocated meanings. A host may refuse their grants while
supporting the bounded r13 profile; it must never reinterpret those records.

Each stream binds one output, allocation and output-facts generation. Initially
it has exactly one credit and one complete base-profile surface with retained
CPU layers, zero or one new owned-ring image, and zero or one predecessor intent.
Unsupported updates drain/close before using the legacy path; the producer keeps
legacy selection for that output binding. It does not flatten content, abandon
the current display or discard live resources to enter SFT.

## Optional complete-write submission

An r13 SFT export may expose the write-only `frame-submit` file. Its absence
(`ENOENT` on lookup) selects the existing `transaction-reuse` plus `submit`
path. Other discovery/open failures are errors, not permission to downgrade.
There is no new record, capability bit, negotiation default or 9P operation.
Open/Close and non-FrameCommit candidates still use the existing path.

A client first opens its ordinary reusable transaction writer, after the
covering bootstrap ACK. `frame-submit` owns no second writer: every write uses
that attach's current live reusable writer. Opening or clunking this plain fid
does not acquire, release or replace that token. The export, authenticated
attach, epoch and request-level custody checks are unchanged; the node is not
visible before explicit r13 negotiation. It has no readable content or stored
file length.

One offset-zero Twrite must contain one complete, valid FrameCommit, including
its ordinary header, submission id and entire body. The client selects this
route only if the record fits `msize - 23` and the node's `iounit` when it is nonzero;
otherwise it uses the existing transaction path (including fragmentation or
local size refusal) **before** sending bytes. This extension does not expand a
client's existing record-size support.
A partial assembly cannot be continued through this node. The server reuses the
ordinary bounded assembly, decoding, inbound capacity and journal reservation
checks, then retains the same bytes under accepted custody and emits the same
Submitted record. Rwrite's count is the complete record length. It establishes
file custody, never frame admission, credit, source release or presentation.

EAGAIN retains the same staged bytes, submission identity and original assembly
deadline. Retry sends exactly those complete bytes to the same node, under the
same live token and existing bounded backoff. A successful retry of accepted
bytes is idempotent; matching just the header and length is insufficient.
Different bytes while accepted are EBUSY. The covering cumulative journal ACK
still controls buffer reuse. Writer clunk, expiry and revocation keep their
existing semantics; accepted custody is not cancelled by clunk. A plain
frame-submit fid may serve a newly opened writer after a definitive refusal,
but cannot make the former writer or its staging live again.

Submitted and Rwrite may arrive in either order. The existing refusal and
unknown-custody rules apply, including EALREADY and disconnect. Never fall back,
change submission id or replay on a new connection after unknown custody.
Prepared/terminal notices, ACK policy and owned-ring registration are unchanged.

## Records and bytes

All integers are little-endian. Offsets below are relative to the body after the
unchanged 32-byte shell file header. Candidate-class records have a nonzero
submission id and zero sequence; events have zero submission id and nonzero
sequence. Every body transaction is nonzero and every body grant connection
epoch equals the envelope epoch. Exact lengths, reserved zeros, enum bounds and
cross-field constraints are mandatory; trailing bytes are not extension space.

| Record | Kind | Class | Body bytes |
|---|---:|---|---:|
| FrameStreamOpen | 279 | Candidate | 72 |
| FrameStreamClose | 280 | Candidate | 72 |
| FrameCommit | 281 | Candidate | Variable; whole record at most 8192 |
| FrameStreamOpened | 55 | Event | 80 |
| FrameStreamClosed | 56 | Event | 80 |
| FrameRefused | 57 | Event | 64 |
| FrameOutcome | 58 | Event | 116 |

DrawSettled remains kind 54. No existing kind changes its bytes.

`StreamKey` is eight nonzero u64 words in order: grant connection epoch, grant
content epoch, output id, output generation, allocation id, allocation generation,
facts generation, stream id. Its size is 64 bytes. Facts generation includes
mode, scale and format changes, not just topology. The producer proposes a stream
id strictly greater than every previously accepted stream id in that grant,
across outputs. Refused opening does not consume it. A new stream cannot inherit
credit, work or replies from a preceding stream.

Open and Close are transaction u64 at 0, StreamKey at 8. Open has no requested
credit count. Opened has the same prefix, credits u32 at 72 (exactly 1), ttl_ms
u32 at 76 (1..250, bounded by the granted permit timeout). Closed has the same
prefix, reason u16 at 72 (1..12), and six zero bytes at 74. An explicit close
uses its own transaction; an automatic close notice uses the opening transaction.
Revocation/disconnect need not deliver a notice after authority ends and must
never deliver it into a replacement grant.

FrameCommit has this layout:

| Offset | Field |
|---:|---|
| 0 | transaction u64, nonzero |
| 8 | scene_bytes u32, exact size of the following neutral Candidate value |
| 12 | flags u16: bit 0 image present, bit 1 predecessor present; all others zero |
| 14 | reserved u16, zero |
| 16 | neutral Candidate value, `scene_bytes` bytes |
| 16 + scene_bytes | optional complete 152-byte D4C1 RingOffer |
| after optional offer | optional 24-byte predecessor: transaction, resource id, generation (u64 each) |

The neutral Candidate is the existing Candidate body **without** its outer
transaction: a 72-byte fixed prefix followed by 64-byte surface, 32-byte placement
and 48-byte target rows. The last u16 of that prefix stays zero: the r12 trailer
is not embedded here. Its `pacing_permit` field carries the stream id. Exactly
one surface is required; placement/target counts remain bounded by the existing
grammar (32/64) and granted limits. All legacy row validation still applies.
Lengths use checked arithmetic, agree with counts and flags, consume the entire
record, and fit both 8192 and the granted whole-record byte limit before any row
allocation. The old runtime's internal 40-byte candidate accounting base is not
a substitute for this complete wire length.

An embedded offer is the existing D4C1 Offer including its length/magic/kind.
Its connection/grant, output, allocation and facts identities must equal those
of the scene and its sole surface. Channel/ring/generation must name the current
registration; slot is 0 or 1, slot generation is even and nonzero. All existing
offer dimensions, extent and identity checks still apply. The new resource must
occur in a placement. There is no separate Offer submission for the same frame.
The host reserves and settles the ordinary source-release obligation internally.

Every predecessor word is nonzero; its transaction differs from the FrameCommit
transaction, and the scene does not reference that resource. No tuple or slot
claim is authority. The owner freshly checks custody and its actual registered
source, allocations, accepted resource backing and stream key.

FrameRefused is transaction u64 at 0, grant epochs at 8/16, output id/generation
at 24/32, stream id at 40, candidate generation at 48, request kind u16 at 56,
reason u16 at 58 (1..12), reserved u32 at 60 (zero). All identity words are
nonzero except candidate generation: it is nonzero for FrameCommit and zero for
Open/Close. Request kind is exactly 279, 280 or 281. This is a request refusal,
not a terminal for an admitted candidate; it never carries credit or retires
anything. It echoes the refused request's identities even if that stream closed.

FrameOutcome starts with the unchanged **76-byte CandidateOutcome body**,
including its transaction and all existing kind/reason/presentation constraints.
It appends stream id at 76, allocation id at 84, allocation generation at 92,
facts generation at 100 and credit_expires_msec at 108 (u64 each). The first four
are nonzero. The last is zero for no credit, otherwise a nonzero deadline on the
owner's monotonic-millisecond clock. Prepared (kind 1) must carry zero. Terminal
kinds 2..4 may carry zero (closing/expired stream) or one renewed credit. Both
halves name the exact same stream/candidate binding; an ordinary CandidateOutcome
must never be used to deliver an SFT candidate's Prepared or terminal event.

The deadline is opaque owner time, not permission for the client to infer clock
offset or renew credit locally. Opening TTL starts when the owner commits Open;
a renewed deadline starts at whole terminal transfer. Neither starts at peer
receipt or ACK. Clients must handle a close/expiry race after seeing credit.
Only one matching terminal can replenish once. SlotReleased, ResourceStatus,
ResourceReleased and native buffer retirement retain their distinct meanings.

## Input, output and backpressure

The one-surface profile requires every placement and target to name surface
index zero. An embedded offer's checked `width * height * 4` must fit the
owned-ring protocol maximum of 4 MiB before contextual admission. The actual
registered capacity, allocation bounds and negotiated limits may be smaller
and remain fresh owner checks; these shape bounds do not authorize a source.

The existing per-submission file assembler accepts fragmented writes only within
its declared length and timeout. A prefix, missing suffix, duplicate fragment or
failed submission never admits half a scene. Decode a complete bounded record
once, retain its owned rows until admission, then move them into the existing
candidate store. Do not split it into ordinary Begin/Chunk/End wire requests.
An invalid envelope or undecodable body follows the existing file submission
refusal path, never a fabricated SFT candidate terminal. FrameRefused applies to
a structurally decoded request whose bounded semantic admission refuses.

Before consuming Open or Close, hold one nonfatal refusal slot. A successful Open
instead reserves two control records: Opened and its lifetime Closed. A successful
Close spends the existing lifetime reservation, not another terminal slot.
Before consuming FrameCommit, hold a refusal slot; acceptance replaces that
obligation with Prepared and terminal reservations, plus resource status/release
and slot-release journal capacity if copying. Reserve the predecessor intent
before any source read. Every fixed SFT control event fits the existing 256-byte
control credit; actual encoded output, shared FIFO and journal capacities must
all be checked. No blanket per-fragment response slot is charged.

No reply room leaves input, identity, credit and predecessor untouched. A valid
frame identity refused for Budget or profile-invalid content is spent, with a
typed refusal and no credit spend. Stale key/phase/sequence does not spend it.
Assembly-present, exhausted credit and close/expiry races are nonfatal reported
refusals on this path; invalid transport authority still fails closed. An open
stream excludes demand/permit admission on its output. A missing/closed stream
does not reinterpret FrameCommit as a legacy Candidate.

Source capture, validation, accepted-image privacy, cancellation debt, fresh
scene handoff and native completion use the existing reviewed runtime owners.
Prepared has no credit. Queueing a terminal has no credit. Transfer the whole
FrameOutcome, replenishment and successful predecessor retirement as one owner
commit; publication backpressure preserves every obligation. Close forbids new
admissions/replenishment while accepted work drains. Reopen waits for all prior
work and notices, not merely receipt of Closed. Revocation/disconnect drain old
physical owners without replaying their events into a successor connection.

## Integration gates

This contract does not enable a host or SDK client. Required before exposure:
independent C/Rust codecs and golden/refusal parity; exact revision/capability
negotiation; complete/fragmented production-export admission under full record
and byte budgets; fresh owner-loop handoff; real Lom and reference producers with
drain/fallback; implementation trace validation against the model; and matched
native CPU, cadence and latency qualification. Default-off remains mandatory.

## Terminal-only notices (r13, required capability bit 19)

`FRAME_TERMINAL_ONLY` (`0x80000`) is an additive option, not a new layout.
The request and grant must both include bit 19, bit 18, owned-ring bit 15 and
all existing content-role prerequisites, at exactly revision 13. A separate owner permission for this option is required in addition to SFT
authentication and build support. Unsupported
required bits, bit 19 without SFT, and unsolicited or missing bit 19 grants
refuse negotiation. Without bit 19 the existing r13 behavior is unchanged.
The selected option is immutable for the connection and bound to each opened
stream; close, expiry and replacement cannot transfer it to another grant.

An opted-in stream emits no Prepared FrameOutcome (kind 1). Both SDKs reject
one if received under this grant. All record sizes, sequence numbering, ACK
policy, source status/releases and terminal layouts stay unchanged. Clients
must not infer preparation, presentation or source reuse from silence. A
terminal may arrive without Prepared; terminal validation and exactly-once
credit still apply. Refused describes a request that was not admitted; an
admitted frame must settle through its terminal outcome, never a later Refused.

Admission retains the existing two-response reservation (Prepared + terminal).
The internal preparation transition still validates identity, marks the
submitted frame prepared and starts the presentation deadline. At that point
it releases exactly one reserved control record without enqueueing an event,
consuming journal capacity, writing a wire wake or returning frame credit.
This is an obligation release, not a larger admission allowance. Presentation
before internal preparation remains invalid. Renderer failure before preparation
releases both reservations; after preparation it releases the remaining one.
Close/expiry retain outstanding physical work and the lifetime close notice;
revocation drops response obligations as before. A full outbox cannot delay
internal preparation merely to publish an omitted notice. Terminal publication
still waits for capacity and commits credit only on transfer, not on creation.

Ordinary Candidate traffic and unopted SFT streams retain their existing
Prepared records. Default-off SFT policy, native completion and custody checks
are unchanged. Protocol compatibility and device-free accounting proofs are
separate from native CPU/latency qualification.

## Lean commits (r13, required capability bit 20)

`FRAME_LEAN` (`0x100000`) is an additive option at exactly revision 13. The
request and grant must both include bit 20, bit 19, bit 18, owned-ring bit 15
and all existing content-role prerequisites; the owner must separately permit
it. Bit 20 without bit 19, unsupported required bits and unsolicited or
missing bit 20 grants refuse negotiation. The option is immutable for the
connection. Without bit 20 nothing below applies and every record keeps its
existing bytes and events.

Lean semantics apply only to FrameCommit records written whole to the
`frame-submit` node. FrameStreamOpen, FrameStreamClose and any commit that
takes the paired `transaction`+`submit` path keep their Submitted record,
their zero sequence word and the ACK-gated writer reuse above.

Header word: the u64 at offset 24 of a lean FrameCommit is a cumulative
journal acknowledgement, 0 meaning none. A nonzero word has exactly the
meaning of the 16-byte `ack` write (`docs/sophia-output-files.md`): the host
applies it after the envelope and epoch checks and before admission. An exact
repeat is idempotent; an older or unretained sequence refuses the whole write
with EINVAL before any effect. A later admission refusal does not undo an
applied acknowledgement. A client keeps one acknowledgement in flight: no
header word while a standalone `ack` write is unanswered, and no standalone
`ack` while a lean commit carrying a word is unanswered. On any reply other
than the exact-count Rwrite the client treats the word as not confirmed and
re-covers it later; repeats are idempotent and values are monotone. The
standalone `ack` node is unchanged and still required when no commit is
queued: idle, close and manual ACK policy. Every other FrameCommit decoder
keeps the zero-sequence rule; a nonzero word outside a negotiated lean
`frame-submit` write is EINVAL.

No Submitted record: a lean commit emits no kind 18 event. Rwrite of the
complete record length is acceptance and file custody. Any other reply is not
custody; EAGAIN retains the same staged bytes and identity (compared ignoring
bytes 24..32), and retry resends the complete record with a fresh word. A
repeated submission id after acceptance is EALREADY, which a lean client
treats as unknown custody and fails closed. The host recycles the reusable
writer at admission: nothing is retained for a lean commit, so the covering
ACK gates reuse only for Open, Close and paired records. A reply can be lost
only through disconnect, which ends the epoch: a client with a lean commit on
the wire at disconnect reports unknown custody and never replays it under a
new epoch. Both SDKs reject a Submitted record naming a lean commit.

Prepared stays elided by bit 19; terminal outcomes, ResourceReleased, credit,
source status and all record layouts are unchanged. Per frame a lean stream
publishes two journal records (terminal and ResourceReleased) and the client
writes once. Default-off, native completion, custody checks and the separate
native CPU qualification are unchanged.

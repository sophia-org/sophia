# Output file records — revision 1

This document specifies the separate output role over 9P2000.L. Session owns
persistent profile configuration whether or not a client is configured. With
no output process there is no output listener. An explicit `--output-process`
receives only its protected output endpoint in `SOPHIA_OUTPUT_9P_SOCKET`;
WM assignment grants no output authority. The retired `SOPHIA_OUTPUT_SOCKET`
is not a supported endpoint. The
[proposed decision](notes/decisions/vkkjmufd-use-native-records-for-the-separate-output-file-role.md)
records the design history. The ownership rules below govern native effects.

The records use little-endian integers and native rows. They do not contain a
socket frame. `sophia_protocol::output_files` currently implements envelopes,
submit/ack controls and every body described below. Runtime custody primitives
reserve terminal outcomes and bound domain replay history; the export joins
these with file custody. Envelope decoding alone never validates
a typed body or grants authority.

## Physical ownership and recovery

The client receives opaque head and mode identities and connector-neutral
labels, never DRM handles or raw input. Observe/configure capabilities confer
no WM, application, shell or profile-write authority. A proposal names the
current connection and base topology epochs, complete enabled-head intent,
explicit transforms, VRR, geometry and per-member mappings. Omitted connected
heads are disabled by intent. The owner validates support and the full layout.

Validated settles validate-only work without a physical effect. For Apply,
Session prepares candidate and rollback resources before changing hardware.
The new topology is published only after the physical owner accepts every
output's first presentation. Until then the previous published topology remains
authoritative. Partial apply and cancellation retain preparation/rollback debt
until restoration is observed; a preserved snapshot alone is not restoration.
RolledBack reports observed restoration, while Failed does not promise it.
Outcome and acknowledgement are separate: acknowledging a record creates no
physical effect or confirmation of a trial.

Disconnect or reassignment abandons active and queued work at the old epoch.
A replacement negotiates afresh; no fid or outcome crosses the epoch boundary.
Committed state survives client departure. Work already dispatched must settle
or roll back before replacement facts or new effects are admitted. Startup and
profile reload transactions belong to Session and settle locally, without a
transport ticket. Client departure does not cancel the startup transaction.
Runtime changes are not written back to the desktop profile. Revision 1 has
no confirmation operation or automatic trial confirmation timer.

The outcome reason is an open u16 diagnostic code; an unknown reason grants
no operation and cannot change the outcome kind. Unknown kinds, transforms,
mappings, intents, outcome values, reserved values and trailing bytes refuse.
New vocabulary requires a negotiated revision or capability with an outbound
gate. Owner deadlines are not a promise that an elapsed transaction committed.

## Identity and bounds

The header is 32 bytes:

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u32 | Total record bytes, including header |
| 4 | u16 | API version, 1 |
| 6 | u16 | Kind |
| 8 | u64 | Nonzero connection epoch |
| 16 | u64 | Submission ID |
| 24 | u64 | Journal sequence |

Objects have submission ID and sequence zero. Candidates have nonzero
submission ID and zero sequence. Events have zero submission ID and nonzero
sequence. Admission compares the epoch with the export's current epoch;
structural parsing only requires it to be nonzero.

Objects: Limits=1, Topology=2. Events: Negotiated=16, Refused=17,
Submitted=18, ObjectPublished=19, Outcome=32. Candidates: Negotiate=256,
Proposal=257. Unknown kinds and versions are refused.

A whole record is at most 65,536 bytes. A candidate is at most 1,784 bytes.
Complete-record decoders refuse truncation, trailing data and total-length
mismatches; they are not stream assemblers. The Limits object supplies assembly
and acknowledgement deadlines. The export enforces them and retains immutable
publications. A bounded worker serves the adapter under Session supervision.

`submit` is exactly 24 bytes: epoch u64, submission ID u64, candidate length
u32, reserved u32=0. Both identities are nonzero; length is 48..1,784. This
interval is an assembly bound; the typed candidate decoder enforces its exact
shape. `ack` is exactly 16 bytes: nonzero epoch u64 and sequence u64.

## Limits and replay history

Limits has a 40-byte body. The first seven u16 fields are fixed for revision 1:
interface revision=1, heads=16, groups=16, modes per head=128, members per
group=4, label bytes=64, total modes=2,048. The u16 at offset 14 is zero.
The remaining fields are:

| Offset | Type | Resource | Default / ceiling | Allowed minimum |
| --- | --- | --- | --- | --- |
| 16 | u32 | Journal records | 64 | 8 |
| 20 | u32 | Retained journal bytes | 16,384 | 2,048 |
| 24 | u32 | One candidate's staging bytes | 1,784 | 1,784 (fixed) |
| 28 | u32 | Assembly deadline in milliseconds | 12,000 | 1 |
| 32 | u32 | Acknowledgement-progress deadline in milliseconds | 2,000 | 1 |
| 36 | u32 | Domain transactions per connection epoch | 4,096 | 1 |

The decoder refuses zero, under-minimum and over-ceiling values. Offset 36
is an explicit replay-history bound; the earlier evidence-only draft's unused
reserved tail is superseded. Acknowledging a journal event does not remove its
domain transaction from replay history. Domain IDs may arrive in any order.
A candidate rejected by semantic validation still consumes its domain ID, as
on the current output owner.

`OutputConnectionState::with_transaction_limit` enforces the finite history.
Exhaustion refuses a new identity before mutation, preserves both accepted
proposals for settlement and leaves old identities recognizable as reused.
Disconnect does not permit reuse in the same epoch; only a newer connection
epoch starts an empty history. The retiring socket adapter's default behavior
is unchanged. The file export must use the advertised bound and map exhaustion
to ENOSPC without cancelling accepted work. A client drains its accepted
outcomes before deliberately reconnecting. File-submission replay is a separate
export obligation; it is not implemented by this domain-ID bound.

## File lifecycle

The root contains `api`, `limits`, `topology`, `events`, `transaction`,
`submit` and `ack`. `api` reads `sophia-output-files version=1\n`, with an actual
newline. `limits` and `topology` are complete Object records, including their
32-byte headers. The limits header supplies the admitted epoch for candidates,
submit controls and acknowledgements. Discovery uses `SOPHIA_OUTPUT_9P_SOCKET`;
clients refuse the retired `SOPHIA_OUTPUT_SOCKET` variable, even when empty.
An endpoint must admit the supervised peer before binding its connection to
the export. Attach strings confer no authority. Only one attach is admitted
per epoch; version reset does not renew that grant.

`transaction` opens read/write, `submit` and `ack` write-only, and all other
files read-only. Append and truncate are refused. Only one staging handle and
one topology handle may be open. An unsubmitted staging handle expires at the
advertised assembly deadline from its first byte; further use returns ESTALE.
Clunk discards unsubmitted staging without transferring custody. Staging writes
append contiguously or repeat an already-written range exactly; holes and
changed overlaps are refused. Controls are complete writes at offset zero.

Submit must name exactly the staged candidate's epoch, submission ID and byte
length. Its successful Submitted receipt transfers file custody. The next
transaction requires both clunk of the old staging handle and acknowledgement
covering that receipt. Repeating the last accepted candidate bytes has no new
receipt, owner delivery or domain mutation. Lower submission IDs are stale;
changed bytes under the same ID are refused. EAGAIN transfers nothing and can
be retried after progress; domain-history ENOSPC leaves accepted outcomes
settleable. Negotiation and configure capability refusals use EACCES; malformed
controls and records use EINVAL; wrong epochs use ESTALE.

`events` uses persistent byte offsets. Reads may split records arbitrarily;
tail reads wait, offsets beyond tail return EINVAL, and released offsets return
ESTALE. A cumulative ack may release only fully read records. Read coverage
includes partial, repeated and out-of-order reads; a hole prevents release and
returns EAGAIN. Repeating the last ack succeeds without extending the progress
deadline. While records remain, only an advancing ack resets that deadline.
Expiry revokes the connection, including pending reads and open fids.

Topology open returns EAGAIN before successful negotiation. Afterwards it pins
the current immutable bytes; its Qid equals the announcement's Qid. At most one
ObjectPublished may remain unacknowledged. A subsequent publication returns
EAGAIN before replacing the object, so the client can always open the exact
announced object. Its ack requires a complete read of that object through a
topology handle; EOF is not required. An early ack returns EAGAIN without
releasing anything. A previously open pin remains immutable after replacement;
publication also waits if replacement would retain a third version. Clunk
releases the pin. Qid paths are never reused across connection epochs.

A negotiation Refused is terminal but remains readable. The peer acknowledges
it and closes; new proposals are refused. The export revokes after its final
ack or the acknowledgement-progress deadline, whichever comes first.

The worker has eight command slots and eight owner-event slots, plus at most
two locally pending events (an unsent event and a disconnect). Snapshot commands
must pass the owner's finite snapshot bounds before entering the queue. A full
command queue returns the command without custody. The export holds one pending
delivery and returns EAGAIN for new submissions until that slot is free; exact
replay does not require another slot. Physical settlement spends the reserved
Outcome credit. Queue replacement already journals its Stale outcome atomically,
so the worker has no separate uncredited reply command. The Session owner must
revalidate a promoted candidate against its current physical topology.

Pause acceptance uses a separate one-slot control and returns abandoned work
before the caller spawns or authorizes a replacement PID. Stop uses an atomic
flag independent of full queues. The worker polls in bounded nonblocking turns;
absence of a peer does not block pause or shutdown. These are custody and
supervision mechanisms, not physical rollback or display acceptance evidence.

Deadline expiry during publication or settlement disconnects that epoch and
leaves the worker available for later connections. An ESTALE owner error without
export revocation remains an error. Disconnected cancels all physical work the
owner observed for that epoch. An abandoned list accounts for transport custody
and may also contain proposals that never reached the physical owner.

## Explicit Session launch

`--output-process=/absolute/path` selects an independent file-role process on
the native Session path with an external WM. Repeat `--output-process-arg=...`
to supply at most 64 arguments, each at most 4,096 bytes; the absolute program
path has the same byte bound. Native device opt-in remains separately required.
Without this selector, Session retains the existing socket role.

The selected process receives only the OutputAuthority role and
`SOPHIA_OUTPUT_9P_SOCKET`; the WM receives no output endpoint or output grant.
Automatic and administrative WM restarts preserve the independent output
process, connection epoch and topology pins. Session does not automatically
restart this one-shot process or repeat its command. Exit requests acceptance
pause and epoch cancellation. Admission retains a pidfd for the authorized
process and checks it before and after peer credentials, preventing a recycled
PID from gaining authority while exit notification is pending. Protected launch
rechecks bubblewrap's child relationship after opening the pidfd. An already
exited peer becomes a dead assignment with no admission authority.
The kernel must support `SO_PEERPIDFD`; absence is a connection rejection with
that requirement in its diagnostic. A failed identity lookup releases the
accepted peer without spending an epoch and leaves the listener available.

The supplied-topology launch/restart fixture and independent C SDK exchange
exercise supervision and file custody. Physical topology acceptance and the
output default rollout remain pending under t253.
This selector launches the process at Session startup. It does not provide an
interactive terminal command launcher or return the child's stdout to a caller.

## Journal custody

The output journal reserves one record and 56 bytes for each pending proposal's
terminal Outcome. At most two proposals are pending: one active and one queued.
Every publication subtracts these reservations from available capacity, and
also preserves their sequence/offset space. Acknowledgements release retained
records, not terminal reservations. Wrong identities or epochs cannot spend a
reservation. An outcome spends exactly its matching reservation.

`Journal::prepare_batch` checks a whole batch before changing the journal.
Dropping a prepared batch publishes nothing and consumes no identity.
The output-specific admission guard reserves Submitted plus a terminal credit
before domain admission. Replacing the queued proposal batches the new
Submitted with the old proposal's Stale outcome and transfers its credit to
the new identity. An immediate semantic rejection batches Submitted and
Rejected without borrowing either existing proposal's credit. Negotiation
batches Submitted, Negotiated and ObjectPublished; refusal batches Submitted
and Refused. Capacity refusal cannot leave a published prefix of these batches.

The export joins these primitives to the real domain owner. It performs trial
domain admission before committing the corresponding prepared batch; capacity
refusal leaves the live domain identity unchanged. Its supervised transport is
tested through actual 9P requests and supplied-topology Session supervision.

## Negotiation and submission receipt

Negotiate has a 16-byte body: minimum revision u16, maximum revision u16,
reserved u32=0, requested capabilities u64. Unknown capability bits and
unsupported revision ranges reach the owner. Negotiation intersects requested
capabilities with observe (bit 0) and configure (bit 1), and requires observe;
the codec does not turn these owner refusals into malformed bytes.

Negotiated has a 24-byte body: selected revision u16=1, six zero reserved
bytes, granted capabilities u64, then four u16 bounds: heads=16, groups=16,
modes per head=128, members per group=4. Observe is required; granted bits
outside observe/configure are refused. The client must additionally check the
grant against its actual request. The connection epoch is in the event header.

Refused has an 8-byte body: reason u16 (1 unsupported revision, 2 observation
required), six zero reserved bytes. It is a negotiation refusal, separate from
a topology Outcome. The export must keep the terminal record readable until
acknowledgement or its bounded refusal-drain deadline; appending it and
immediately revoking reads would lose the refusal. This lifecycle belongs to
the export rather than the codec.

Submitted has a 16-byte body: nonzero submission ID u64, candidate kind u16
(256 or 257), six zero reserved bytes. This receipt identifies file custody.
It does not mean a topology transaction was validated or committed.

## Topology publication

ObjectPublished has a 24-byte body: object kind u16=2 (Topology), six zero
reserved bytes, nonzero topology epoch u64, nonzero Qid path u64. The topology
epoch identifies the domain generation; the Qid path identifies the exact
immutable bytes retained by the export. The file lifecycle above defines
publication retention and read/ack dependencies.

The Topology body contains a 24-byte prefix, head rows (104 bytes each), mode
rows (24 bytes each), then group rows (84 bytes each):

| Prefix offset | Type | Field |
| --- | --- | --- |
| 0 | u64 | Nonzero topology epoch |
| 8 | u64 | Primary logical output |
| 16 | u16 | Head count, 1..16 |
| 18 | u16 | Group count, 1..16 |
| 20 | u16 | Total mode count, 1..2,048 |
| 22 | u16 | Reserved, zero |

| Head offset | Type | Field |
| --- | --- | --- |
| 0 | u64 | Head identity |
| 8 | u64 | Generation |
| 16 | u16 | Flags: connected=1, enabled=2, VRR capable=4; no other bits |
| 18 | u16 | Nonzero transform mask; bits 0..7 correspond to proposal values 1..8 |
| 20 | u16 | UTF-8 label byte count, 1..64 |
| 22 | u16 | Head mode count, 1..128 |
| 24 | u64 | Current mode, or zero for absent |
| 32 | u16 | First mode index in the flattened table |
| 34 | 6 bytes | Reserved, zero |
| 40 | 64 bytes | Label followed by zero padding |

The first mode index must equal the sum of preceding heads' mode counts.
The ranges must cover the entire mode table exactly. A mode row contains mode
identity u64, width/height i32, refresh in millihertz u32, preferred u16 (0 or
1), reserved u16=0. IDs, dimensions and refresh must be positive.

A group row contains output identity u64, generation u64, x/y/width/height i32,
member count u16 (1..4), reserved u16=0, then four 12-byte member slots with
the same layout as Proposal. Unused slots are zero.

Both encoding and decoding apply `OutputAuthoritySnapshot::validate`:

- The topology epoch is nonzero. Head IDs are nonzero and unique; head
  generations are nonzero, labels contain 1..64 UTF-8 bytes, and each head has
  1..128 modes. Mode IDs are unique within each head; mode IDs, dimensions and
  refresh are positive.
- An enabled head is connected and its current mode belongs to its own mode
  table. A disabled head may have zero or any nonzero current-mode identity;
  the owner does not check membership for disabled heads. The encoder refuses
  `Some(INVALID)` rather than silently converting it to `None`.
- Group output IDs are nonzero and unique; generations are nonzero. Logical
  x/y are nonnegative and width/height positive. Each group contains 1..4
  existing heads, and each head appears in at most one group across all groups.
- The primary output names a group, and every enabled head is grouped.

The row counts, enum values, flags, transform masks and reserved bytes must
also satisfy the wire rules above. Snapshot validation does not reject group
overlap; proposal validation separately rejects overlapping proposed groups.

Revision 1 reports supported transforms and VRR capability, but does not expose
the head's current transform or VRR policy. A proposal is a complete target
configuration: connected heads omitted from it are disabled, and each included
head carries explicit transform and VRR values. A client cannot infer those
current values when changing only a mode. Clients must require explicit values
for each included head rather than guess normal rotation or disabled VRR.
Inspection must distinguish unavailable current settings from supported values.
Adding observable current settings requires a separately specified owner and
contract change; the reserved bytes remain zero in revision 1.

The largest body is 24 + 16×104 + 2,048×24 + 16×84 = 52,184 bytes; its complete
record is 52,216 bytes. Counts and exact total length are checked before row
allocation. A published snapshot is authoritative, so it must satisfy snapshot
invariants during decoding; proposals still receive semantic validation by the
owner.

## Proposal rows

The body is a 24-byte prefix, `head_count` 32-byte head-target rows, then
`group_count` 76-byte group rows. Counts are 1..16 each, checked before
allocation. The largest body is 1,752 bytes; its record is 1,784 bytes.

Prefix: nonzero domain transaction u64, nonzero base topology epoch u64,
intent u16 (1 validate-only, 2 apply), primary group index u16, head count u16,
group count u16. The domain transaction differs from the file submission ID
and journal sequence. The connection epoch comes from the header.

Head-target row: head u64, head generation u64, mode u64, transform u16,
VRR policy u16, reserved u32=0. Transforms 1..8 are normal, rotate90,
rotate180, rotate270, flipped, flipped90, flipped180, flipped270. VRR policies
1..3 are disabled, automatic, always.

Group row: output u64 (zero requests a new identity), x/y/width/height i32,
member count u16 (1..4), reserved u16=0, then four 12-byte member slots.
A used slot contains head u64, mapping u16 (1 fit, 2 cover, 3 exact), reserved
u16=0. Every byte in an unused slot is zero.

Head identities, generations, modes, primary index, signed geometry and group
membership reach `OutputTopologyCandidate::validate_against`. The codec does
not consult a snapshot. This retains specific owner refusals for stale
topology, unknown heads/modes, invalid geometry and unsupported features,
including the existing rule that a refused domain transaction cannot be reused.
Zero head/generation/mode values are structurally representable and are refused
by that semantic validation. Unknown enum encodings are malformed records.

## Terminal outcome

Outcome has a 24-byte body: nonzero domain transaction u64, nonzero topology
epoch u64, outcome u16, reason u16, reserved u32=0. The connection epoch comes
from the event header. Outcomes 1..6 are validated, committed, stale, rejected,
rolled-back and failed. Reasons retain the existing open u16 vocabulary;
unknown reason codes are preserved and never imply success.

## Evidence scope

Literal byte fixtures check layout independently of the encoders. Negative
tests cover each truncated prefix, reserved and unused member bytes, identity
classes, enum/count bounds, maximum candidate and topology sizes, exact mode
table coverage and authoritative snapshot invariants. Owner tests feed native
decoded negotiation and proposals into `OutputConnectionState`, including
semantic refusal and domain replay. These are deterministic codec/admission
checks, not an independent-language client exchange, export custody proof or
native display acceptance.

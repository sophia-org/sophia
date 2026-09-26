# Shell files over 9P2000.L

Status: **accepted contract (t251, accepted by the operator on 2026-09-26).**
Nothing here is implemented yet; t252 implements it. `sophia_shell_v1` over its
existing socket remains the only shell transport and the installed default
until t252's gates pass. The items under "Open decisions" stay open and do not
block t252. Source references
are to the tree this draft was written against (signed `2f9c2220`, based on
`11d6deef9`).

The contract changes the transport of the shell role, not its semantics. Every
operation keeps its current owner, admission, bounds and receipt meaning. It
adds no file-descriptor passing, no GPU grant and no new desktop feature, and
it covers only direct Unix-socket clients. Mounted access has its own open
contract in the [control bus](sophia-9p-control-bus.md).

## Current owners, versions and grants

### Protocol and revisions

`protocol/sophia-shell-v1.kdl:1` declares frame-version 1, interface-major 1,
interface-revision 8, max-descriptors 16, max-label-bytes 128,
max-pending-activations 16 and max-shortcuts 256. The content design is ADR
[6ndjwffd](notes/decisions/6ndjwffd-content-capability-design-for-sophia_shell_v1.md).
The GPU permission is ADR
[mn4mzcnf](notes/decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md). Capability bits are revision-gated:

| Bit | Capability | Revision | Requires |
| --- | --- | --- | --- |
| 0 | descriptor switcher | r1 | |
| 1 | work-area reservation | r1 | |
| 2 | tab groups | r2 | |
| 3 | shortcut catalog | r3 | |
| 4 | reference sheet | r3 | |
| 5 | application catalog | r4 | |
| 6 | application launcher | r4 | 5 |
| 7 | content surface | r5 | |
| 8 | content discrete input | r5 | 7 |
| 9 | view indicators | r6 | |
| 10 | indicator activation | r6 | 9 |
| 11 | native launcher | r7 | 5, 7, 8 |
| 12 | persistent catalog | r8 | 1, 5, 7, 8 |

A client's required capabilities are a requirement: an unavailable bit is
refused, never silently downgraded (`docs/sophia-shell-v1-direction.md`).

### Admission and grants

| Fact | Current owner |
| --- | --- |
| At most three components; roles Bar, ApplicationLauncher, Dock | `crates/sophia-config/src/shell_components.rs` (`MAX_SHELL_COMPONENTS`) |
| Role to content store profile: Bar = Legacy, Dock = PersistentCatalog, Launcher = NativeLauncher | `crates/sophia-session/src/shell_component_connections.rs:170-172` |
| Negotiation per profile: Bar up to r6 with bit 0; Launcher exactly r7 with bits 5, 7, 8, 11; Dock exactly r8 with bits 1, 5, 7, 8, 12 | `crates/sophia-runtime/src/shell_transport/negotiation_policy.rs`, `native_launcher.rs`, `catalog_candidates.rs` |
| One private 0700 endpoint per component, one active peer, protected launch under Bubblewrap | `crates/sophia-runtime/src/policy_socket.rs`, `crates/sophia-session/src/live_session/metadata_shell/component_session.rs` |
| Peer admission by supervisor evidence; the evidence "is a declaration the supervisor makes, not a proof" | `policy_socket.rs:270-274` (`authorize_protected_peer`) |
| One content epoch registry for all components: 64 MiB, three active epochs, sixteen retained | `shell_component_connections.rs:86`; `crates/sophia-runtime/src/shell_content/epoch_registry.rs:59-61, 93-95` |
| Legacy descriptor shell (Narthex): one endpoint, mutually exclusive with components | `crates/sophia-session/src/live_session/metadata_shell.rs` |
| Direct GPU: a separate per-component grant; content and GPU permissions do not imply each other | ADR `mn4mzcnf`, `live_session/metadata_shell/gpu.rs` |

### Content limits

`ContentLimits::prototype` (`crates/sophia-protocol/src/ipc/shell_content/limits.rs:283-340`)
is the starting grant. `role_limits` (`shell_component_connections.rs:378-394`)
lowers it per role: with a dock present, staging is 4 MiB, resident 12 MiB for
the bar and 8 MiB otherwise, and retiring 8 MiB. A launcher without a dock gets
staging 4 MiB, resident 12 MiB and retiring 8 MiB. The values this contract
depends on:

| Limit | Prototype |
| --- | --- |
| `max_resource_bytes` | 4 MiB |
| `max_staging_bytes` / `max_resident_bytes` / `max_retiring_bytes` | 8 / 16 / 16 MiB |
| `max_session_retiring_bytes` | 64 MiB |
| `max_frame_payload` / `max_chunk_bytes` | 65536 / 65488 |
| `max_live_resources` / `max_resource_ids` / `max_open_transfers` | 64 / 4096 / 4 |
| `max_candidate_bytes` / surfaces / placements / targets | 8192 / 8 / 32 / 64 |
| `max_control_records` / input queue / output queue | 64 / 128 KiB / 256 KiB |
| transfer / transfer idle / candidate / prepare / present timeout | 2000 / 500 / 1000 / 1000 / 2000 ms |
| permit timeout / action-ack timeout / peer write | 250 / 1000 / 2000 ms |
| candidate rate | 120 Hz |

### Resource custody

`ContentResourceStore` (`crates/sophia-runtime/src/shell_content/resources.rs`)
owns every transfer: `begin` (200), `chunk` (274), `end` (309), `cancel` (353),
`expire` (391), `lease` (407), `retire` (428), `collect` (453) and `revoke`
(486). Staging, resident, retiring and backing bytes are charged separately.
A renderer holds a lease. `ResourceReleased` is sent once, when no consumer
remains. Revocation aborts incomplete transfers and keeps referenced storage.

No descriptor crosses the shell wire today. There is no SCM_RIGHTS, memfd,
DMA-BUF or sync file. Pixels are premultiplied BGRA copied in-band
(ADR 6ndjwffd §4). Lom and Provlita render on the GPU, read back, and upload
bytes.

## Operation matrix

Each current message maps to one file operation. The receipt meaning does not
change. "Record" means a complete binary record in the envelope below.

| Current kinds | Direction | File operation | Owner and receipt meaning |
| --- | --- | --- | --- |
| 96 ClientHello | C to S | `Negotiate` candidate record via `transaction` + `submit` | Negotiation policy per profile; refused unless the exact role profile intersects |
| 97 ServerWelcome, 160 AdmissionRefused, 161 ContentLimits | S to C | `Negotiated` or `Refused` event; `limits` object | Selected revision, epoch, capabilities; refusal reasons 1-4 then revocation; limits immutable per grant |
| 98 DescriptorSnapshot | S to C | `descriptors` snapshot object plus event | Broker shell sources; at most 16 rows |
| 99 Candidate, 107 TabsCandidate, 112 ReferenceCandidate, 118 LauncherCandidate | C to S | Candidate records | Existing validators; outcomes 100/113/119 as events |
| 101 Activation, 102 ActivationAck; 120/121 | both | Activation event; ack record | Presented candidate and recipient epoch required |
| 103-106 Tabs, 108-110 Shortcuts, 114-116 plus 202 Catalog, 181-184 Indicators | S to C | `tabs`, `shortcuts`, `catalog`, `indicators` snapshot objects plus event | Each Begin/Entry/End transfer becomes one immutable object |
| 111 ReferenceRequest, 117 LauncherRequest | S to C | Events | Unchanged |
| 122 LaunchOutcome | S to C | Event | Started means process creation only |
| 162 ContentOutputFacts | S to C | `outputs` snapshot object plus event | At most 16 outputs |
| 163 AllocationRequest, 164 AllocationResult; 188 | both | Allocation record; result event | Allocation owner; granted/rejected/released/invalidated |
| 165 ResourceBegin | C to S | `ResourceBegin` record binding an `upload/N` slot | `ContentResourceStore::begin`; `transfer_admitted` charges staging |
| 167 ResourceChunk | C to S | Writes by the slot's bound writer fid | `chunk`, fed canonical chunks of exactly `rows_per_chunk` rows (the last may be shorter) |
| 168 End, 169 Cancel, 170 Retire | C to S | Records; a clunk before End also cancels | `end` then `accepted`; `cancel`; `retire` |
| 166 ResourceStatus, 171 ResourceReleased | S to C | Events | Accepted means validated and stored; Released is sent once, when no consumer remains |
| 172-174 Candidate; 189-190, 198-199 | C to S | One complete candidate record per submit (at most 8192 bytes) | A pacing permit is required; Begin/Chunk/End collapse into one record |
| 175 CandidateOutcome | S to C | Event | Prepared, presented (exact candidate retired on its output), rejected, superseded |
| 176 FrameDemand, 178 DemandCancel, 177 FramePermit | both | Records; permit event | Permit TTL 250 ms; rate 120 Hz |
| 179 ContentAction, 180 ContentActionAck | both | Event; ack record | No coordinates cross |
| 185 IndicatorActivate, 186 outcome | both | Record; event | Exact snapshot action echo |
| 187 Opening, 191 Focus, 192 FocusRevoked, 193 Input, 197 Closed | S to C | Events | Focus lease minted by Session only after an actual Presented |
| 194 InputAck, 195 Activate | C to S | Records | Consumed or stale; activation causes keyboard or content action |
| 196, 201 ActivationOutcome | S to C | Events | Admitted means a queue slot only |
| 200 CatalogActivate | C to S | Record | The action names a catalog slot, never a command |

The launch context has no wire field. Session reads the committed WM output
launch context when a launch is queued
(`crates/sophia-session/src/session_actions/native_catalog.rs`). That stays
unchanged.

## Proposed export

### One export per component

Each component endpoint serves its own 9P export. The endpoint keeps today's
directory, socket, supervisor PID evidence, role profile and Bubblewrap
binding. The export admits one attach for each admitted connection epoch,
following the WM rule in [WM files](sophia-wm-files.md). There is no shared
tree, and there are no paths into another component's export. A replacement
process gets a fresh epoch. Each component keeps its own logical qid
allocator, which continues across epochs. The legacy descriptor shell is a
separate export profile on its own endpoint.

Paths, attach names, UIDs, fids and qids grant nothing. Authority comes from
the endpoint's admitted peer and the role profile Session fixed before the
peer connected.

### Root vocabulary

The root is fixed per role profile and listable with `TREADDIR`. A name requires
both the role's existing disclosure permission and its negotiated capability.
The component bar keeps selected bit 0 for negotiation parity, but that bit is
inert today: only the separate legacy descriptor shell receives descriptor
snapshots. The component bar therefore exposes no `descriptors`, `tabs` or
`shortcuts` nodes or feeds. `ContentStoreProfile::Legacy` on the bar does not
turn it into the legacy descriptor shell. No capability bit alone widens a
component's metadata audience.

| Name | Access | Profiles | Meaning |
| --- | --- | --- | --- |
| `api` | read | all | Small immutable text: family `sophia_shell_fs_v1`, API version, role profile, `fd_transfer=none` |
| `limits` | read | content profiles | The granted `ContentLimits` as a binary record, immutable for the grant |
| `events` | read | all | Ordered records by byte offset, retained until acknowledged |
| `transaction` | read/write | all | The attach's single candidate buffer; at most one open `transaction` fid per attach, as in the WM contract |
| `submit` | write | all | Submits the attach's staged candidate by epoch, submission ID and exact length; it names no fid |
| `ack` | write | all | Acknowledges events through a sequence number |
| `outputs` | read | content profiles | Pinned output facts object |
| `catalog` | read | launcher, dock; legacy descriptor when r4 and bit 5 are selected | Pinned catalog object, with r8 identities for the dock |
| `descriptors`, `tabs`, `shortcuts` | read | legacy descriptor | Pinned feed objects |
| `indicators` | read | bar with bit 9 | Pinned indicator object |
| `upload/0` .. `upload/N-1` | write | content profiles | Fixed transfer slots; N is `max_open_transfers` (4 in the prototype) |

Snapshot objects follow the WM snapshot rule. An event names each object's
generation and qid. Opening pins the object current at open time, and a later
object never aliases an open pin. An object that has not yet been published
answers `EAGAIN`.

An announced qid is not kept forever. Per feed, Session retains only the
current object and at most one older object still pinned by an open fid.
Each attach holds at most one open pin per feed; a second open while a pin is
held returns `EBUSY`. Publication continues while a pin is held; the
old pin stays immutable and the epoch fences it. Object identity (qid)
changes whenever the bytes change, even at an unchanged domain generation
(indicators can republish focus at the same generation).

The retention rule requires this invariant:

- the opened object's generation and qid are reported through `getattr`;
- a client whose opened object does not match the event it is handling must
  clunk, reopen, and continue from the newest event. `ESTALE` below the
  retention floor means: re-read every disclosed snapshot object and resume
  from the newest event sequence;
- a mismatch never authorises anything. Every activation, candidate or action
  names the exact generation and slot identity it acts on, and the existing
  owner validates that against current state, rejecting stale references as it
  does today.

The encoded snapshot cap per component is the sum over its role's disclosed
feeds of 2 x cap plus one shared 4 MiB build scratch. These are encoded-buffer
bounds, not RSS; allocator capacity, metadata and queues are accounted
separately. The per-object caps are: catalog 4 MiB, tabs 1 MiB, shortcuts
128 KiB, indicators 32 KiB, descriptors 4 KiB, outputs 1 KiB.

The r8 catalog maximum is 3,014,740 bytes in old framing, and 3,145,876 bytes
conservative with headers, which fits the 4 MiB cap. The new codec must
enforce count, row, and header bounds independently.

### Negotiation is a candidate, not a node

The WM contract negotiates through a submitted candidate. This draft does the
same rather than adding a `negotiate` file. The client submits a `Negotiate`
record carrying what today's Hello carries: minimum and maximum revision and a
required capability mask. Session applies the role's fixed profile with the
same rules as today. Version 1 adds no optional-capability mask: that would
change shell negotiation rather than transport it. The result is one
`Negotiated` event with the selected revision, epoch, capability set and
limits generation, or a `Refused` event with the current reason (1 permission
denied, 2 unsupported, 3 invalid dependencies, 4 unavailable) followed by
revocation.

There is exactly one selection per epoch. Replaying the same submission ID
replays its Submitted custody and cannot negotiate again. A separate node
would have to repeat the WM's custody, replay and epoch rules for a single
record, so it adds nothing.

### Records and submission

The envelope and custody rules are the WM file rules. The header holds total
bytes, API version, kind, epoch, submission ID and event sequence. A
submission ID rises strictly within an attach. `Submitted` records custody
only, and the semantic outcome follows as its own event. An identical submit
before acknowledgement returns success without re-enqueueing, and replay at or
below the watermark is `EALREADY`. A submit refused with `EAGAIN` has
transferred nothing.

Candidate Begin/Chunk/End collapse into one complete record, which stays
within `max_candidate_bytes` (8192 bytes, `crates/sophia-protocol/src/ipc/shell_content/limits.rs:309`). As in the WM contract, each attach has one
candidate buffer, and `submit` refers to that attach's staged candidate. The
buffer therefore needs no more than the largest control record, not the WM's
1 MiB. The shell transaction cap is 64 KiB per attach transaction buffer, equal to
`max_frame_payload` (65,536 bytes, `crates/sophia-protocol/src/ipc/shell_content/limits.rs:294`).

### Resource staging without client-created files

The `sophia-9p` core performs no create, mkdir or remove: Tlcreate, Tmkdir and
Tunlinkat are refused with `EOPNOTSUPP` (`crates/sophia-9p/src/wire.rs`). The
client therefore never names a new file. Uploads use the fixed `upload/N`
slots.

1. The client submits a `ResourceBegin` record naming the resource ID
   (`ContentResourceId`: id and generation, scoped to the grant;
   `crates/sophia-protocol/src/ipc/shell_content.rs:28-33`), the slot index and
   the description (size, stride, scale), as the current 165 record does. The
   adapter calls `ContentResourceStore::begin`, which admits the transfer and
   charges staging exactly as today. The slot is now bound to exactly
   (content grant, `ContentResourceId`). The `transfer_admitted` status arrives
   as an event. Begin on a slot that is still bound is refused.
2. The client opens the slot for writing. Read and read/write opens are refused.
   The first fid opened for writing on a
   bound slot becomes its only writer. Any other open for writing answers
   `EBUSY`. Writes append at the exact next byte offset (see chunking below).
   Gaps, repeated prefixes, overflow and requests past the declared length
   refuse before changing bytes.
3. `ResourceEnd` submitted for that `ContentResourceId` calls `end`. The
   `accepted` or `rejected` status is an event.
4. `ResourceCancel`, or a clunk of the current writer fid before End, calls
   `cancel`. The transfer and idle timeouts (2000 and 500 ms) still expire it
   through `expire`.
5. `ResourceRetire` is a record. `ResourceReleased` is an event sent exactly
   once, when the last lease drops.

A binding ends at End, cancel, expiry or revocation. When it ends, every fid
opened on the slot under that binding is fenced: further reads and writes
answer `ESTALE`, and its later clunk only releases the fid. Only the current
binding's writer fid can cancel by clunking, so an old fid can never cancel a
successor bound to the same slot. An unbound slot answers `EAGAIN` to open. A
slot bound in another epoch is stale (`ESTALE`). The slot index is transport
bookkeeping, not authority.

`getattr` on the live bound slot reports its accepted append cursor as size:
canonical bytes already passed to the store plus bytes in charged partial
scratch. It reports the binding's qid, not a successor's. After a flush race,
the writer can query that same binding and resume at this offset. Flush never
undoes an executed write; repeating an earlier prefix is still refused.
If the binding ended, the held fid is stale and cannot discover or append to
a successor. This metadata read grants no pixel readback.

One slot carries at most `max_resource_bytes` (4 MiB), which exceeds the WM's
1 MiB transaction bound. Total store staging stays bounded by the role's
`max_staging_bytes` (4-8 MiB), charged at Begin.

**Chunking.** The store accepts only canonical chunks: each chunk must be
exactly `rows_per_chunk * row_bytes` bytes, the last one the remainder, at the
next ordinal and offset (`crates/sophia-runtime/src/shell_content/resources.rs:286-298`).
`rows_per_chunk` is `min(max_frame_payload - 48, max_chunk_bytes) / row_bytes`
(`crates/sophia-protocol/src/ipc/shell_content/validation.rs:393-399`). A 9P
write can split anywhere. After validating the binding, offset and entire
declared request range, the adapter accepts at most the prefix completing the
current canonical chunk. A positive short `Rwrite` reports that prefix; the
client advances by the returned count. Incomplete bytes stay in one reusable
chunk buffer. A completed chunk goes to `chunk` once, and successful admission
clears the buffer for reuse. The core already supports positive short writes
(`crates/sophia-9p/src/connection.rs:695-719`).

Malformed canonical bytes, including invalid premultiplied pixels, cause the
store to abort the transfer. The adapter fences the binding, drops its scratch
and retains the existing rejection event; that write returns an error.
Limiting a write to one chunk boundary prevents the error from hiding earlier
successful chunks from that same write. Earlier successful writes may still
end in resource rejection: `Rwrite` proves byte custody, while successful End
proves resource acceptance. Incomplete End keeps the owner's terminal
`Incomplete` result. Process expiry and terminal outcomes before another
write. Only a successful canonical chunk refreshes the idle deadline; partial,
empty and rejected writes do not. The overall deadline never extends.
A client trickling less than one canonical chunk can therefore expire with
partial bytes buffered; those writes cannot keep a transfer alive indefinitely.

Scratch is a separate transport charge, not part of the resource store's
staging allowance. The file export reserves
`max_open_transfers * min(max_frame_payload - 48, max_chunk_bytes)` bytes at
admission: 261,952 bytes for the prototype, under 768 KiB across three active
component exports. Before accepting Begin custody or calling the store,
acquire the slot buffer and response capacity; an early capacity refusal
leaves the store untouched. Once `begin` runs, its existing generation and
failure semantics apply. The buffer is released at binding termination; a
retained pool remains charged. Revocation releases transport scratch without
releasing renderer-held content. This preserves admission of a 4 MiB resource
when the role's staging grant is exactly 4 MiB.

The resource registry reserves staging, resident and retiring storage; it has
no adapter scratch API (`shell_content/epoch_registry.rs:135-153`). Transport
input/output accounting is already separate (`shell_transport/accounting.rs`).
The file adapter therefore owns and reports this additional bounded charge.
It never retains another copy of the staged prefix. Version 1 offers no upload
readback or prefix replay: staging bytes are private to the resource owner,
and its `lease` API is for accepted resources with real consumers. No new
staging-read API is needed.

### Events, snapshots and acknowledgement

The rules are the WM journal's: strictly increasing sequences, byte offsets,
whole records, reads that block at the tail, `EINVAL` past it and `ESTALE`
below the retention floor. Acknowledgement releases transport retention only.

Shell traffic is denser than WM traffic: frame permits, candidate outcomes and
resource statuses. A component journal holds at most 256 records. 64 of them
are the terminal reserve, equal to `max_control_records`
(`crates/sophia-protocol/src/ipc/shell_content/limits.rs:317`, enforced in aggregate
by `crates/sophia-runtime/src/shell_transport/control_budget.rs:18-41`). Only
records that already hold a counted credit may use the reserve, so every
promised response always has space. The other 192 hold unsolicited Session
events (snapshot announcements, allocation invalidation, focus revocation,
opening, content actions, closed) and unacknowledged history.

Byte bounds are derived from the largest Session-to-client record of each role
profile. The file envelope's 32-byte header (as in [WM files](sophia-wm-files.md))
replaces the 24-byte frame header, adding 8 bytes per record. The journal byte
bound is 256 times that record, rounded up to the next power of two and capped at
1 MiB. The terminal reserve is 64 times the largest terminal record. Snapshot
objects are not journal records; their events only name the object.

To bound a stalled reader, `journal_ack_progress_timeout` is 2000 ms. It is a
separate constant from the 2000 ms peer-write timeout, which the socket layer
owns; the same value treats a stalled reader like a stalled socket. When Session
must append an unsolicited record and the non-reserve part of the journal is
full, it waits for acknowledgement progress. If no acknowledgement advances
within the deadline, that component alone is closed and revoked, as saturation
does today. A reader that keeps acknowledging is never closed by this rule.
Credited records never wait. There is no fake acknowledgement, overwrite,
invented outcome or dropped owed record.

### Per-role disclosure bounds

The root names come from the vocabulary above. Snapshot bound is twice the sum of
the disclosed feeds' caps (current plus one pinned older object) plus one
shared 4 MiB build scratch.

| Role profile | Root names | Snapshot feeds and caps | Largest S-to-C record (file framing) | Journal bytes | Terminal reserve | Snapshot bound |
| --- | --- | --- | --- | --- | --- | --- |
| Bar (Lom, r6) | `api`, `limits`, `events`, `transaction`, `submit`, `ack`, `outputs`, `upload/N`; `indicators` with bit 9 | outputs 1 KiB; indicators 32 KiB | AllocationResult, 192 B (`crates/sophia-protocol/src/ipc/shell_content/fields.rs:332-351`) | 65,536 (256 x 192 = 49,152, rounded) | 12,288 | 4,261,888 |
| Launcher (Bemenu, r7) | `api`, `limits`, `events`, `transaction`, `submit`, `ack`, `outputs`, `catalog`, `upload/N` | outputs 1 KiB; catalog 4 MiB | native Input, up to 420 B (`crates/sophia-protocol/src/ipc/shell_native_launcher/records.rs:100`) | 131,072 (256 x 420 = 107,520, rounded) | 26,880 | 12,584,960 |
| Dock (Provlita, r8) | `api`, `limits`, `events`, `transaction`, `submit`, `ack`, `outputs`, `catalog`, `upload/N` | outputs 1 KiB; catalog 4 MiB with r8 identities | AllocationResult, 192 B | 65,536 | 12,288 | 12,584,960 |
| Legacy descriptor (Narthex, r1-r8) | `api`, `events`, `transaction`, `submit`, `ack`, `descriptors`, `tabs`, `shortcuts`; `catalog` when r4 and bit 5 are selected | descriptors 4 KiB; tabs 1 MiB; shortcuts 128 KiB; catalog 4 MiB when selected | LauncherRequest, 342 B (`crates/sophia-protocol/src/ipc/shell_launcher.rs:130-146`; query at most 256 B, `crates/sophia-protocol/src/packets/shell_launcher.rs:8`) | 131,072 (256 x 342 = 87,552, rounded) | 21,888 | 6,561,792; 14,950,400 with catalog |

The bar and dock record sizes come from the terminal-debt inventory (184-byte
AllocationResult and up to 412-byte native Input in today's framing, plus 8).
For the legacy descriptor profile, the records that become snapshot objects
(descriptor snapshots of at most 3,084 bytes framed, tabs, shortcut and application
entries, catalog identities) are excluded; the largest record that stays a journal
event is the launcher request. Its reserve is sized on that record rather than on a
separately derived terminal record, which over-reserves.

Note: The component bar's inert bit 0 discloses no descriptor, tab, or shortcut feed.

Catalog objects can exceed the WM's 1 MiB snapshot bound: 4096 entries with
128-byte labels and 256-byte keywords. Content actions, focus revocation, input-lease
loss and allocation invalidation remain local Session transitions. They never wait
for a reader's acknowledgement credit.

### Record kinds and correlation (t252 first slice)

The byte layouts are in
[`sophia-shell-files-v1.kdl`](../protocol/sophia-shell-files-v1.kdl) and
`sophia_protocol::shell_files`. The kind table is closed, and it grows only with
the operations t252 implements:

| Class | Kinds |
| --- | --- |
| Object | `Limits` 1 |
| Event | `Negotiated` 16, `Refused` 17, `Submitted` 18, `AllocationResult` 32 |
| Candidate | `Negotiate` 256, `AllocationRequest` 257 |

As in the WM contract, the header never carries a domain identity: events
have submission ID zero. A content record's body starts with its nonzero
transaction ID, followed by the existing `sophia_shell_v1` payload bytes of the
same record, so every existing record check applies unchanged. The owner's
event for that transaction carries the same ID back. The transaction ID is the
domain correlation, independent of the submission ID, which records custody
only. A record family not yet in the table is not carried: a component on the
file wire whose owner queues such a record is closed rather than sent an IPC
frame.

## Multiple writers, isolation and revocation

Components never share a writer or an export. Admission stays
`authorize_protected_peer` plus the exact-profile negotiation. Because that
evidence is a supervisor declaration, the t133 admission review applies to
every shell export.

`stop()` revokes the export: every held fid and waiting read answers `ESTALE`,
as `WmFiles::revoke` does. The socket then closes. The
retirement-claim settlement gate still precedes restart. Retained epochs (at
most sixteen) drain as they do today.

Revoking a grant removes its pending native catalog launches and withdraws an
admitted native launch whose single execution attempt has not begun
(`revoke_native_catalog_grant`,
`crates/sophia-session/src/session_actions/native_catalog.rs:275-296`). A
returned worker payload may stay alive, but it loses execution authority.
Execution is permitted only by `begin_native_catalog_execution`, which consumes
the one attempt and sets `native_execution_attempted` (:227-246). Once that
flag is set, revocation cannot recall a spawn already begun. The file
transport preserves exactly this and adds no cancel or execute authority of its
own.

Controls to port from `crates/sophia-session/tests/shell_component_connections.rs`:
component impersonation, three-role admission and a foreign grant. Controls to
add for 9P:

- an attach on another component's socket;
- a submit replayed across epochs;
- a stale snapshot object or upload slot after replacement;
- one component's slow reader while the others progress;
- a disconnect with a bound, partially written slot (staging released,
  referenced storage retained);
- revocation while a candidate is presented.

## File descriptors and GPU

Version 1 carries bytes only. 9P has no descriptor transfer, and a v9fs mount
could not carry one. Using SCM_RIGHTS on the same socket would be a side
channel outside the protocol. Pixels stay copied exactly as today.

A 4 MiB resource needs about 64 writes at a 64 KiB msize, compared with 65
chunks today. DMA-BUF or sealed memfd import remains a separate future
contract (`docs/sophia-shell-v1-direction.md`, ADR `mn4mzcnf`). It would need
its own admission, GPU-grant coupling and retirement. The GPU grant is
unchanged and still cannot retract an already-open render-node descriptor.

## Compatibility and revision skew

| Client | Role | Revision | Codec and transport seam | Notes |
| --- | --- | --- | --- | --- |
| Lom | bar | r6 | Sophia's `sophia-shell-client`, pinned to git `2e569301` | Moving that crate's transport moves Lom; not independent evidence |
| Bemenu (`bemenu-sophia`) | launcher | r7 | Vendored Sophia C `shell_wire`, manifest-pinned to `sophia-stack` `c2ff3fcd`; I/O in `shell_wire/io.c` and `frame.c` | Frame kinds are hard-coded in `connection_receive.c` |
| Provlita | dock | r8 | Sophia's `sophia-shell-client` through path dependencies on `../sophia-stack` | **Cannot build as-is**: that directory does not exist |
| Narthex | legacy descriptor reference | r1-r9 | Its own Nim codec in `src/wire/*`; socket I/O in four procedures in `src/narthex.nim` | Independent, but sends no content, allocations or resources |

Narthex offers revision 9 with overview capability bit 13 (`src/types/shell_overview.nim:2-3`).
That capability exists only on Sophia's unmerged `overview` branch
(`cf1c33ed2`, `46dfc4da8`), not in this base. Against this base, Narthex must
negotiate at most r8. The file profile carries the same per-role revisions as
today, so skew is resolved by the same negotiation, not by the transport.

No client reconnects in-process. Each relies on supervisor restart with a
fresh process, which matches one attach per epoch.

Current IPC remains the default. Session-owned configuration selects transport
per component at startup; the mutually exclusive legacy descriptor shell has
its own selection. Clients and inherited environment do not choose the
server's protocol, and there is no sniffing or fallback. Mixed transports can
use the same one `ContentEpochRegistry`; selection neither creates another
budget nor changes a role's grants.

A transport change is a complete component replacement: stop, revoke, settle
the existing retirement claims, then issue a fresh grant and connection epoch.
It never migrates a live grant. Until a reload owner implements that complete
transition, a reload requesting a transport change must refuse it and retain
the startup selection. Explicit Session relaunch is the rollback path; an
installed-default change remains a separate acceptance decision.

## Independent clients and evidence

No independent client covers content today. Lom and Provlita use Sophia's own
library, Bemenu uses Sophia's own C binding, and Narthex covers descriptors
only. t252 therefore needs one independently written file client for the
content profiles. Upload alone (r5) is not enough; it must cover both the r7
launcher and the r8 dock profiles:

- negotiation for each exact profile;
- allocation;
- slot upload, including split, cancelled and fenced writes;
- candidates and pacing;
- action acknowledgement;
- r7: native opening, focus lease, semantic input, activation and close;
- r8: catalog snapshot with identities, and catalog activation by generation
  and slot.

The independent Go oracle will carry these scenarios, written from
`protocol/sophia-shell-v1.kdl` and this file contract without Sophia codec reuse.
Its test admission is supplied, so it cannot prove supervisor authentication.
The product clients then prove integration, not independence; Narthex remains
the descriptor reference rather than acquiring content work for this gate.
Because Provlita cannot build as-is (due to missing `../sophia-stack` path
dependencies), its r8 dock bounds (catalog with r8 identities at the 4 MiB cap,
per-output allocations and reservations, journal and snapshot bounds) are proven first through the
independent Go oracle's r8 profile. Provlita's own integration evidence requires
repairing its dependency pin, which is a prerequisite recorded here and not a
transport change.

Required evidence follows the control bus's five retirement criteria, per
profile:

- wire conformance, including malformed records, partial writes and flush;
- equivalent admission, disclosure, receipts, presented input, reconnect and
  retirement through the production owners, with compiled negative controls;
- independent clients;
- measured performance against current IPC;
- a rollback and compatibility path.

## Per-role acceptance and work matrix

The transport must reproduce each behaviour below through the existing owner.
Rows marked as product gaps belong to their own tasks, not to this transport.

| Role | Must behave as today over files | Product gaps, not transport |
| --- | --- | --- |
| Dock (Provlita, r8) | Per-output allocations and edge reservations, checked against the allowed reservation extent. Pinned tiles from the catalog, with r8 identities at the 4 MiB cap. Activation names the catalog generation and slot and is accepted only against the Presented target. Launch context taken by Session from the committed WM output context, refused when stale. Replacing the dock does not disturb bar or launcher. Retained dock content retires after revocation, and storage is reclaimed. | Running-window feed (t043); reservation arbitration (t106) |
| Launcher (Bemenu, r7) | Opening from Session; parentless allocation with no reservation. Focus lease minted only after an actual Presented, and FocusRevoked on loss. Semantic input with stale acknowledgements. A query edit disarms activation. Activation admits a queue slot only; the revocation semantics above hold. Close, and the opening timeout. | Popout workflow (t099) |
| Bar (Lom, r6) | Panel allocation per output with its reservation. Indicator snapshot and exact activation echo. Presented work-area bands survive reconnect until the new first Present. Content upload and retirement within role limits. | Recovery (t100) |
| Legacy descriptor (Narthex, r1-r8) | Descriptor snapshot, candidate, activation and ack; tabs; shortcuts and reference; launcher catalog when r4 and bit 5 are selected. Reservation via candidate, and withdrawal. | Overview r9 exists only on the unmerged `overview` branch |

## Budgets

These are the accepted acceptance budgets, not measurements. They are stated
before measurement and cannot be relaxed after a result.

| Measure | Budget |
| --- | --- |
| Panel repaint: submit of a bar-sized resource (for example 1920x24, 180 KiB) to `accepted` | p95 within current IPC + 1 ms, p99 within + 2 ms |
| 4 MiB resource upload to `accepted` | p95 within current IPC + 10%; no timeout at the 2000 ms transfer bound |
| Candidate submit to Presented | p95 within current IPC + 1 ms, p99 within + 2 ms, at 60 and 120 Hz |
| Frame demand to permit | p99 below half the 250 ms permit TTL |
| CPU per uploaded MiB and allocations per frame | no more than current IPC + 10% |
| Failures | no timeout, disconnect or unbounded queue growth |

Each distribution reports p50, p95 and p99, the maximum, and every timeout,
with the same client, workload and output on both transports.

## Open decisions

- The `AllocationResult` event `status=4` (invalidate) preserves today's semantics in v1:
  it is Session-initiated, holds no pre-reserved credit (`crates/sophia-runtime/src/shell_content/allocations.rs:379-407`),
  and uses the non-reserve journal space. A guaranteed variant would reserve +16
  (`max_allocations_total`).

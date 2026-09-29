# Shell files over 9P2000.L

Status: **accepted contract (t251, accepted by the operator on 2026-09-26).**
The content and descriptor exports are implemented. Descriptor contract
acceptance is recorded under t271; remaining IPC compatibility retirement is
tracked separately. The items under "Open decisions" stay open. Source references
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
| Descriptor component: one 9P endpoint, mutually exclusive with independent content components | `crates/sophia-session/src/live_session/metadata_shell.rs` |
| Direct GPU: a separate per-component grant; content and GPU permissions do not imply each other | ADR `mn4mzcnf`, `live_session/metadata_shell/gpu.rs` |

### Content limits

`ContentLimits::prototype` (`vendor/rust-desktop-sdk/source/crates/sophia-shell-protocol/src/shell/content/limits.rs`)
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

The file wire uses `max_chunk_bytes` directly for upload chunks. The
`max_frame_payload` and `max_input_queue_bytes` fields remain in the Limits
layout only for socket compatibility; their existing scalar bounds and
`+24`/`+48` validation relationships remain mandatory. Production grants keep
their prototype values, 65,536 and 131,072. Removing those fields requires a
coordinated contract and SDK change; it does not follow from ignoring them in
file owners.

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
| `api` | read | all | Small immutable text, one line: `sophia-shell-files version=<api> role=<profile> epoch=<connection epoch> fd_transfer=none`. The epoch is the value every record header of this attach carries; a client reads it after attaching, before its first submit |
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

When a catalog carries persistent identities (`identities_present=1`), each
entry has one nonempty identity name and those names are distinct by exact
UTF-8 bytes. Distinct slots cannot name the same persistent application.
Display labels and keywords do not establish identity and may repeat. A plain
launcher catalog (`identities_present=0`) carries no identity names.

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

The `Negotiated` body preserves the shell role's welcome limits:
`max_descriptors` is 1–16, `max_label_bytes` is 1–128 UTF-8 bytes, and
`max_pending_activations` is 1–16. These bounds apply to every selected profile,
including a content profile that does not consume descriptors. Both encoders
and decoders refuse values outside these ranges; a zero value is not an
unused-field marker. This makes the existing role maxima explicit on the file
wire without changing the body layout.

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
within `max_candidate_bytes` (8192 bytes in the Limits contract). As in the WM contract, each attach has one
candidate buffer, and `submit` refers to that attach's staged candidate. The
buffer therefore needs no more than the largest control record, not the WM's
1 MiB. The shell transaction cap is independently fixed at 64 KiB per attach
transaction buffer; it is not derived from the socket's frame limit.

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
`rows_per_chunk` is `max_chunk_bytes / row_bytes`. The earlier expression
`min(max_frame_payload - 48, max_chunk_bytes) / row_bytes` is equal for every
valid Limits object because `max_chunk_bytes + 48 <= max_frame_payload`
remains required. Thus the change preserves every admitted upload layout. A 9P
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
`max_open_transfers * max_chunk_bytes` bytes at
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
(enforced in aggregate by `crates/sophia-runtime/src/shell_transport/control_budget.rs`). Only
records that already hold a counted credit may use the reserve, so every
promised response always has space. The other 192 hold unsolicited Session
events (snapshot announcements, allocation invalidation, focus revocation,
opening, content actions, closed) and unacknowledged history.

Byte bounds are derived from the largest Session-to-client record of each role
profile. The file envelope's 32-byte header (as in [WM files](sophia-wm-files.md))
replaces the 24-byte frame header. These file bodies also include an 8-byte
transaction, making the whole record 16 bytes larger than the old frame.
Use the native layouts when sizing the journal. The journal byte
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
| Bar (r6) | `api`, `limits`, `events`, `transaction`, `submit`, `ack`, `outputs`, `upload/N`; `indicators` with bit 9 | outputs 1 KiB; indicators 32 KiB | AllocationResult, 200 B (32-byte header + 168-byte body) | 65,536 (256 x 200 = 51,200, rounded) | 12,800 | 4,261,888 |
| Launcher (r7) | `api`, `limits`, `events`, `transaction`, `submit`, `ack`, `outputs`, `catalog`, `upload/N` | outputs 1 KiB; catalog 4 MiB | NativeInput, 430 B (32-byte header + 398-byte body) | 131,072 (256 x 430 = 110,080, rounded) | 27,520 | 12,584,960 |
| Dock (r8) | `api`, `limits`, `events`, `transaction`, `submit`, `ack`, `outputs`, `catalog`, `upload/N` | outputs 1 KiB; catalog 4 MiB with r8 identities | AllocationResult, 200 B | 65,536 | 12,800 | 12,584,960 |
| Descriptor (r1-r8) | `api`, `limits`, `events`, `transaction`, `submit`, `ack`; selected `descriptors`, `tabs`, `shortcuts`, `catalog`, `indicators` feeds | descriptors 4 KiB; tabs 1 MiB; shortcuts 128 KiB; catalog 4 MiB; indicators 32 KiB | LauncherRequest, 352 B | 131,072 (256 x 352 = 90,112, rounded) | 22,528 | 6,561,792 with descriptors, tabs and shortcuts; add 8,388,608 for catalog and 65,536 for indicators |

All rows use native file record sizes. Descriptor feed disclosure, selected-feed
accounting and combined descriptor/content grants follow
[the descriptor contract](sophia-shell-descriptors.md). Snapshot objects do not
consume journal bytes. The descriptor terminal reserve uses its largest event,
LauncherRequest, conservatively exceeding the actual terminal-record size.

Note: The component bar's inert bit 0 discloses no descriptor, tab, or shortcut feed.

Catalog objects can exceed the WM's 1 MiB snapshot bound: 4096 entries with
128-byte labels and 256-byte keywords. Content actions, focus revocation, input-lease
loss and allocation invalidation remain local Session transitions. They never wait
for a reader's acknowledgement credit.

### Record kinds and correlation (t252)

The byte layouts are in
[`sophia-shell-files-v1.kdl`](../protocol/sophia-shell-files-v1.kdl) and
`sophia_protocol::shell_files`. The kind table is closed, and it grows only with
the operations t252 implements:

| Class | Kinds |
| --- | --- |
| Object | `Limits` 1, `Outputs` 2 |
| Event | `Negotiated` 16, `Refused` 17, `Submitted` 18, `ObjectPublished` 19, `AllocationResult` 32, `ResourceStatus` 33, `ResourceReleased` 34, `CandidateOutcome` 35, `FramePermit` 36, `Action` 37 |
| Candidate | `Negotiate` 256, `AllocationRequest` 257, `ResourceBegin` 258, `ResourceEnd` 259, `ResourceCancel` 260, `ResourceRetire` 261, `Candidate` 262, `FrameDemand` 263, `FrameDemandCancel` 264, `ActionAck` 265 |

As in the WM contract, the header never carries a domain identity: events
have submission ID zero. A content record's body starts with its nonzero
transaction ID, followed by the existing `sophia_shell_v1` payload bytes of the
same record, so every existing record check applies unchanged. The owner's
event for that transaction carries the same ID back. The transaction ID is the
domain correlation, independent of the submission ID, which records custody
only.

Output facts are the `outputs` object (at most 1 KiB: sixteen outputs fit),
not a journal event. Each publication gets a fresh qid and is announced by one
`ObjectPublished` event carrying the object kind, the facts generation and
that qid; the object and its announcement commit together, and the event's
journal room is checked before a qid is spent. Opening `outputs` pins the
current object; a second open on the same attach is `EBUSY`, and a pinned
read never changes while newer objects are published.

A `ResourceBegin` body adds the upload slot after the transaction ID (slot
`u16` and six reserved bytes, then the payload). There is no chunk record:
chunks are the slot's writes, assembled into canonical chunks as described in
resource staging. The export reserves the slot at the accepted Begin, binds it
when the store's `transfer_admitted` status is journaled, and ends the binding
when the resource's accepted, rejected or cancelled status is journaled, so the
binding follows exactly what the reader can observe. A record family not yet in the table is not carried: a component on the
file wire whose owner queues such a record is closed rather than sent an IPC
frame.

Candidates, pacing and content actions follow the same pattern.
`FrameDemand`, `FrameDemandCancel` and `ActionAck` are submitted records, and
`FramePermit`, `CandidateOutcome` and `Action` are events. Each is its
transaction ID followed by the record's value. A `Candidate` record is one
whole candidate: the transaction ID, the candidate header (grant, candidate
generation, output, facts generation, pacing permit, interaction generation),
the surface, placement and target counts as `u16` with one reserved `u16`, then
the rows. There are no chunk ordinals and no repeated counts or identities. The
record is at most 8192 bytes, header included; a candidate at the prototype
maxima (8 surfaces, 32 placements, 64 targets) fits. The candidate owner
receives Begin, Chunk and End in that order, so the permit, assembly, deadline
and outcome rules are the socket path's, unchanged. A record that is
malformed, or whose parts the value validators refuse (layout, counts,
reserved bytes, identities, per-row fields, rectangle arithmetic), is refused
at `submit` with `EINVAL`, and nothing reaches the owner. Rules that need
the owner's state or the whole candidate (target triple uniqueness,
overlapping bounds, targets inside their surface, current permits,
generations and slots) are the owner's; their outcomes are in "Negotiation
and pacing outcomes" and "Role family outcomes".

### Role family kinds (t252 B5, draft)

The launcher, dock and bar families follow the same rule: whole typed values,
no transfer shapes. Kind numbers are reserved here before implementation:

| Class | Kind | Replaces | Value |
| --- | --- | --- | --- |
| Object | `Catalog` 3 | 114-116 transfer, and 202 identities for r8 | The whole application catalog; for the dock, each entry with its r8 identity. Cap 4 MiB |
| Object | `Indicators` 4 | 181-184 transfer | The whole indicator snapshot: active output, output statuses, indicators. Cap 32 KiB |
| Event | `NativeOpening` 38 | 187 | Opening, catalog generation, state revision |
| Event | `NativeFocus` 39 | 191 | The focus lease binding, minted only after an actual Presented |
| Event | `NativeFocusRevoked` 40 | 192 | Binding and reason |
| Event | `NativeInput` 41 | 193 | Semantic input event with its text |
| Event | `NativeActivationOutcome` 42 | 196 | Activation, status and reason; admitted means a queue slot only |
| Event | `NativeClosed` 43 | 197 | Opening and reason |
| Event | `CatalogActivationOutcome` 44 | 201 | Activation, status and reason |
| Event | `IndicatorActivationOutcome` 45 | 186 | The exact activation echo, status and reason |
| Candidate | `NativeAllocationRequest` 266 | 188 | Parentless allocation, no reservation |
| Candidate | `NativeCandidate` 267 | 189, 190 and 174 | A whole candidate plus opening, catalog generation, state revision, selection and rows |
| Candidate | `NativeInputAck` 268 | 194 | Event and disposition |
| Candidate | `NativeActivate` 269 | 195 | Event, cause and slot |
| Candidate | `CatalogCandidate` 270 | 198, 199 and 174 | A whole candidate plus the catalog generation it presents |
| Candidate | `CatalogActivate` 271 | 200 | The content action and catalog generation, never a command |
| Candidate | `IndicatorActivate` 272 | 185 | Snapshot generation, output, indicator, action and event |

**Owner handoff.** These families are frame-shaped above the wire today, so
B5 moves the seam up to typed values on both sides of the transport:

- Session stops encoding frames for component roles. Indicator snapshots and
  application catalogs, plain or with r8 identities, reach the transport as
  typed values through `publish_indicators` and `publish_catalog`. Session no
  longer builds frames and hands them to `send_async`
  (`metadata_shell/indicators.rs`, the catalog paths in
  `metadata_shell/launcher.rs` and `application_catalog/publication.rs`).
- The transport's owner paths hand typed records to one queue call per wire,
  not raw socket frames through `output.push`:
  - native launcher: opening, focus, focus revocation, input, activation
    outcome and close;
  - catalog activation outcomes;
  - indicator activation outcomes.

  The socket wire encodes frames, including the multi-frame catalog and
  indicator transfers, and the file wire encodes events or publishes objects.
- Inbound, the file export decodes the new candidate kinds into typed
  submissions: native and catalog candidates as whole values, input acks,
  activations and indicator activations. The existing owner calls
  (`take_native_launcher_input_ack`, `take_catalog_request`,
  `take_indicator_request` and the candidate services) read them from the typed
  queue exactly as they read decoded frames today.
- Response credit is charged per record by kind, not in socket-frame bytes, and
  each wire enforces its own byte bounds. This removes the socket-shaped
  charge the file wire inherited in B4.

The descriptor profile carries its feeds and owner records as native objects,
events and candidates. Its admission, exact values, semantic refusals and
presentation rules are specified in [descriptor shell files](sophia-shell-descriptors.md).
Its seventeen kinds and twenty-four body/prefix/row layouts are included in
`sophia-shell-files-v1.kdl`. Session selects this role before negotiation;
capability bit 0 on an ordinary content component grants no descriptor authority.

Objects are published as `outputs` is: fits-then-qid-then-announce, pinned on
open, `EBUSY` for a second pin, a fresh qid whenever the bytes change. Each
owner hands its records to the wire as typed values; no owner queues a socket
frame on a file-wire component.

### Negotiation and pacing outcomes (normative)

These are the observable outcomes a client and the independent oracle rely
on. Layouts are in the KDL; this section fixes which records appear.

**Negotiation.** A Negotiate record is judged against the role profile Session
fixed before the peer connected.
- Accepted: one `Negotiated` event. For the component content profile the
  selected revision is min(maximum, 6) for any offer with
  1 <= minimum <= maximum and minimum <= 6. Content bits (7, and 8 with 7) are
  grantable only when the selected revision is at least 5. The capabilities
  are bit 0 (required in every offer), work-area reservation, the granted
  content bits, and the indicator bits only when requested at revision 6. `limits_published` is 1
  and the `limits` object is readable once the event is visible.
- Accepted (native launcher, r7): one `Negotiated` event with
  `selected_revision`=7 for any offer with minimum <= 7 <= maximum (any other
  offer is not served at all; see below). `required_capabilities` must equal
  exactly bits 5, 7, 8 and 11 (application catalog, content surface, content
  discrete input, native launcher) -- not a superset or subset -- and the
  granted `capabilities` echo that same exact mask.
- Accepted (persistent catalog, r8): one `Negotiated` event with
  `selected_revision`=8 for any offer with minimum <= 8 <= maximum.
  `required_capabilities` must equal exactly bits 1, 5, 7, 8 and 12
  (work-area reservation, application catalog, content surface, content
  discrete input, persistent catalog); the granted `capabilities` echo that
  same exact mask.
- Refused by content policy: one `Refused` event, then revocation. Reason 1
  (permission denied) when the operator denied content, with
  `denied_capabilities` the requested content bits (or only bit 8 when just
  discrete input is denied); reason 4 (unavailable) when content is
  unavailable or its budget cannot be admitted. For r7 and r8,
  `denied_capabilities` is instead the whole exact mask above, never a
  partial one: reason 4 when content is unavailable, reason 1 otherwise
  (denied outright, or granted without discrete input).
- An offer the profile cannot serve at all (minimum revision 0 or above the
  profile's maximum, minimum above maximum, bit 0 missing, discrete input
  without surface, or a required bit the profile cannot grant) ends the attach
  without a `Refused` event: the peer observes the export's revocation.
  Reasons 2 and 3 are reserved and not used by this version. For r7 and r8,
  the same "ends the attach" outcome (no `Refused` event) also covers: a
  revision window that does not include the exact fixed revision;
  `required_capabilities` other than the exact mask above; reaching
  negotiation with no content limits already reserved for this connection;
  and reserved limits whose grant names a different connection epoch.
- A second `Negotiate` submit after one was accepted fails with `EALREADY`;
  any content record before negotiation fails with `EACCES`. Neither is
  journaled.

**Pacing.**
- A `FrameDemand` that the candidate owner can serve yields a `FramePermit`
  with state 1, the permit, `ttl_ms` <= 250 and reason 0.
- A permit that expires unconsumed yields `FramePermit` state 2, reason 6
  (timeout).
- `FrameDemandCancel` naming the standing demand (permit_id 0) or its
  unconsumed permit yields `FramePermit` state 3, reason 11 (cancelled), with
  the same demand_id and the cancelled permit_id (0 when only the demand was
  standing). A cancel that names no current demand or permit is a stale
  record: the component's authority is revoked and nothing is journaled for it.
- A new permit for an output supersedes that output's pending, not yet
  submitted candidate: `CandidateOutcome` kind 4, reason 10 (superseded).
- A `Candidate` whose `pacing_permit` is not the output's current unconsumed
  permit (missing, wrong, cancelled or expired) is a stale record, handled
  like the stale cancel above: it gets `Submitted` custody, no
  `CandidateOutcome`, and the component's authority is revoked. (Open product
  question, not a transport rule: whether a refused permit should instead
  yield `CandidateOutcome` rejected with reason 1 on both wires.)

### Role family outcomes (normative)

These are the observable outcomes for the native launcher (r7), persistent
catalog (r8) and view indicator (r6) families: which record, status and
reason a client sees for each negative case. Per-record field layouts and
byte-level rules are in the KDL; this section fixes lifecycle behaviour that
spans several records. `reason` values are the shared `ContentReason` codes
used throughout this contract (1 Stale, 2 Budget, 3 Malformed, 4
Unauthorized, ... 12 Revoked) unless noted otherwise.

**Native launcher activation (`NativeActivate` to `NativeActivationOutcome`).**
An activation is checked against the connection's current focus binding,
state revision, published catalog and (for a keyboard cause) outstanding
Accept receipt, in that order; the first failure decides the outcome.

| Status | Reason | Trigger |
| --- | --- | --- |
| 1 Admitted | 0 | Every check passes and the launch queue admits it. Admission is queue ownership only, not application startup. |
| 2 Stale | 1 | The binding does not match the current focus exactly (any field, including one already superseded by a later `NativeFocus`); the named `state_revision` does not equal the connection's current state revision (a query `NativeInput` since the binding was observed disarms an activation issued against the older revision); the catalog generation or connection epoch does not match; for `cause`=1 (keyboard), no matching Accept receipt remains eligible within `action_ack_timeout_ms`, or `slot` is not the presented candidate's `selected` slot; or the activation reaches the connection after `NativeClosed` for that opening (no Presented candidate remains). |
| 3 Unknown | 3 | `slot` is absent from the current catalog. |
| 4 Unauthorized | 4 | `slot` is present but not `available`, or not among the presented candidate's displayed rows. |
| 5 Capacity | 2 | Every check above passes but the launch queue itself refuses for capacity. |

A `NativeActivate` is never accepted "before a Presented candidate" or
"without a focus lease" as a distinct case: both collapse into Stale above,
because `native_launcher_focus()` is `None` until an actual Presented
mints a `NativeFocus`, so the binding-match check already fails.

**Native launcher input acknowledgement (`NativeInputAck`).** The
`disposition` a client declares is not itself checked; the session instead
checks whether the named `event` has an outstanding, not yet acknowledged
receipt. If it does, the receipt is retired (a non-Accept receipt regardless
of the declared disposition; an Accept receipt keeps the declared value,
which does not by itself affect activation eligibility above). If it does
not -- unknown, already acknowledged, or from an opening `NativeClosed` has
since cleared -- the ack is consumed with no reply and no other effect:
there is no outcome record for a stale `NativeInputAck`.

An Accept receipt remains eligible after its acknowledgement, regardless of
disposition, until its activation has been attempted, its eligibility deadline
passes, or its binding/opening is invalidated. Ack and activation may arrive in
either order. A receipt cannot authorize a second attempt. Local atomic queue
admission of both records does not make their server custody or effects atomic.

**Native and persistent-catalog candidates.** Most of a `NativeCandidate`'s
structure is a value-validator rule the decoder checks, on both wires,
before the record reaches the owner at all: exactly one surface, at least
one placement, `target_count` equal to `row_count`, every displayed
`NativeCandidateRow.slot` distinct and in range, and `selected` a member of
the displayed rows (or 0 when there are none). A record violating any of
these is refused at `submit` with `EINVAL`; nothing is journaled, not even
`Submitted`. A `CatalogCandidate`'s decoder checks only that every provided
surface has role=1 (panel); it does not check the surface or placement
*count*, so `surface_count` exactly 1 and `placement_count` at least 1 are
the persistent-catalog candidate owner's to enforce instead, once
`Submitted` custody has already transferred.

Everything else here needs live connection state the decoder does not have,
so only the owner judges it, after custody has transferred. A
`NativeCandidate` or `CatalogCandidate` naming a stale `catalog_generation`
(or, for the native launcher, a stale `opening` or `state_revision`) is
refused with `CandidateOutcome` kind 3 (Rejected), reason 1 (Stale). The
following are refused with the same kind 3, reason 3 (Malformed) instead: a
`CatalogCandidate`'s surface- or placement-count violation above; a
displayed row (native) or target action naming a catalog slot that is not
currently present and `available` in the connection's live catalog; and,
for either family, a duplicate (`target_id`, `target_generation`,
`action_id`) triple or an overlapping bounds rectangle on the same surface
among that record's targets -- the base `Candidate` rules, which the
decoder never checks either. A `NativeCandidate` or `CatalogCandidate`
whose `pacing_permit` is not the output's current unconsumed permit follows
the base `Candidate` pacing rule above instead: `Submitted` custody only, no
`CandidateOutcome`, and the component's authority is revoked. None of these
cases reach `NativeFocus`, an activation owner or the launch queue.

The preceding outcomes describe candidate validation before renderer submission.
There is also a revalidation when a pending native candidate is taken for
renderer submission: facts generation, interaction generation, opening, catalog
generation, state revision and allocation binding must still match. A mismatch
returns an owner error; current Session terminates the component and revokes its
grant. That path does not promise a Rejected outcome. These different stale-work
results remain a role-outcome consistency question (t262).

**Persistent catalog activation (`CatalogActivate` to
`CatalogActivationOutcome`).** `reason` is always 0 here regardless of
`status`, unlike the native launcher above; only `status` varies.

| Status | Trigger |
| --- | --- |
| 1 Admitted | The wrapped action's `event_id` is a live, already-issued one; `catalog_generation` and the grant's connection epoch match the currently published catalog exactly; a matching, still-awaiting, not cancelled, not expired ledger entry exists for that exact action; that entry's target is still part of the currently Presented candidate (activation is accepted only against the Presented target); `action_id` names a slot present in the current publication; and the launch queue admits it. |
| 2 Stale | Any eligibility check above (other than the slot lookup) fails. |
| 4 Unauthorized | `action_id` names a slot absent from the current publication, or the launch queue itself refuses as unauthorized. |
| 5 Capacity | Every eligibility check passes but the launch queue refuses for capacity. |

Status 3 (Unknown) is never produced for `CatalogActivationOutcome` in the
current code; an unrecognized slot is Unauthorized (4) here, where the
native launcher's analogous case above is Unknown (3).

**Indicator activation (`IndicatorActivate` to
`IndicatorActivationOutcome`).**

| Status | Reason | Trigger |
| --- | --- | --- |
| 0 Accepted | 0 | A snapshot has been published; the named `connection_epoch`/`snapshot_generation` match the snapshot last published; a published `IndicatorEntry` matches the named (`output`, `indicator`, `action`) triple exactly; that entry's `action` is nonzero; and (when ordinary input is disabled) `event_id` exceeds every previously accepted `event_id` on this connection; and the downstream admission step admits it. |
| 1 Stale | 0 | No snapshot has ever been published, or the named `connection_epoch`/`snapshot_generation` do not match the one last published. |
| 1 Stale | 0 | Ordinary input disabled: otherwise eligible, but `event_id` does not exceed the connection's high-water mark. |
| 1 Stale | 1 | Ordinary input enabled: `indicator_admission` finds no linked, still-awaiting action for the named `event_id` -- it is 0 or exceeds every event this connection has ever issued; no action with that `event_id` remains on the ledger (already collected once fully settled, or it was never an indicator-family action, e.g. a native-launcher or persistent-catalog action's `event_id`); its `action_ack_timeout_ms` deadline has passed; it was issued under a different `connection_epoch`; the WM has already recorded a decision for it (admitted or rejected), even though not yet acknowledged; or its own `output`/`indicator`/`action` differs from the ones named in this activation. Or, in either mode, the WM admission reports a duplicate. |
| 2 Unknown | 0 | No published `IndicatorEntry` matches the named (`output`, `indicator`, `action`) triple. |
| 2 Unknown | 2 | Otherwise-eligible, but the downstream admission step refuses for capacity. This family has no dedicated Capacity status; a capacity refusal is folded into Unknown/Budget here. |
| 3 Unauthorized | 0 | A matching `IndicatorEntry` exists but its `action` is 0 (published but not activatable). |

### Native launcher lifecycle and clocks (normative)

These rules describe the current production Session. Runtime fixtures may
supply different owner contexts; such a fixture does not establish a different
production value or clock domain. No additional authority is inferred from
client receipt time, an object read, or local queue admission.

**Catalog and facts.** Production Session publishes one native catalog per
grant, at generation 1. It does not republish that catalog during an opening.
There is therefore no supported transition in which an existing opening adopts
a new catalog generation. A client observing that situation must disarm new
candidates and activations for the old generation; it can continue acknowledging
input while awaiting close or termination. An arbitrary runtime fixture that
republishes a catalog is not evidence that production Session supports it.

Session advances `facts_generation` when output facts change. The Outputs object
and its ObjectPublished announcement carry that same generation. A candidate
must name the Session's current facts when validated and when taken for renderer
submission. Fetch the facts used to build the candidate, and disarm that local
scene when a newer Outputs announcement arrives. This reduces stale work but
does not eliminate a race with a later server update. The announcement does not
reserve that generation for the client.

Production Session supplies `interaction_generation` **1** to the native and
ordinary shell candidate owners. It is not client-selected authority and is not
a counter the client may advance. The codec's field bound remains nonzero;
owner validation requires equality with the supplied context. Native focus,
input and activation bindings carry the presented candidate's value. A future
policy that changes this value needs a defined way to communicate it first.

**Input and actions.** `NativeInput.issued_mono_usec` uses Session's host
`CLOCK_MONOTONIC` microseconds. An admitted local client using the same clock
domain may compare it with its own clock. The acknowledgement deadline starts
at the later of issuance and the owner's last service timestamp. An unacknowledged
input expires after `action_ack_timeout_ms`; Session closes its opening with
reason 6 (Timeout). An Enter intent held while waiting for the requested revision
to be presented expires after `presentation_timeout_ms`, also closing with
reason 6. Late acknowledgements within the connection's grant after close are
consumed without effect; a wrong-grant acknowledgement is fatal. Using
issuance plus the timeout is an earlier client scheduling deadline; client
receipt time must not restart the server's deadline. Eligibility and expiry are
judged at Session service time. Sending before a local deadline does not
guarantee timely arrival or service.

`Action.kind` 1 is an invocation, 2 a dismissal, and 3 a cancellation. Kinds 1
and 2 require an exact `ActionAck` with disposition 1 (Consumed) or 2 (Stale).
Kind 3 requires no acknowledgement. A stale, cancelled, unknown or mismatched
action acknowledgement within the connection's grant has no effect; a wrong
grant is fatal. A missing kind-1 acknowledgement produces a kind-3 cancellation
after the deadline. The file codec requires disposition 1/2 for both acknowledged
kinds; the dismissal owner itself does not separately validate that value.
Native pointer activation (`cause` 2)
names the invocation Action's `event_id`, its row slot as `slot`, the current
focus binding and current state revision. It requires the matching eligible
action before its deadline and may name any displayed, available row, not only
the selected row. Native targets use `action_kind` 2 and `action_id` equal to the
row slot. This is distinct from the base ContentTarget profile's reserved kinds.
A keyboard-only SDK convenience layer may omit pointer activation while still
exposing Actions and their acknowledgement obligations.

**Permit time.** `FramePermit.ttl_ms` is the lifetime from the owner's grant-time
sample, not from client receipt. Current Session uses elapsed milliseconds from
its content service's `Instant` origin and samples before polling and servicing
demands in that visit. The timestamp and origin are not transmitted. Neither
receipt plus TTL nor enqueue plus TTL is a guaranteed client-side expiry bound:
the sample can precede ingestion, and queueing and scheduling delays are not
bounded by this protocol. A margin is an advisory scheduling policy only.

The owner expires an unused permit before ingesting later candidates. Expiry
produces FramePermit state 2/reason 6; a candidate subsequently naming it follows
the fatal stale-permit rule above. A client that judges a permit too old locally
must not treat that judgement as a server cancellation or send a speculative
cancel for it. It must wait for authoritative expiry/cancellation before asking
for another permit when it leaves the old permit unused. A client must not send
a new FrameDemand while that output has an outstanding permit or an assembling
candidate: current Session propagates the resulting grant refusal as a fatal
error. Grant refusal from saturated owner capacity is likewise fatal. These
rules provide no guarantee that a candidate sent
before a local deadline reaches the owner before expiry. Changing that failure
policy or adding a usable issue timestamp is separate contract work (t262).

**Close and launch admission.** NativeClosed clears presentation, focus and input
authority immediately; it is not a resource-release barrier. Closing rejects
pending allocation proposals as Stale, cancels a standing demand or unused
permit with FramePermit state 3/reason 11, and rejects assembling or pending
candidates with CandidateOutcome kind 3/reason 11. Work already submitted to the
renderer remains with its owner and can still settle. After the old pixels are
removed, Session invalidates the opening's allocations with AllocationResult
status 4/reason 12. Resources retain their normal retire/release lifecycle, and
resource records remain serviceable after close.

A late allocation request, including release, is rejected Stale or ignored if
its request ID has already been processed. A new late candidate for the closed
opening receives a Cancelled outcome; a late demand receives FramePermit state
3/reason 11. Old identifiers can instead be ignored. After reopening, a late
demand is recognized as belonging to the old opening only if it names a
non-current allocation; an output-wide demand (allocation 0) is served as
current. The client must stop using the allocation at close rather than waiting
for its invalidation. Reopening requires a larger opening ID. Once a native
activation is Admitted, focus is disarmed, new input/focus cannot be issued for
that opening, and further activations are Stale. Session then closes it with
reason 11 (Cancelled); admission is still not proof that an application started.

**Identifiers and concurrency.** Within the grant's candidate owner,
`demand_id` must strictly increase across outputs; a non-rising value is a fatal
stale request. `candidate_generation` must strictly increase; a non-rising value
is rejected Stale. Allocation request IDs must strictly increase; a non-rising
value is rejected with AllocationResult status 2/reason 1. A transaction need
only be nonzero; these rules do not require it to be monotonic. Submission IDs
remain a separate strictly increasing attach-scoped sequence.

There is at most one standing demand and one permit/assembly per output. A newer
demand can replace the standing demand; grant consumes the demand and candidate
begin consumes the permit. Every new frame therefore needs a new demand.
FrameDemand reason 3 means withdrawal and takes priority over reason 1 or 2
(dirty or animation work, treated alike). Replacing a standing withdrawal with
reason 1 or 2 is a fatal stale request. Activation responses are serviced one at
a time; each Accept permits one attempt. Input receipt capacity is at most 16 and
at most `max_pending_actions` minus pending pointer-cancellation reservations;
other advertised allocation/candidate limits still apply.
An SDK may choose stricter single-operation bounds as local queue policy.

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

| Role | Revision | Client boundary |
| --- | --- | --- |
| Bar | r6 | Standalone desktop SDK content session |
| Native launcher | r7 | Standalone desktop SDK native launcher session |
| Dock | r8 | Standalone desktop SDK persistent catalog session |
| Descriptor | r1-r8 | Standalone desktop SDK descriptor session |

The C and Rust desktop SDKs expose native records over standard 9P2000.L.
Client implementations and their dependency pins belong in their repositories.
Revision 9 and overview capability bit 13 are not part of this accepted contract.
Skew is resolved by revision and capability negotiation, with no wire fallback.
Supervisor replacement starts a fresh process and epoch; reconnect never replays
unsettled submissions.

Every Session shell component uses 9P2000.L. Omitted `transport` selects it;
an explicit `9p2000.L` is accepted and `current-ipc` is refused. A descriptor
component excludes other shell components; the single-shell CLI selectors are
retired. Clients and inherited environment do not choose the server's protocol.
Protected launch supplies only the owner's `SOPHIA_SHELL_9P_SOCKET`, removing
supplied endpoint variables including the retired `SOPHIA_SHELL_SOCKET`.
There is no sniffing or fallback. Components share one `ContentEpochRegistry`;
the transport default neither creates another budget nor changes role grants.

Replacement stops and revokes the old component, retains outstanding retirement
claims and issues a fresh grant and connection epoch. It never migrates a live
grant. Profile reload does not replace the startup component selection. Recovery
to an IPC client requires a verified compatible older release as a whole for the
next login, rather than substitution in a running 9P Session. The default remains
experimental: source retirement does not close the outstanding t250/t252 latency
and physical qualification, or authorize installation.

## Independent clients and evidence

Independent checks include the Go file oracle and the C desktop SDK peer,
implemented separately from the server's Rust codecs. Product integration is
separate evidence in client repositories and desktop tooling. Upload alone (r5)
is not enough; content-profile evidence must also cover the r7 launcher and r8 dock:

- negotiation for each exact profile;
- allocation;
- slot upload, including split, cancelled and fenced writes;
- candidates and pacing;
- action acknowledgement;
- r7: native opening, focus lease, semantic input, activation and close;
- r8: catalog snapshot with identities, and catalog activation by generation
  and slot.

The Go oracle's admission is supplied, so that fixture does not prove supervisor
authentication. Protected C descriptor and content peers exercise the production
launch and presentation owners separately. Each test's limits remain explicit;
codec independence alone establishes neither physical presentation nor a product
client's behavior. The descriptor proof includes work-area changes only after
the matching presentation, tabs, shortcuts and launcher exchanges.

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

## Amendment 1 (2026-09-26): native bodies, IPC-independent records

The operator set the migration's end state: all IPC code is purged and every
role runs on 9P files. The accepted body rule, a transaction ID followed by
the unchanged `sophia_shell_v1` payload, would keep the IPC payload codec
alive as the file format, so it is replaced before more families build on it:

- **Typed records are wire-neutral.** The shell record structs, limits and
  validators leave `sophia_protocol::ipc` for a neutral module. Their
  validation errors do not name the IPC codec. The IPC codec becomes one
  encoder over those values and is deleted at t255 without touching owners.
- **Every file record has a native layout**, defined in
  `sophia-shell-files-v1.kdl` and `sophia_protocol::shell_files` as
  `wm_files` does: no socket framing artifacts (chunk ordinals, repeated
  counts, Begin/Entry/End transfers). `Candidate` is one header and three
  counted row tables; limits, outputs, catalog and indicators are objects
  with their own layouts. Slice-1 kinds are re-encoded; nothing shipped.
- **Budgets are wire-neutral.** Owners charge response credit per record,
  and bulk records in native record-body bytes. The content registry and typed
  FIFO use the same charge; each wire enforces its own byte bounds. A record
  stays queued and charged until a journal append accepts it or the socket
  writes its last byte. A refused or partial transfer never releases custody.
- **Clients seam at typed values.** `sophia-shell-client` queues typed
  records and objects; each wire encodes natively. No frame translation.
- **The independent oracle is written from this contract alone**, never
  from `protocol/sophia-shell-v1.kdl`.

The per-role behaviour, owners, bounds and budgets above are unchanged.

### IPC purge inventory (t255)

Nothing new may depend on these; each is deleted when its role's file wire
is the accepted default:

| Area | IPC code |
| --- | --- |
| Shell | socket transport (`shell_transport` socket branch, inbox/outbox frames), `ipc::shell_*` codecs (`fields.rs`, `codec.rs`), `packets/shell_*` |
| Shell clients | Rust desktop SDK `sophia-shell-client` socket wire; C desktop SDK `src/shell_wire` socket half (Sophia pins the latter under `vendor/c-desktop-sdk/source`) |
| Shell file contract | the socket-shaped `Limits` fields (`max_frame_payload`, `max_input_queue_bytes`) and their +24/+48 validation relations remain until coordinated SDK retirement. Owners already charge native record-body bytes and per-record control credits; `max_output_queue_bytes` and the control reserve remain Session retention bounds. Only the socket adapter uses the two socket fields to bound I/O |
| WM | `policy_transport_worker/current_ipc.rs`, `ipc::wm_v1*` and `ipc::policy_*` codecs, Hagia's legacy policy wire |
| WM file wire (retained) | `wm_records`, `wm_rows`, `policy_scalars` and `BinaryCodecError` are neutral owners; `sophia-wm-files-v1.kdl` owns the fixed row layouts. The old IPC adapter imports these; deleting that adapter does not delete the file codecs |
| Output | output socket role (`ipc::output_v1`), migrated by t253 |
| Control | control socket (`ipc::control_v1`), per the control-bus plan |
| Broker/portal | `ipc::broker*`, `ipc::portal` (t254 inventory) |

## Open decisions

- The `AllocationResult` event `status=4` (invalidate) preserves today's semantics in v1:
  it is Session-initiated, holds no pre-reserved credit (`crates/sophia-runtime/src/shell_content/allocations.rs:379-407`),
  and uses the non-reserve journal space. A guaranteed variant would reserve +16
  (`max_allocations_total`).

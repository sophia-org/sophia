# C shell wire foundation

## Native 9P file backend (t252 B7)

`sophia_9p_client.h` with `nine_p/*.c` implements version, attach, walk,
lopen, read, write, clunk and flush over an already admitted stream fd.
`sophia_shell_files.h`, `sophia_shell_files_content.h` and `shell_files/*.c`
provide the independent base file records and `sophia_shell_files_client.h`
orchestrates a file session. Compile these C99 sources together. They do not
link the existing `shell_wire` sources or translate IPC frames.

### Role record codecs

`sophia_shell_files_roles.h` adds Catalog and Indicators objects, native launcher
events/requests, and persistent catalog candidates/activations. These use the
same `sophia_sf_encode` / `sophia_sf_decode` entry points. The file session also
supports r7/r8 through `sophia_sf_client_init_profile`; Bemenu adoption remains
separate work.

Candidate bodies also expose `sophia_sf_role_candidate_{encode,decode}_bytes`
and `sophia_sf_role_candidate_validate_value`. Byte operations enforce field
bounds; normal record operations additionally enforce family value rules.
Native one-surface/nonempty-placement, slot uniqueness, selection membership
and target/row-count rules are value checks (submit EINVAL, no journal entry).
Catalog one-surface/nonempty-placement checks and state-dependent eligibility
belong to the runtime owner. Byte decoding alone grants no authority.

Catalog and Indicators rows borrow immutable encoded storage. A decoded
`sophia_sf_catalog` points into the original record; `sophia_sf_catalog_entry_at`
returns a typed row with borrowed text. Row encoders let the caller construct
the storage without allocating a 4096-entry C struct array. Native input text
also borrows its record buffer. Keep that storage alive until finished with
the decoded value. Persistent catalog validation uses about 12 KiB of bounded
stack scratch for slot and identity uniqueness; it does not allocate a catalog
or change the borrowed rows. Encode into separate storage: destinations must not overlap
the input value or its borrowed data. Failed encoding leaves destinations
unchanged; failed decoding leaves output values unchanged.

The largest record cap is 4 MiB (`SOPHIA_SF_MAX_RECORD`); submission staging
retains its 64 KiB cap (`SOPHIA_SF_MAX_TRANSACTION`), and native/catalog
candidates remain bounded to 8192 bytes. The record union contains bounded
typed candidates and snapshot views, never a full catalog allocation.

`sophia_sf_client_init_profile` takes BAR, LAUNCHER or DOCK plus caller-owned
object scratch. It checks the API role and, for launcher/dock, the exact
revision/capability profile. Supply 4 MiB for maximum catalogs, or 32 KiB for
indicators. The base initializer retains its inline 1 KiB buffer and BAR profile.
Large reads respect msize/iounit. An oversized object fails the fetch and closes
its pin without overflowing the buffer or terminating the session. Storage must
remain alive and separate from client/wire state until disposal; returned views
remain borrowed until the next fetch.

The development `SOPHIA_SF_DESCRIPTOR` profile uses `spec/proposed/`. Its api
must name `descriptor`, and its offer must obey that proposal's revision and
capability dependencies. `sophia_ss_welcome` exposes the validated selection.
Metadata-only sessions become ready after bootstrap Submitted and Negotiated
are consumed, with no Limits request; combined content also needs valid Limits.
Unselected object and record families are refused locally. Descriptor, tab and
shortcut announcements participate in the same fetch/ack holds as other feeds:
full decode, matching qid/generation and an EOF probe precede release.
`sophia_sf_client_init_buffers` and `sophia_ss_open_fd_staging` support caller-owned
large transaction scratch. A 52,488-byte transaction buffer accommodates all
proposed candidates (including 8,260-byte maximum tab candidates); provide
separate queue capacity for the desired admission group. The queue retains its
512 KiB total bound and 64-slot bound. The transaction buffer must be separate
from queue storage, object scratch and the session; the initializer checks all
regions before starting I/O. Records are copied on admission, then copied into
transaction storage before queue compaction. The inline initializer still uses
8 KiB and refuses records that can never fit without issuing a ticket or
submission. `sophia_ss_record_bytes` validates and measures candidates without
allocating encoded scratch; it does not promise admission capacity. This
development path has scripted-peer coverage, not production-export
qualification yet.

Reference inputs are the shell file KDL at `bae4ec4a9` (including the Limits
rules from `f64d670e0` and conditional-bound correction `6bb0c8f2e`), extended
with role layouts and normative value rules from `ee5e7f809` and the validation
layer clarification in `ae60576f8`,
profile blob `de101e3d` and diod reference blob
`48d63c80` (upstream `de51d1ee1bd5`). The KDL alone supplies record layouts
and validation. The API lifecycle contract is pinned at `43e4530b3`.
After attach, the client reads the complete `api` file and learns its epoch
before submitting any records. It rejects malformed, oversized or unterminated
API lines. Session's launch owner supplies the revision range, capability mask
and fd. Bemenu can select this backend explicitly at startup
and own a `sophia_sf_client`; it must continue consuming events and servicing
the fd. There is no endpoint discovery or automatic backend selection.

Transport memory is caller-owned: `(2 * requests + 1) * msize`, with 1–32
requests and msize 4096–16777216. Each request reserves its completion storage
and a paired flush slot. Completed replies, including empty ones, retain their
tag until consumption and any Rflush. Handles include a monotonically increasing
serial so stale handles cannot consume a reused slot. Flushing a clunk is refused.
An unanswered flushed attach/walk releases its reserved fid; a reply preceding
Rflush preserves its effects. Each service call performs at most 32 send and
32 receive attempts with separate byte budgets. A clean EOF allows validated
completions to drain; malformed replies poison the connection. The caller closes
the fd and disposes of all storage. No reconnect or replay is implicit.

The session requires at least eight request slots and enough fids for the root,
four service files and temporary object/upload pins (32 is a suitable setting).
It holds at most one staged submission, one decoded event, one object fetch and
one upload. An events read can remain outstanding while other operations run.
Call `service`, inspect and consume events, and explicitly call `ack` after
every service and event-processing pass. Fetch announced objects promptly:
their acknowledgement holds also block the next queued submission. Do not wait
for a full journal or an acknowledgement deadline. Submitted proves custody,
not semantic acceptance. Positive
short writes advance the acknowledged cursor; submit EAGAIN retries the same
submission. Every completed submission closes and reopens its transaction fid.
The client validates the selected revision and required capabilities.

Negotiation starts a Limits fetch when announced. Drain `object_result` after
it completes. Subsequent Limits/Outputs fetches use fresh pins and compare the
announced generation and qid; a superseded pin reports AGAIN. The returned
object pointer remains borrowed until the next fetch. Unknown record kinds
fail closed. Catalog/Indicators use the same pin and announced-identity checks.

Use `upload_begin`, then wait for status 1 and the upload writer to open before
calling `upload_chunk`. The chunk pointer remains borrowed until acknowledged
or terminal status cleanup; the client advances it across short writes. It
supports one transfer at a time within the granted slot count. Consume pending
events before continuing uploads. End requires every declared byte to have been
acknowledged. Cancel waits for the writer to open and returns BUSY while writes
remain outstanding; retrying follows them in order. Status 2 accepts, 3 rejects
and 4 cancels; terminal
status closes the writer before the slot is reused. The application still owns
allocation, permit, action and presentation lifecycle decisions.

Run `sh tools/check_shell_c_wire_files.sh`. The Rust test harness compiles the
independent C tests and exercises the production export with supplied protection
evidence. Coverage includes R4-1 through R4-7, tag wrap, reply poisoning, clean
EOF, 22 base and 17 role KDL-derived literal vectors, negotiation, rejected allocation, a split
64 KiB upload, cancellation and explicit acknowledgements. It does not start a
desktop or prove protected launch. Base records include native whole Candidate
rows. `shell_files_c_roles` additionally drives the C launcher and dock sessions
through real allocation/resource/candidate/focus owners, including a 4096-row
catalog, query disarm, semantic input, activation and close. Catalog admission
decisions are scripted from the contract; this is not a Session launch-policy
test. A small-buffer dock fixture also proves oversized-object refusal releases
the pin and leaves the session usable. Neither peer starts applications or
changes the Bemenu repository. These live flows pass against runtime
`3856d1014`; the file gate runs two base tests and one role integration test.

## Existing socket backend

`sophia_shell_wire.h` and `shell_wire/{frame,io,negotiation}.c` implement the
published revision 1–8 envelope and negotiation without linking Rust. Compile
those three C99 sources into the client. `sh tools/check_shell_c_wire.sh` runs
the strict C gate; the canonical and shell-protocol gates call it too.

An optional typed catalog assembler is now available as
`sophia_shell_catalog.h` plus `shell_wire/catalog.c`, described below.

The framing layer is **not the complete shell lifecycle SDK**. It does not
authenticate/connect a peer or authorize effects. Separate typed content,
upload, outbox and native lifecycle helpers now handle their respective
records and ownership. Bemenu uses these with explicit Session admission;
neither linking this library nor selecting a backend grants that admission.
See the [capability map](../../docs/native-desktop-capabilities.md) for the
current experimental role limits and independent-client evidence scope.

## Ownership and service

The caller supplies an already admitted stream fd and two separate buffers, each
between 24 and 65,560 bytes. It keeps those buffers and the wire struct alive and
unmodified. The library never allocates, closes a descriptor, changes descriptor
flags or discovers an ambient display/endpoint. Each instance has exactly one
incoming and one outgoing frame slot.

`sophia_shell_wire_queue` copies a client frame into the outgoing slot. A second
queue attempt returns BUSY without changing the original. `flush` retains the
whole slot and exact partial-write offset until the final byte reaches the local
kernel. That is not evidence that the peer received or accepted the record.

`receive` reads at most one incoming frame, even if the kernel has more data. Its
returned payload borrows the incoming buffer until `consume`; repeated receives
return that same frame. Decode and validate the message payload and lifecycle
identity before effects or consumption. `consume` is a buffer operation, not a
protocol ACK. No outstanding input frame may be silently overwritten by a newer
one. The lower layer intentionally cannot decide which semantic obligations must
remain after parsing.

Each service call has an explicit byte budget and at most 32 syscalls including
interrupted calls. Reads/writes use nonblocking per-call flags; writes suppress
SIGPIPE. EAGAIN retains ownership. Fatal I/O, framing errors and EOF latch a
terminal result without closing the caller's fd or declaring pending work
delivered. EOF midway through a frame is invalid/truncated. A peer half-close is
terminal here: there is no half-close response-drain protocol. The caller owns
connection teardown and exact disposition of higher-level obligations.

Frame validation checks magic, version, known shell kinds, required/zero
transactions, reserved header bytes, complete size and the 64 KiB payload cap.
Stream I/O additionally checks message direction. It does **not** validate payload
fields, negotiated capability gates, transaction replay or current grant identity.
Those belong to the typed lifecycle layer to follow. Do not mistake accepting an
envelope for accepting an operation.

## Negotiation

The typed hello encoder checks a coherent revision 1–8 range, known required
capabilities and their dependencies. Existing revision 1–6 requires descriptor
bit 0. Welcome validation requires exactly 28 payload bytes, nonzero connection
epoch, requested revision/capability agreement, capability dependencies and bounded
advertised limits. Caller state still must enforce one welcome per negotiation
and retain its exact epoch; this stateless decoder does not prevent replay.

Revision-7 native launcher and revision-8 persistent catalog negotiation are
supported under their exact capability dependencies. Session's explicit
component configuration admits supported roles; the wire helper cannot expand
those roles, operator grants or resource budgets.

## Evidence boundaries

The controls use actual private socket pairs, including an undrained small kernel
send buffer, byte-at-a-time delivery, held input, exact output retry and peer
closure. A separate wrapped-syscall test supplies EINTR to prove the call budget.
The corpus reader round-trips 70 checked-in Rust-produced frame envelopes and
checks the base hello/welcome payload independently. Other families' payloads
remain opaque to this reader. Schema inventory and source length are checked.

These tests connect no live endpoint and require no renderer/device. They do not
exercise Session admission, client content state, WM actions or native acceptance.

## Authorized application catalog

After validating a welcome that grants `application_catalog`, initialize
`sophia_shell_catalog` with two disjoint caller-owned entry arrays. Each holds
up to 4,096 entries; callers may choose a smaller explicit capacity, in which
case larger catalogs refuse. The helper allocates nothing and opens no socket.
Compile `shell_wire/catalog.c` alongside the wire sources.

Pass kinds 114–116 from the existing receive FIFO to
`sophia_shell_catalog_accept`. Other families return UNRELATED for dispatch by
their owner. Begin/entry/end must agree on connection, transaction and catalog
generation. The helper rejects duplicate slots, incomplete counts, trailing
bytes, invalid flags, oversized text, non-scalar/noncanonical UTF-8, controls
and bidi formatting controls. Strings are copied with a trailing NUL; embedded
NUL is invalid. Limits apply to UTF-8 bytes, not characters.

Only a validated End swaps the complete staging array into the current catalog.
The previous catalog remains visible during assembly and after a rejected
transfer; a rejected transfer latches a connection error requiring fresh state.
A pointer returned by `sophia_shell_catalog_entries` is borrowed until the next
successful commit or reinitialization. Copy any identity/text that must outlive
that boundary. The input wire payload may be consumed immediately after the
assembler returns. An empty completed catalog is distinct from no catalog.

The catalog conveys labels, keywords, availability and opaque slots only. It
provides no execution authority, presented selection, keyboard lease, resource
upload, content candidate, or application dispatch. These remain the typed
lifecycle/Session integration still required for Bemenu.

The C gate independently decodes the Rust-generated catalog payloads, including
labels, keywords, slot order and availability. It also tests replacement
atomicity, unrelated-family interleaving, stale/mismatched identities, malformed
Unicode, exact maximum text lengths and every truncation of a maximal entry.
These are codec/assembly controls with supplied welcome facts, not an admitted
native launcher.

## Native launcher wire vocabulary (revision 7)

`sophia_shell_native_launcher.h` and `shell_wire/native_launcher.c` validate kinds
187–197 without allocating, connecting or authorizing an effect. They share the
bounded framing layer. Golden frames from the Rust codec and the independent C
payload validator are compared by `tools/check_shell_protocol.sh`, including
bounded byte mutations. Text shares the catalog's strict UTF-8/control policy.

The native launcher role requests exactly bits 5, 7, 8 and 11 (`0x9a0`) at revision
7. It does not request the descriptor launcher, work-area reservation or indicators.
The C hello/welcome codec can represent this request and the configured Session
native launcher path can admit it. No backend may treat successful payload
validation as a focus lease, catalog membership or
permission to start an application. The reusable lifecycle below does not replace
live Session admission or the complete launcher application.


## Native presentation and input owner

`sophia_shell_native_lifecycle.h` joins the native opening, candidate, focus,
input and activation records in the original receive FIFO. It owns copied scene
and catalog-slot identities, not menu item pointers. Candidate Prepared never
installs interaction targets; only an exact Presented following Prepared does.
A focus lease must match that presented scene and current edit revision. Catalog
replacement, focus revocation, replacement presentation and close invalidate the
appropriate interaction without erasing an already-owned response or outcome.

Use the same `sophia_shell_outbox` and transaction counter as resource uploads and
other control traffic. Before calling the serialized UI edit callback, the owner
reserves actual ACK storage and FIFO position. It preencodes both possible ACK
outcomes, calls the edit at most once, then commits the chosen response without
allocation. Accept does not call the edit callback: it reserves and emits an
ordered ACK/activation pair using the copied presented selection. Pointer actions
use the exact presented target. Cancellation generates no ACK. An activation
outcome is not local permission to execute an application.

On BUSY, retain and retry the original borrowed input frame. The retained response
matches its kind, transaction and complete payload; retry only commits that exact
response, never repeats the UI edit. Other frame families must still dispatch in
socket order to their owners. Uncommitted outbox reservations stop flushing at
their FIFO position, remain aggregate byte/record charged, and cannot be reused
after commit or owner disposal. This is a serialized returned-error contract,
not thread safety or callback panic recovery.

The caller must separately validate welcome/capabilities, install the committed
catalog, obtain current allocation and frame permit, and retain resident upload
resources. The lifecycle supports one pending one-surface candidate, up to 32
placements/targets within negotiated bounds. Opening and render revisions must
be supplied from the captured UI state. The complete Bemenu controller and live
Session integration are still required before an attended run.

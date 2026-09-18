# C shell wire foundation

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

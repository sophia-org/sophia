---
id: v7m2c9ra
date: 2026-09-17
kind: investigation
status: implementation
tags: [shell, launcher, protocol, bemenu]
---
# Native launcher wire vocabulary and retained lifecycle obligations

This implements the byte vocabulary for
[the independent launcher contract](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md).
It does not enable a second live component. Session negotiation remains capped at
revision 6 and the configured independent launcher remains explicitly refused.

## Inventory and compatibility

The complete IPC inventory ended at kind 186 and shell capability bit 10. The
extension allocates kinds 187–197 and bit 11, since revision 7. Frame version and
header remain unchanged. All new records require a nonzero transaction, little
endian fields, no implicit padding and zero reserved bytes. Unknown, oversized,
truncated and trailing fields refuse. Connection/grant epochs and all exact
binding identities are nonzero; no null binding stands for a current snapshot.

The native role requests exactly catalog/content/discrete-input/native-launcher
bits (5,7,8,11; `0x9a0`), with minimum revision 7. It gets no descriptor, reservation,
indicator or GPU authority from that request. Existing r1–r6 requests retain their
meaning. C negotiation can encode this vocabulary but cannot grant it; the live
runtime's implementation intersection still excludes revision 7.

| Kind | Record | Payload bytes |
|---|---|---|
| 187 | Opening | 56 |
| 188 | AllocationRequest | 84 |
| 189 | CandidateBegin | 108 + 2 × rows (at most 32) |
| 190 | CandidateChunk | 40 + 64 × surfaces + 32 × placements + 48 × targets |
| 191 | Focus | 104 |
| 192 | FocusRevoked | 108 |
| 193 | Input | 132 + text bytes (at most 256) |
| 194 | InputAck | 124 |
| 195 | Activate | 124 |
| 196 | ActivationOutcome | 128 |
| 197 | Closed | 28 |

The schema encodes exact field order. `NativeLauncherBinding` includes grant,
opening, output/generation, allocation/generation, catalog, candidate, presentation,
interaction and state revisions plus a fresh focus lease. Event adds event ID and
resulting state revision. Activation adds an explicit cause family and catalog
slot; keyboard-event and content-action numeric IDs are not interchangeable.
Outcome echoes the complete activation and has its request transaction.

## Candidate and allocation ownership

Kind 188 is a parentless transient request; operation 1 acquires, 2 resizes and 3
releases. Resize/release name an existing allocation. Edge is the existing 1–4
placement preference and margins retain their signed bounds. Session chooses
actual placement on the opening's authorized output. Existing kind 164 returns
its allocation, with zero parent, anchor and allowed reservation. There is no
fabricated parent and no work-area claim.

Kind 189 replaces ordinary Begin in the SAME owned candidate transaction. Its
first 80 bytes are the existing Begin; opening/catalog/state revision and ordered
rows follow. One surface, at least one placement, and one target per visible row
are required. Empty results have zero rows/targets/selection; nonempty results
select exactly one included slot. Rows are unique, in 1–4096 and at most 32.

Kind 190 retains the existing chunk byte layout but explicitly permits only
surface role 3 (parentless transient) and target action kind 2 (catalog row).
Surface parent is absent, anchor zero and reservation zero. All placements and
targets use surface index zero. Target action ID names the catalog slot. Runtime
must match targets, in candidate order, against the exact ordered Begin rows and
published catalog; a codec cannot establish that cross-record fact. Resource,
pacing, End, outcome, Presented, discrete action and release records are reused.

The existing kind 173 codec continues rejecting these new roles/action kinds.
This avoids widening old clients' authority as a side effect of learning a new
payload. Runtime assembly must attach row binding to its actual candidate owner,
not keep a second pending candidate or independently acknowledged metadata queue.
Every native byte/record consumes existing negotiated aggregate limits, including
the larger Begin and its retained response credit.

## Input, activation and pending presentation

Opening starts at state revision 1. Session grants Focus only for the exact
Presented binding; initial rendering and Prepared cannot acquire keyboard input.
Text is committed UTF-8 with the catalog's control/bidi-formatting exclusions.
Semantic commands are numbered explicitly in the schema. There is no clipboard,
raw keycode, arbitrary return-string execution or helper channel. Escape and
outside dismissal remain Session cancellation, not client-generated key authority.

Edits/navigation advance Session's issued state revision and disarm activation
until that exact resulting model presents. Each input carries its original
binding and the resulting revision. ACK retains the exact issued event even if
new content or focus is later installed. Accept and Activate instead require
revision equality with their presented binding. A matching codec shape is not
proof an Accept event was issued, acknowledged or eligible.

To avoid losing Enter during pending repaint, the owner integration must retain
one bounded Accept intent with original issuance time and reserved control credit,
scoped to the opening and latest issued state revision. It may issue Accept only
after that exact state presents and receives its new focus lease. A later edit,
cancel, replacement or revoke invalidates this intent; it cannot migrate to a
newer selection. This is retained input, not early launch authority. Pointer
press/release is likewise never retargeted from old pixels to a new row.

Activate cause 1 must match an issued Accept and its selected slot; cause 2 must
match the exact issued ContentAction/target. The shared admission owner validates
current catalog, lease, presentation and pending event before enqueueing policy.
ACK and admission remain orthogonal. Outcome statuses are admitted/stale/unknown/
unauthorized/capacity (1–5); only admitted has reason zero. Admitted means queue
insertion, not application startup. No receipt, retry or reconnect may replay it.

## Evidence and remaining work

Rust validates typed encoding and decoding. Independent C code parses the byte
fields without linking Rust. Controls cover every fixed length, all truncations,
zero identities, selection/count/role disagreement, reserved bytes, invalid UTF-8,
control characters, bounded rows/text, activation cause and presented revision.
The golden corpus covers all eleven kinds; byte mutations compare acceptance
classes across the two implementations. Sanitizer evidence is payload parsing,
not native execution or focus/launch lifecycle evidence.

Still required before admission: exact candidate-store attachment and credits,
role-specific protected negotiation, two supervised live services, transient
composition, presented focus/input ownership, catalog/launch integration, C client
lifecycle and Bemenu backend, formal controls, full joined headless integration
and separately attended acceptance. This checkpoint does not make `lom-test`
ready for a native launcher.

Scoped validation before freeze: 167 protocol tests pass in a device-hidden
namespace; strict protocol all-target Clippy, formatting and source layout pass.
The independent C framing/catalog/native suite passes. Clang ASan+UBSan validate
all eleven native golden records and 3,734 byte-mutated records. Two separately
compiled Rust mutations fail their intended controls: accepting an unpresented
revision and widening legacy kind 173 to native roles. Both sources were restored
byte-for-byte. These are codec controls with supplied bindings, not real Presented
or keyboard delivery. A first sanitizer build lacked GCC sanitizer libraries and
a first isolated compiler run lacked private include paths; neither counted as a
pass. Corrected Clang and bounded include mounts produced the retained passes.
Evidence is `.artifacts/bemenu-native-wire/` in the coordinating root checkout.
Exact-source canonical validation remains a subsequent checkpoint gate.

## Native store ownership over 795a3d60

The runtime now has an immutable per-epoch native launcher storage profile. It
uses the existing allocation, resource, permit, candidate and response-credit
owners. Legacy constructors remain legacy, and their public request methods
refuse native records. This does not enable revision-7 transport negotiation.

Session-supplied opening identity stays on the pending and granted allocation.
Native allocations are parentless, limited to one per grant, and reserve no
workspace extent. A mismatching grant decision leaves the original proposal
owned. Candidate Begin attaches bounded ordered catalog slots and selected row
to the actual assembly; End checks current catalog availability, issued model
revision and exact allocation opening, and row targets must match that order.
The larger Begin and row bytes count against the candidate byte limit before
chunk admission. Renderer handoff rechecks current provenance before transferring
ownership and carries that same row binding with the real resource leases.

Chunk/End use the existing grant-wide candidate-generation correlation. Terminal
responses retain the original Begin transaction; this slice does not add a new
transaction-equality rule to the legacy assembly contract. Prepared and Presented
remain distinct caller-supplied renderer transitions, not inferred outcomes.

Ten device-hidden integration controls exercise the actual stores, including a
held submitted launcher bundle across disconnect while a separate bar uploads
under its own grant. Replacement waits for the held bytes; releasing them admits
the replacement without changing the bar's resource. The control supplies the
renderer failure/completion boundary and does not drive native rendering or bar
actions. Four separately compiled mutations fail the intended tests: omitted
allocation-opening comparison, ignored catalog row order, ignored current model
revision and omitted native metadata byte charge. Mutations run in an isolated
source copy and are restored; no live source is mutated. Evidence is retained in
`.artifacts/bemenu-native-stores/`.

The first test run had two incorrect fixture expectations that omitted the
already-owned ResourceReleased credit. The corrected assertions retain that
credit alongside candidate responses; production accounting was not weakened.
Protected role negotiation, live supervision, geometry/composition publication,
focus/input/activation and the Bemenu C backend are still required. These store
controls do not establish readiness for `lom-test`.

## Role transport over 009415a2

The independently constructed Session component owner now selects the immutable
store profile from its configured role before peer admission. A native launcher
must negotiate revision 7 and exactly application-catalog, content-surface,
content-discrete-input and native-launcher capabilities. It cannot request
indicator, descriptor-launch or switcher authority. The existing operator
content policy must also grant discrete input. Unavailable and denied remain
distinct encoded refusals, and refused negotiation retires only its reservation.
Legacy and live single-shell startup retain revision 6. The independent live
profile remains refused pending full supervision/focus/client integration.

Native resource, pacing, allocation and candidate intake shares one visit of at
most the negotiated record limit (capped at 32) and 64 KiB payload processing.
Each socket direction is additionally bounded to 64 KiB and retains partial
framing/writes. Oversized negotiated payloads refuse before record removal.
This is a per-connection content bound, not completed owner-loop fairness across
input, actions, supervision or renderer work. A new allocation/resource/demand
must have aggregate response capacity before inbox removal; candidate permits
already own their response credits. Resource transitions and event transfers
are shared with the legacy path, not duplicated resource ownership.

The native service uses Session-supplied opening, current catalog and issued
revision with the actual stores. Opening notifications and content outcomes use
the existing FIFO. The borrowed Session connection exposes the same methods as
the actual component owner. No focus, text issuance or application activation
is inferred from successful negotiation or supplied renderer completion.

Eight device-hidden socket controls cover the complete allocation/upload/pacing/
candidate roundtrip with supplied geometry/catalog and renderer completion,
exact source retention through disconnect, role widening, operator refusal,
wrong grant/legacy allocation, stale catalog, mixed-record bounds and buffered
payload continuation and EOF after a buffered request. The EOF control retains
the actual pending allocation until explicit disconnect; EOF alone does not
claim successful completion. Session's two-peer controls now use a real revision-6 bar
and revision-7 launcher, retaining old resources while the bar progresses. Their
protection evidence is supplied; no protected child, native presentation or
actual launch runs. The buffered-byte control preloads the inbox so the socket
read bound cannot hide a missing dispatch-byte bound. Evidence is retained in
`.artifacts/bemenu-native-transport/`.

Native text input still needs an explicit bounded response/issuance owner. Its
132 fixed payload bytes plus up to 256 UTF-8 bytes exceed the legacy 256-byte
control envelope. That next integration must reserve the complete frame through
FIFO/partial writes; this slice does not send native text as unreserved bulk.

Six independently compiled negative mutations in the disposable source snapshot
fail their intended socket controls: accepting widened role capabilities,
ignoring operator refusal, accepting a foreign grant, removing the payload bound
doubling the negotiated record bound and suppressing EOF notification. The source snapshot is restored after
each mutation. These are deterministic private-socket accounting/role controls,
not evidence of natural kernel saturation or physical input dispatch.

## Native focus and input over 6c65ccad

The transport now retains non-owning metadata from the actual candidate store's
successful Presented transition. Prepared cannot install focus. Focus names the
exact grant, opening, output, allocation, catalog, candidate, presentation,
interaction, state revision and a checked connection-local lease. The matching
Presented outcome enters the common FIFO first. A newer Presented disarms the
previous focus until the matching new focus is installed; stale callbacks must
also supply and match their original focus or opening, rather than acting on
whatever happens to be current.

Native control records use a 512-byte accounting envelope, sufficient for the
largest bounded UTF-8 input. Opening reserves its future Closed response; focus
reserves FocusRevoked; an unsent Enter reserves its eventual Input response.
These credits share the existing aggregate record/byte limits with resource,
candidate and FIFO owners. Encoding/refusal retains the producer; successful
enqueue is followed by Copy-state transfer without callbacks or I/O. Close
disarms first and retains refused notifications for polling. This is a returned
failure contract, not panic recovery or proof of displayed-resource withdrawal.

Sixteen fixed input receipts retain exact events across model replacement.
Wrong or duplicate ACKs cannot settle a different receipt. ACKed Accept metadata
remains distinct from application admission, which is not implemented here.
Enter while edits are awaiting presentation is retained against that exact
revision, with its original transaction and issuance time. Only matching focus
can issue it; another edit cancels the intent. Empty selection cannot retarget it.
Native candidate intake and renderer handoff check the transport's issued state,
not merely a caller-supplied revision.

The monotonic deadline visit checks unacknowledged input against the negotiated
ACK timeout and pending Enter against the presentation timeout. New issuance
also visits it. Expiry closes the exact opening; clock regression or a stale
opening cannot close a successor. Idle timeout enforcement still requires the
Session owner to call this visit before input/ACK service. Pixel consumers remain
owned independently; no resource release or launch is inferred from closing.

Nineteen private-socket controls pass, eleven new in this slice. They use real
stores, FIFO and encoded input/ACKs with supplied geometry, catalog, protection
and renderer transitions. Six compiled mutations fail their intended controls:
omitting expected focus, omitting expected opening, comparing only an ACK event
number, issuing Enter before its revision is presented, omitting timeout, and
trusting a stale caller revision at intake. Each disposable source is restored.
Evidence is under `.artifacts/bemenu-native-focus/`. The socket saturation control
counts all successfully queued bulk records, including those already written to
the kernel, before checking the two reserved terminal notifications. It is not
a latency or owner-loop fairness test. No physical input, supervised launcher,
catalog execution, native display or Bemenu backend has run in these controls.

The limit follow-up also constrains native receipts and unsent Enter by the
negotiated pending-action limit, shared with pointer cancellation obligations.
The sixteen-element array is only a storage ceiling. A smaller or zero grant
does not inherit that ceiling, and retiring a pointer cancellation or exact ACK
frees only its own slot. Native text also checks the negotiated frame-payload
bound before receipt/revision transfer, independently of its 512-byte control
credit. Four additional private-socket controls exercise zero/one-action grants,
shared pointer/native occupancy and an exact 304-byte payload boundary. The
pointer control supplies Session's target decision; it proves shared transport
accounting, not pointer authority. Evidence: `.artifacts/bemenu-native-input-limits/`.

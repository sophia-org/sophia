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

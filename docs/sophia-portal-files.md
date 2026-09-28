# Portal transfer file contract

**Design only.** There is no production portal listener, file export or
multi-namespace admission in the current desktop. The existing socket and
clipboard coordinator are test paths. This contract specifies their replacement
without claiming those capabilities are live. Exact layouts are in
[sophia-portal-files-v1.kdl](../protocol/sophia-portal-files-v1.kdl); the
[decision](notes/decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
records authority and compatibility choices.

## Admission and functionality

The portal decision owner serves a separate 9P2000.L export. Its requester must
have a trusted `PortalRequester` admission context identifying the exact
protected peer UID/PID, source/target namespace pair, connection epoch and live
namespace authority. This is a new required seam, distinct from `PortalBroker`
(the decision owner) and HostDomain. No such production requester admission is
claimed today. No endpoint environment variable is defined before that owner
exists. Attach strings and paths cannot create this admission.

Both namespaces must match the admitted pair. A foreign or invalid namespace
is EACCES before custody. The owner derives publish/request permissions from
the namespace registry; it never trusts permission bits supplied by a requester.
Policy still defaults to Deny. These scope checks deliberately replace the old
socket packet's self-asserted permission bits. Requests always enter Pending,
so its caller-supplied decision field is removed too. These are explicit
authority corrections rather than byte-for-byte socket compatibility.

Carry the implemented request/grant lifecycle, clipboard payload, completion,
failure, expiry and revocation. All seven transfer kinds remain representable.
Other family payloads, executors, prompt UI, production launch, multiple live
namespaces and descriptor transfer are absent functionality. A 9P request grants
none of them. In particular an absent executor cannot report successful execution.
Cross-namespace permission or lifecycle refusal remains a bare Denied; no reason
is disclosed. The returned denial still correlates the request's transfer ID.

## Files, records and phases

| File | Access | Meaning |
| --- | --- | --- |
| `api` | read | `sophia-portal-files version=1 role=portal-requester epoch=<epoch> fd_transfer=none` plus newline |
| `limits` | read | Immutable Limits and broker-generation object, available before negotiation |
| `events` | read | Handshake, Submitted, Decision and TransferOutcome journal |
| `transaction` | read/write | One candidate, at most 344 bytes |
| `submit` | write | Exact 24-byte control at offset zero |
| `ack` | write | Exact 16-byte cumulative control at offset zero |
| `upload/0` | write | One grant-bound payload slot with one writer |

The record header is 32 bytes. Objects have zero submission ID/sequence;
candidates have a nonzero submission ID and zero sequence; events have zero
submission ID and a nonzero sequence. All carry the admitted nonzero connection
epoch. Unknown kinds, malformed booleans, padding, lengths and enums are EINVAL;
a stale outer epoch is ESTALE before custody.

Staging, exact submit replay, immutable accepted bytes, submission watermark,
partial journal reads and acknowledgement follow the
[broker file custody rules](sophia-broker-files.md#files-and-record-identity).
They are file-custody rules, not transfer-ID reuse permission. A new transaction
cannot replace a submission whose Submitted is unacknowledged. One attach is
allowed per admitted connection. Qid paths never name a different node later.

Negotiate selects interface revision 1. A malformed body is EINVAL; an invalid
or unsupported revision range commits Submitted plus Refused atomically.
Success commits Submitted plus Negotiated. Another negotiation is EALREADY;
other candidates before negotiation are EACCES. Refused permits journal reads
and ack, accepts no further candidate, and closes after terminal ack or the
bounded drain deadline. Revocation remains distinct and fences reads immediately.
The drain deadline starts at Refused publication and cannot be extended by a
partial ack; the normal ack-progress reset applies only outside refusal drain.

## Requests, grants and payload custody

TransferRequest contains transfer, admitted namespace pair, kind, optional MIME,
byte-size hint, source generation and owner-clock deadline. MIME distinguishes
absent from present-empty UTF-8, preserving the old codec. A zero transfer ID,
source generation or deadline is representable and reaches lifecycle Denied,
as on the socket path. Such invalid requests create no retained identity.
Decision's transfer may be zero only when denying that invalid request; Allowed
requires a valid grant. Foreign namespaces are rejected by admission first.
With journal room available, expired, duplicate and active-capacity requests
also receive Submitted plus Denied, preserving the socket's reduced outcome.
A duplicate never changes its original request or grant or allocates another
terminal credit. This domain denial is distinct from exact submission retry.
Only a valid new identity can hit the new history ceiling and return ENOSPC;
journal pressure is EAGAIN before any lifecycle call.

For each admitted request, reserve Submitted, Decision and a potential terminal
credit before calling the lifecycle. Denied releases the unused terminal credit.
Allowed records the active grant with its exact namespaces, kind, source and
broker generations and deadline. Client and owner compare the grant's broker
generation with Limits. No user decision or grant can be inferred from an ack.

PayloadBegin names one active grant and slot zero, and declares at most 65,536
bytes. It reserves Submitted before allocating the payload. The slot binds the
transfer and the grant's source/broker generations. The first write-open is
its only writer; additional writers are EBUSY. Writes append or exactly repeat
a retained range; gaps, changed bytes and overflow are refused without mutation.
The aggregate payload allocation for the export is one 65,536-byte buffer.
A second grant cannot allocate another payload while the slot is occupied.
Only Clipboard has a payload encoding in this version. Begin for another kind
returns EOPNOTSUPP before custody; a generic Allowed decision alone does not
promise an implemented payload executor.

PayloadEnd must match the transfer, declared length and complete uploaded bytes.
Before its Submitted takes custody, reserve that receipt and validate the exact
active grant, namespace rights, generations, deadline, payload and existing
terminal credit. Only then may the executor receive work. This transfer from
upload custody to executor custody is the irreversible boundary. The slot may
be reused only when its buffer has been released, not merely when End is parsed.

Before End, TransferCancel reserves Submitted and settles the grant as Revoked.
Writer clunk, expiry or authority revocation settle through the same terminal
credit. End is single-use: Cancel/End after execution custody is EALREADY, and
a repeated Begin on the same grant cannot allocate another slot. An exact retry
of the still-retained submission follows the custody retry rule instead.
Old upload handles become ESTALE after their binding ends. No operation emits
two terminal outcomes. An executed effect is not described as unexecuted when
a connection departs: the executor owner settles its issued work.

Completed means the executor succeeded. Failed execution, cancellation and
revocation produce Revoked; deadline expiry produces Expired. These states
retain the existing portal lifecycle vocabulary. Generation/namespace changes
invalidate unexecuted grants. A disconnected peer never causes a fresh execution
or automatic replay on its replacement connection.

## Retained history and resource bounds

Defaults/ceilings: 64 pending/active transfers, 4,096 admitted transfer identities
per broker generation, one upload slot, 65,536 aggregate payload bytes, 344 staging
bytes, 255 MIME bytes, 256 journal records and 32,768 retained journal bytes.
Limits can reduce these within the KDL ranges; max-retained-transfers must be at
least max-transfers. The identity and active ceilings are shared by all of the
broker owner's connections. Payload/journal/staging bounds are per export.
Initially the owner admits one active requester export at a time, matching the
current serial socket service. Increasing this requires a reviewed aggregate
budget and admission rule; multiple parallel exports are not implied here.
After disconnect, admission also waits for any issued executor custody and its
payload buffer to settle. A replacement connection cannot accumulate another
buffer beside work retained from the previous export.

Keep admitted IDs after denial and settlement. Acknowledgement, clunk and
reconnect cannot clear them. Arbitrary nonzero caller IDs remain valid; there is
no new monotonic-ID requirement. A duplicate remains detectable at capacity.
History exhaustion returns ENOSPC before new custody while accepted work can
settle. Do not evict tombstones or silently advance broker generation. A
deliberate owner generation change revokes old grants and fences connections
before new admission. Issued execution must settle before old history is dropped
and a new generation begins. This finite-history ceiling is an explicit compatibility
change: the current lifecycle's active bound leaves its retained maps unbounded.

| Admission | Immediate journal reservation | Retained terminal credit |
| --- | --- | --- |
| Negotiation | Submitted 48 + Negotiated 48 (or Refused 40) | none |
| Request | Submitted 48 + Decision up to 88 | Outcome 48 if allowed |
| Begin | Submitted 48 | grant's existing credit |
| End | Submitted 48 | grant's existing credit, spent by executor settlement |
| Cancel | Submitted 48 + Outcome 48 | spends existing credit |
| Expiry / revocation / premature writer clunk | Outcome 48 | spends existing credit |

Reserve both record count and bytes before custody. Sixty-four outstanding
terminal credits require 64 records and 3,072 bytes that no new receipt or
Decision can consume. The longest accepted transfer has three receipts
(Request, Begin, End or Cancel), one Decision and one Outcome: five records,
280 bytes. Sixty-four plus negotiation would need 322 records and 18,016 bytes.
The 256-record ceiling intentionally refuses further admission earlier; the
calculation does not claim all 322 fit. EAGAIN changes no new identity, grant,
payload allocation or accepted submission. Already reserved outcomes still fit.

Assembly expires after at most 12 seconds from its first byte. A nonempty
journal requires ack progress within at most 2 seconds, even below capacity.
Only a strictly advancing ack resets that deadline. Peer reads, duplicate acks
and new requests do not. Payload lifetime is bounded by the grant's owner-clock
deadline, which is rechecked immediately before execution. All counters and
offsets use checked arithmetic; exhaustion refuses before mutation.

## Required replacement evidence

Retain the portal socket and CLIPBOARD/PRIMARY coordinator behavior as conformance
cases. Add exact protected-pair admission and foreign-namespace refusals,
permission-derived denial under Allow policy, and denial without reason leakage.
Prove incomplete/stale/expired/foreign payloads never execute; accepted payloads
settle once across cancellation and disconnect races. Separate active-limit,
history-limit and journal-limit tests; include duplicate IDs after settlement
and reconnect, no implicit generation rollover, and both byte/record terminal
reservations under pressure. Require independent C and Rust SDK clients against
the real export, with refusal draining and final-ack deadlines. Layout checks
prove none of these runtime properties by themselves.

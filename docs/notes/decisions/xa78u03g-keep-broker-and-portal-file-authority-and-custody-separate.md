---
id: xa78u03g
date: 2026-09-28
kind: adr
status: accepted
tags: [adr, broker, portal, protocol]
---
# Keep broker and portal file authority and custody separate

## Context

Reviewed against Sophia 80df2c4ce. This design specifies future exports;
it does not claim that the portal is running in production.
It supersedes the conflicting rules in the earlier evidence-only drafts,
which remain unchanged as review history. The reconciled
[broker contract](../../sophia-broker-files.md) and
[portal contract](../../sophia-portal-files.md) carry native KDL layouts and
explicit implementation acceptance requirements.

## Shared transport and custody rules

Use separate, explicitly admitted 9P2000.L exports and the existing desktop
SDK repositories, one per language. Neither export tunnels a socket frame.
Transport selection is explicit; there is no sniffing or fallback. The Engine's
internal typed channels remain separate from these process interfaces.

One attach belongs to one admitted connection epoch. Peer identity and the
trusted admission record confer authority; attach strings and request fields
do not. Revocation fences old fids, staged candidates and replies. A fresh
connection never implicitly resubmits work from a departed connection.

Reserve every receipt and immediate consequence before taking custody. A
failed reservation changes no submission watermark, domain identity or state.
While an accepted submission is retained, only its exact retry succeeds;
acknowledgement releases that retained submission, after which its ID is
EALREADY. The next transaction cannot replace unacknowledged custody.
Assembly is bounded at 12 seconds from its first byte, without extension by
partial writes. Acknowledgement progress is bounded at 2 seconds whenever
retained records remain, even if the journal has not filled. Repeating an ack
does not extend its deadline. Reads and unrelated requests do not count as ack
progress. Lower advertised deadlines must be nonzero.

Negotiation either commits Submitted plus Negotiated atomically, or Submitted
plus Refused atomically. Refusal enters a drain state: no new candidates;
existing journal bytes remain readable until the terminal ack or the 2-second
deadline. Only then is the export closed. Pre-attach permission failures are
errno refusals and disclose no role data. Revocation is distinct from this
negotiation-refusal drain and can invalidate reads immediately.
The refusal deadline starts at Refused publication. A partial ack cannot
extend it; the normal ack-progress reset applies outside refusal drain only.

## Metadata broker

Session serves SOPHIA_BROKER_9P_SOCKET to the protected metadata-broker child.
Preserve MetadataBroker evidence, exact kernel peer UID/PID, single-peer
admission and the existing no-reconnect failure policy. The current production
child is a real consumer; ordinary host-user reachability is not admission.

Keep all five request kinds, all five response kinds and rejection values
1 through 5. In particular InvalidConnectionEpoch=5 remains a valid single
Rejected response for a pending request. It is distinct from ESTALE on a wrong
outer connection epoch. Attention and disclosure requests remain implemented
even though Session currently has no production sender for them.

Replace the existing single-response restriction with one ResponseSet of one
or two ordered rows. The only two-row response is PublishRule followed by
EmitDescriptor where the reducer produces both. Validate the whole set against
the pending request and prepare every Session effect before applying any row;
a second-row failure cannot deliver the first rule. Backend enqueue capacity
is part of that preparation, not a fallible step after the first effect.

Preserve the old codec's distinction between absent text and present empty
UTF-8 text, including its accepted control characters. Label reduction and
disclosure stay with the existing authorities, not a stricter replacement codec.
Preserve optional-icon representation, including a present zero value where
the old codec permits it. Raw surface IDs in requests reach the reducer's
UnknownSurface rejection; structural parsing must not erase that response.
Authority-bearing returned surface/generation/grant values still require the
existing owner checks. None of these choices permits raw application metadata
to reach the WM or shell outside the current disclosure rules.

The grant revocation epoch remains 1, as implemented. Generation and retirement
checks remain mandatory. Dynamic grant revocation requires its own model and
descriptor-contract change; a carried field does not prove that behavior.

### Broker capacity calculation

Use at most 16 journal records and 4,096 retained bytes, a 512-byte staging
buffer, one pending request and one pending ResponseSet. The largest response
candidate is 448 bytes. The existing 5-second response deadline starts at
request publication.

Handshake reserves two records, at most 104 bytes (48-byte Submitted plus
56-byte Negotiated; Refused is 40). Publishing a 200-byte BrokerRequest also
reserves its eventual 48-byte Submitted receipt. The next request waits until
the prior response is applied and its Submitted is acknowledged. Thus the
maximum unacknowledged paced batch is four records and 352 bytes, including
the handshake. Capacity refusal leaves an unappended Session request in its
bounded owner slot. A response cannot consume the reserved receipt twice.
No subsequent request is necessary to start the final ack deadline.

## Portal

There is no production portal listener or multi-namespace admission today.
The socket and clipboard coordinator are test callers. Keep that distinction;
do not invent a live endpoint variable or imply installed portal capability.

The future exporter receives a trusted PortalRequester admission context:
exact protected peer identity, connection epoch, source/target namespace pair,
and references to revocable namespace authority. This requester role is distinct
from the PortalBroker decision-owner role and HostDomain. Its implementation
must introduce that explicit admission seam; merely reusing either existing
role is incorrect. A requester cannot select an arbitrary namespace pair by
writing IDs. Check the admitted pair before custody and recheck live permissions
and generations immediately before executing a transfer.

The owner derives publish/request permissions from the namespace registry.
Remove the socket packet's self-asserted permission bits and implicit decision
input (a request starts Pending). This is an explicit security correction, not
byte-for-byte socket parity. Foreign namespace IDs return EACCES before custody.
Preserve present empty UTF-8 MIME values where the old codec permits them.
Ordinary policy or lifecycle denial remains an undifferentiated Denied event;
do not disclose its reason. Malformed encoding remains an errno refusal.

Carry the implemented request/grant lifecycle and clipboard payload. All seven
transfer kinds remain representable, but the other family payloads, executors,
prompt UI, production launcher and descriptor passing are absent capabilities.
They must not be synthesized by transport migration. An operation requiring
an absent executor cannot report successful execution.
Only Clipboard has a payload encoding in this version; PayloadBegin for another
kind returns EOPNOTSUPP before custody. Allowed alone promises no executor.

### Portal retained identities

Active capacity alone is insufficient: the current lifecycle keeps request and
grant entries after settlement. Introduce a separate finite ceiling of 4,096
distinct admitted transfer identities per broker generation, shared across its
connections. At most 64 are pending/active. Retain each admitted identity after
denial or settlement; journal ack, clunk and reconnect never erase it. IDs remain
arbitrary nonzero caller values, not implicitly monotonic IDs. Duplicates remain
detectable when the new-identity ceiling is reached.

Exhausting the identity ceiling returns ENOSPC before new custody, while accepted
work can settle. Do not silently evict tombstones, revoke active work or advance
the broker generation to regain space. A deliberate generation change is a
separate owner lifecycle operation: old grants are revoked and old connections
fenced before new admission. Malformed pre-custody requests create no entries.
This finite-history rule is a deliberate compatibility change and must be
advertised in Limits and tested independently of the 64-active bound. The
existing unbounded maps cannot satisfy it without an owner change.

### Portal payload and journal accounting

Use one upload slot and at most 65,536 payload bytes in the whole export, not
one allocation per grant. A slot is bound to an active transfer, source
generation and broker generation. The first writer owns it; stale fids cannot
write a later binding. One candidate staging buffer is at most 344 bytes.
At most one accepted candidate awaits its custody ack.
The owner initially admits one requester export at a time. After disconnect,
issued executor custody and its buffer must settle before replacement admission.
Parallel exports require a separate aggregate budget and admission decision.

Use 256 journal records and 32,768 retained bytes. On TransferRequest, reserve
Submitted (48), Decision (up to 88) and, before granting, a terminal Outcome
credit (48). Denial needs no terminal credit. PayloadBegin reserves its own
Submitted (48) before allocating/binding the slot. PayloadEnd reserves its own
Submitted (48), validates the complete payload and grant, then transfers
execution custody; its existing terminal credit covers completion or failure.
TransferCancel reserves Submitted (48) and spends the existing terminal credit.
After End takes execution custody, Cancel is refused rather than manufacturing
a second outcome. Clunk, expiry and revocation spend the same single credit.
Repeated begin/end/cancel cannot create another operation or terminal credit.

The longest accepted lifecycle has Request, Begin, End (or Cancel): three
Submitted receipts, one Decision and one terminal Outcome. That is five records
and 280 bytes per transfer; 64 such lifecycles plus the 96-byte successful
handshake would require 322 records and 18,016 bytes without acknowledgements.
The 256-record bound intentionally backpressures earlier: **322 is not claimed
to fit**. Admission checks retained bytes/records plus all terminal credits
before every operation. Sixty-four outstanding terminal credits require 64
records and 3,072 bytes and can never be consumed by further receipts or
decisions. Consequently all existing grants can settle even when ordinary
admission is EAGAIN. Capacity refusal retains no new payload or lifecycle state.

Before an executor runs, validate payload completeness, exact transfer binding,
namespace permissions, source and broker generations, deadline and terminal
reservation together. No irreversible action occurs before those checks.
Disconnect fences upload handles and cancels unexecuted grants; already-issued
executor custody must be settled by its owner, never reported as unexecuted.

## Implementation evidence required

The wire-layout checks validate complete byte coverage, kind classes, preserved
rejection values and custody arithmetic directly from both KDL files. Eight
malformed-layout controls cover overlap, gaps, row overflow, duplicate kinds,
wrong kind class, undersized body, shifted generation and reversed bounds.
They are design checks, not independent clients or runtime state-machine proofs.
Before replacing either socket, require independent C and Rust SDK exchanges
with the real export, malformed/replay/capacity controls, protected admission,
and a deliberate refusal-drain/ack-deadline test. Broker tests must kill partial
two-row application and prove rejection 5 versus wrong-epoch refusal. Portal
tests must distinguish journal pressure, active capacity and retained-history
capacity, including reconnect and duplicate IDs after settlement. Executor
tests must prove no action on an incomplete/stale/foreign transfer and exactly
one terminal outcome across cancellation races.

No old socket is removed by this design. The correct WM-file owner reference is
crates/sophia-session/src/live_session/policy_transport_worker/ninep/. Existing
metadata-chain, broker transport, portal socket and X clipboard tests remain the
behavioral inventory for their replacements.

## Alternatives

Leaving these process interfaces on IPC would contradict the requested desktop
direction. Combining them with the WM, shell or administrative export would
merge unrelated authorities. Retaining only active transfer IDs would allow
settled IDs to be replayed; requiring monotonic caller IDs would change their
existing arbitrary-order semantics. Finite history with explicit exhaustion
preserves those semantics within an advertised bound.

## Acceptance and connections

Accepted as a design on 2026-09-28 under niltempus's instruction to complete
t264–t274 and use native 9P interfaces through one SDK per language. It was
first published as proposed in c70cfae53. Root reconciled all eight findings
from the draft review against the existing codecs and owners, then checked the
source KDL with four layout tests, including eight malformed-layout controls.
The contracts linked above replace the earlier evidence-only wire drafts.

Acceptance covers authority, custody, bounded resources and the target layouts.
It does not accept an export implementation, SDK implementation, default change
or socket removal. The finite portal-history ceiling and separate requester
admission context still require owner changes and regression evidence. The
implementation evidence above remains mandatory; t255 qualification is not
waived by finishing t273's design work. Review and gate evidence are recorded
in the linked investigation.

- [IPC inventory and t273 review](../investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
  owns the migration evidence and points to the prior draft review.
- [Public 9P direction](../../sophia-9p-control-bus.md) preserves separate
  authorities and does not grant descriptor transfer through file writes.
- [Native descriptor contract](../../sophia-shell-files.md) consumes the broker's
  action-grant identities; dynamic revocation must change both contracts.
- [Desktop role plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  keeps portal production wiring separate from the current desktop migration.

Review sources at 80df2c4ce: `sophia-protocol/src/ipc/broker_v1.rs` and
`ipc/primitives.rs` for rejection and optional-text representations;
`sophia-broker/src/metadata.rs` and the CLI's `commands/runtime/brokers.rs`
for the reducer's two-command result and current wire refusal;
`sophia-session/src/live_session/metadata_broker.rs` for admission and effects;
`sophia-portal/src/{lifecycle,broker,socket}.rs` for retained identities,
permissions and execution. These crate-relative source paths are under `crates/`.

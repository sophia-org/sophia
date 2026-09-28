---
id: vkkjmufd
date: 2026-09-28
kind: adr
status: proposed
tags: [adr, output, protocol]
---
# Use native records for the separate output file role

## Context

The output role still uses its socket transport. Its topology vocabulary and
admission owner are now independent of that transport. The operator's 9P-only
direction requires a native file contract without retaining an IPC frame inside
file writes. Output authority remains separate from WM layout authority.

## Decision

Use the common file-record identity shape (version, kind, epoch, submission ID,
journal sequence), with native fixed rows for output proposals and topology.
Keep the existing topology operations, validation and outcomes. The
[implementation draft](../../sophia-output-files.md) records the first codec
slice; it does not advertise an available export or change a transport default.

Parsing must distinguish malformed records from semantic refusals. Unknown
requested capability bits reach negotiation for intersection. A validly encoded
proposal with an unknown head, stale generation or invalid geometry reaches the
topology owner, preserving its reduced refusal and transaction replay rules.
Reserved bytes and unused fixed-row slots must be zero.

## Alternatives

Embedding the output socket envelope would preserve the dependency being
retired. Reusing shell content records would conflate distinct authorities and
operations. Neither is proposed.

## Consequences

The codec can be tested without an IPC module or display. It does not yet settle
export custody. Before the export lands, specify bounded journal reservations
for every accepted proposal's terminal outcome, immutable topology retention,
terminal negotiation-refusal draining, and bounded command/event channels.

The current owner remembers every domain transaction ID in an epoch. Journal
acknowledgement does not bound that history. The export must explicitly bound
this history without losing accepted outcomes or silently imposing a monotonic
transaction-ID rule on existing clients. Submission replay and domain replay
remain distinct concerns.

## Acceptance and connections

Proposed. This codec work is not acceptance of the export, protected admission,
performance or native topology behavior. The
[t253 plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t253--migrate-the-separate-output-role)
owns those exits. The
[IPC inventory](../investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
tracks removal evidence for t272. The current
[socket contract](../../sophia-output-v1.md) remains supported until replacement
acceptance.

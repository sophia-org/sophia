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

The codec and custody primitives can be tested without an IPC module or
display. Each accepted proposal reserves one terminal event (56 bytes), with
at most one active and one queued proposal. Receipts and immediate consequences
publish as atomic batches. The export now joins this custody to its
submission/acknowledgement lifecycle, immutable topology retention, terminal
negotiation-refusal draining, and bounded command/event channels. Session
integration and physical-owner recovery remain separate gates.

The proposed Limits record advertises at most 4,096 domain transactions per
epoch. The bounded owner constructor enforces that history independently of
journal acknowledgements, preserving arbitrary domain-ID order. A new identity
at capacity is refused before mutation; accepted work may still settle. Only a
newer connection epoch clears history. The client drains accepted outcomes
before reconnecting; the export must not revoke those outcomes on exhaustion.
Submission replay and domain replay remain distinct concerns.

## Acceptance and connections

On 2026-09-29 niltempus approved the standalone `sophia-output` CLI as the
product consumer and authorized proceeding with the output migration. The CLI
uses the public desktop SDK, with product behavior tested in its own repository.
This resolves the missing consumer choice. Deterministic export custody and
supervised 9P tests now pass; independent-client recovery, live Session
integration and the plan's native acceptance gates still need implementation
and evidence. The record design remains subject to those checks.

Proposed. This codec work is not acceptance of the export, protected admission,
performance or native topology behavior. The
[t253 plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t253--migrate-the-separate-output-role)
owns those exits. The
[IPC inventory](../investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
tracks removal evidence for t272. The current
[socket contract](../../sophia-output-v1.md) remains supported until replacement
acceptance.

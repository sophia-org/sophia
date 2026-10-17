---
id: 7mls2e2q
date: 2026-10-17
kind: adr
status: accepted
tags: [adr, shell, performance, 9p]
---
# Reuse a shell transaction writer without changing custody

## Context

The historical t331 ledger reconciles 53 panel requests and 152 panel-plus-bulk
requests per steady frame. Five panel transactions and eight bulk transactions
each open and clunk a fresh writer. Pixel chunks already fit the negotiated
frame size; larger transport writes would not remove this control traffic.

## Decision

Add the optional `transaction-reuse` node, retaining `transaction` unchanged.
One writer stays open across submit/ACK cycles under the same single-candidate
allowance. The next offset-zero write rearms only after the covering ACK.
Assembly expiry permanently fences the token; an old operation cannot affect
a successor. Reuse one bounded record buffer, with its capacity charged.

The [normative amendment](../../sophia-shell-files.md#reusable-transaction-writer)
owns the states, errors, compatibility, storage and cancellation rules. C and
Rust SDKs discover the node after negotiation, with legacy selection only when
lookup returns `ENOENT`. Future per-export capability discovery may list the
node; support does not change shell role capabilities or authority.

## Alternatives

Silently rearming `transaction` would change old handles' stale behavior.
Changing the strictly parsed API line would reject old clients. A role capability
would couple transport scheduling to revision-gated desktop features. An optional
node preserves both old clients and old exports without a version flag day.

Implicit resource End, retirement bundled with a new candidate, larger chunks,
upload-fid reuse and speculative submit remain separate designs. The explicit
upload and presentation barriers continue to protect retained pixels and custody.

## Consequences

Against the historical ledger, reuse removes 15 panel or 24 bulk requests per
steady frame, before ACK-policy changes. Its one-time walk/open belongs to startup.
Clunk was already asynchronous, so removing it does not remove a waited-on reply.
Owner passes, CPU and allocation savings must be measured independently.

`Staging::new` uses an empty vector and does not itself allocate. First writes,
request handling and owner passes have distinct costs. This slice is not assumed
to satisfy the +10% gate by itself. Fresh attribution and native evidence decide
whether another bounded change is needed; the acceptance ceiling does not move.

## Acceptance and connections

Created as proposed, then accepted on October 17, 2026 when niltempus directed
implementation of the reviewed t331 plan and amendment. Acceptance authorizes
implementation, not a performance result or installed-desktop change.

- [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md)
- [Request ledger and attribution](../investigations/2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md)

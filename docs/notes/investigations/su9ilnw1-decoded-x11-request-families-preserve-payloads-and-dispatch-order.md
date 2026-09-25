---
id: su9ilnw1
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, x11, architecture]
---
# Decoded X11 request families preserve payloads and dispatch order

## Question

Can the last source-layout debt row be removed by grouping decoded requests,
without changing protocol bytes, validation order or resource ownership?

## Evidence

Baseline: `e5e38de3`, with 265 variants in the 1,355-line
`wire/request.rs`. The existing enum mixed core requests and extension families.
This is the representation follow-up explicitly admitted by
[t240](../plans/queue-11-parallel-production-readiness.md#t240).

## Finding and resolution

`XWireRequest` retains its authority-packet variant and wraps fourteen decoded
families. Core X11 requests occupy one 563-line file; extension groups cover
SHM, DRI3, XFixes, Present, Render, Shape, GLX, XTEST, RandR, XKB, Sync and XI,
with small server-discovery/negotiation requests grouped separately. The public
types remain re-exported through the existing wire facade. The family payloads
are ordinary passive enums; no generated-variant machinery or byte encoding
change is involved.

Decoders, dispatch matches, socket observations and fixtures now construct or
match nested variants. Dispatch routing and validation remain in their original
order. Descriptor-count handling still admits one descriptor for legacy DRI3
imports, fences and SHM AttachFd, or the declared count for multi-buffer DRI3.
The final debt row is removed rather than increased or replaced.

## Validation and remaining work

The complete authority suite and authority Clippy pass; all workspace targets
compile. A disposable token comparison reversed the wrappers in all 78 modified
existing Rust files and confirmed unchanged tokens after ignoring comments and
Rustfmt's optional trailing commas. One test closure gained only Rustfmt body
braces around its request expression. All 264 moved payload declarations retain
the same tokens. The raw layout audit reports no new violation.

The core probe, main-tree layout gate and selected-core/xproto/events profile
gates remain required before closure. t240 stays open until those results are
retained. No physical acceptance is claimed.

## Connections

The [t026 source-layout investigation](izw9opes-private-session-tests-can-move-without-widening-the-production-api.md)
explains why this enum representation change was separated from source moves.
The [source-layout ledger](../../source-layout-debt.txt) remains the exact list
of admitted oversized source units.

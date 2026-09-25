---
id: su9ilnw1
date: 2026-09-25
kind: investigation
status: closed
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

Signed candidate `f0d78651503e3d6cfce3bb60c9f4f52080ec36f5` passed the core
probe (162/162), main-tree layout gate, and all three X11 profile gates on a
clean, unchanged checkout. Each profile ran native-input (40/40) and XTEST
(44/44). XTS completed every manifested purpose with the existing expectations:
selected-core 76 passed/23 declared, xproto 339/50, and events 118/77 with
XTEST admission. Declared dispositions are retained baseline limitations, not
new passes. No expectations were weakened or rewritten.

The first selected-core attempt supplied a hyphenated manifest filename instead
of `xts_expected_selected_core.json`; its XTS result was BLOCKED despite the
wrapper's overall PASS. It is not acceptance evidence. The corrected run in
`x11-profile-f0d78651-xts-selected-core-rerun` supplies the accepted result.

Checksummed evidence is retained under
`~/.local/state/sophia/development-evidence/t240-request-families-f0d78651/`:
authority/Clippy/workspace logs, the reversible token comparison, core report,
profile reports and XTS journals, selections and expectations. The source-layout
debt ledger now has no rows. These deterministic results close t240; no physical
acceptance is claimed.

## Connections

The [t026 source-layout investigation](izw9opes-private-session-tests-can-move-without-widening-the-production-api.md)
explains why this enum representation change was separated from source moves.
The [source-layout ledger](../../source-layout-debt.txt) remains the exact list
of admitted oversized source units.

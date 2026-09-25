---
id: v2yrv8je
date: 2026-09-24
kind: investigation
status: closed
tags: [investigation]
---
# Native family audit makes output layouts explicit without changing frozen WM bytes

## Question

For [t022](../plans/queue-08-cp-15-1-native-protocol-family-lifecycle-audit.md),
do WM, shell and output expose one documented lifecycle without assuming
identical payloads or overstating experimental role stability?

## Evidence and resolution

The audit starts from signed master `b62d7bd9` in worktree branch
`native-family/t022`. It follows the three role schemas/codecs, runtime
`policy_ipc`, `shell_transport` negotiation/profile owners, `output_ipc`,
`output_transport`, `output_service`, and the live Session output owner.
The earlier [capability audit](vle7mt47-native-desktop-capability-audit-separates-contracts-from-client-ui.md)
identified output's missing schema but did not reconcile lifecycles.

The common contract incorrectly advertised shell r6 and no production content,
omitted output's implemented live owner, and described all welcome capabilities
as intersected requests. Shell uses requirements with a baseline and
supervisor-selected r7/r8 profiles. These profiles reuse the same protected
content owners under explicit revision gates. WM and output intersect requested
bits. The common audit table and role chapters now state these differences.

`protocol/sophia-output-v1.kdl` extracts all five existing output messages,
nested records, ceilings, enum/flag values and samples. A separate generator
module produces tables and valid/malformed vectors directly from KDL without
using the production codec. No production wire bytes, kinds or enums change.
Frozen WM artifacts remain unchanged. Shell welcome's field order, output's
independently chosen proposal transaction, and single-frame versus counted
transfers are documented specializations, not layouts to unify by force.

Output's signed coordinate fields admit only nonnegative logical origins at
owner validation. Its reason u16 is deliberately open, unlike its outcome enum.
The nested count ceilings fit one payload, so chunk frames are unnecessary.
The schema and role prose expose these constraints to independent implementers.

## Validation and limits

The protocol/runtime suite passed 435 tests before two final boundary tests
were added; the final output schema suite passes all five tests. It checks
decoded models and re-encoded frames, nested count/reserved/enum/UTF-8 mutations,
every truncated prefix, trailing bytes, public bounds, signed-coordinate
admission and the largest published snapshot. Clippy for protocol/generator
all targets passes. A negative control incremented the production snapshot
head generation: the schema test failed as expected; restoring the codec
returned it to green. Worktree artifacts retain `t022-tests.log` and
`t022-negative-control.log`.

The initial layout gate found two preexisting stale ceilings on `b62d7bd9`:
X authority windows dispatch is 1406 lines against 1405, and wire request is
1355 against 1351. Neither file changes here. The owning agent confirmed both
ledger corrections in its candidate `053a7d1b`; rebase and final layout check
await that candidate's gates. The initial full build also exhausted `/tmp`;
moving this agent's caches to the worktree's disk-backed `.artifacts` allowed
the tests to finish. Neither incident is a protocol failure.

Independent output lifecycle interoperability, the family conformance entry
and retained compatibility are [t023](../plans/queue-09-cp-15-2-one-family-level-conformance-surface.md).
This audit does not declare shell/output stable. No display, input device,
installed configuration or live session changed. Physical acceptance remains
separate from deterministic evidence.

## Contracts

Final acceptance: schema/audit commit `76e51913` rebased on `bf968409`, then
validated with family candidate `60fb80e9`. The corrected full family gate
passes, including 437 protocol/runtime tests and all nine live output-owner
tests with `native-session` enabled. The rebased layout gate, fmt, diff checks
and protocol/generator/xtask clippy pass. Evidence is retained at
`~/.local/state/sophia/development-evidence/native-family-60fb80e9`.
All t022 audit exits are met without changing production wire semantics.

- [Native family](../../sophia-policy-ipc.md): common lifecycle and audit map.
- [WM](../../sophia-wm-api.md): stable r3 negotiation and transactions.
- [Shell](../../sophia-shell-v1-direction.md): profiles, grants and retirement.
- [Output](../../sophia-output-v1.md): independently implementable r1 lifecycle.

---
id: nywg1vat
date: 2026-09-18
kind: milestone
status: recorded
tags: [milestone, x11, validation]
---
# M3 integrated acceptance checkpoint and remaining C controls

This records an evidence review under [t093](../../../todo.md) and its
[M3 plan](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md).
It does not close M3 or change its acceptance requirements.

The latest combined result is **18 of 20 PASS, zero FAIL**, on signed source
`a2bc967c585c6c58e100a3baae7d5140eb125bb9`. The contained report is
`.artifacts/m3-finish/c-acceptance-run-08-merged/report.json` in the common
repository. Source content SHA-256 is
`76b0b83b36e433f038348c8cbeac3f33c007f590269849abf0828ee2ddf4f862`;
binary SHA-256 is
`c8056b98fe2e915eda06199e5fef506b85b5bb1777f985eca8fb1456dd2c1d17`.
The fresh build target, source attestation inside containment and
unchanged-source check are recorded in that report. All A, B and D cases and
four C cases pass together. C.indeterminate_send and C.control_cleanup remain
unbound, so the aggregate is NOT_RUN.

The earlier signed source `8b4be691e3d71459102245a1dccf6d15ea2ca401` was run
through `cargo xtask check m3-acceptance`. The report is
`.artifacts/m3-finish/parent-all-bound-18/report.json` in the common repository.
Source content SHA-256 is
`d570281892d5ae6e80e874d0e260e806e6eb11b1be337f7d11ab8e9db506ad2e`;
binary SHA-256 is
`02363d3340b39afd51df90ff9a25caa467be166b8c1ebb39f815d52b8745328d`.
Source attestation inside containment and the unchanged-source check passed.

All three A cases, four B cases, seven D cases, C.poison and C.exact_origin
passed: 16 of 20. C.capacity failed because one production turn drained the
six queued items, contradicting its assumed remainder. C.interrupted_ownership
failed because the exact release remained in the turn beside its hold, before
the transition to settlement that its assertion assumed. These failures require
deterministic controls of the relevant stages; they do not permit removing the
identity, remainder or credit assertions. That run's aggregate remains FAIL.
The successor establishes current ownership and a remainder with one production
step before the service turn, then compares credit with every item still owned.
The interruption control checks the retained hold's client/window identity and
exact credit without requiring a particular intermediate list.

C.indeterminate_send and C.control_cleanup remain unbound. The former needs
a demonstrated prefix of the same delivery and a true unknown handover, with
all actors collected. Earlier completed frames are not that prefix. The failed
unknown-handover run also called `finish` on its third, still-running fixture;
the retained audit identifies the missing stop/collection prerequisite, not an
established production collection omission. A corrected diagnostic now performs
the true handover unwind, collects all its actors and visits maintenance before
checking that the handover was not replayed. Its audit report is
`.artifacts/m3-finish/c-indeterminate-unwind-review.md`.

Control cleanup has a separately tested original-source Configure path.
All-nine-kind integration remains required before binding its C case. An
unresolved publication, missing receipt or peer dependency must stay owned.
Review also requires successful peer writes to discharge their original debt;
indefinite retention after a proven success cannot substitute for cleanup.

Supporting live-recipient controls passed seven of seven on `d9e5e874` and
again after restoring identical source as `4b7c41c6`. Compiled negatives
`e6c829bd` and `819bf570` each failed the intended control: recipient termination
cannot supply native reconciliation, and StateOnly disposal requires its
explicit output disposition. Reports are under `.artifacts/m3-finish/` in
`parent-live-stateonly-01`, `live-proof-false-native`,
`live-proof-untyped-absence` and `live-proof-restored`. These component results
are separate from the twenty-case aggregate.

The harness now keys build directories by archived source content, including
file modes. Reusing a fixed target after restoring older archive timestamps
can reuse a mutant binary; such runs are excluded from acceptance evidence.
Exact diagnostic tests may run under `m3_acceptance::diagnostics::` through the
component gate. That namespace is rejected by acceptance bindings. The gate
change passed 24 contained self-tests; its report is
`.artifacts/m3-finish/diagnostic-gate-selftest/report.json`.

Strict all-target, all-feature X-authority Clippy passed on `8b4be691`.
The formatting check found two line-wrap differences in the harness cache
helper; the accompanying documentation checkpoint corrects them. No full
workspace or physical acceptance is claimed. M4–M6, public XTEST discovery,
Session integration and hardware remain outside this evidence review.

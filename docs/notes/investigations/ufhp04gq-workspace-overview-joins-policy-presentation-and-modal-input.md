---
id: ufhp04gq
date: 2026-09-25
kind: investigation
status: resolved
tags: [policy, shell, validation]
---
# Workspace overview joins policy presentation and modal input

## Paired scope

Sophia t241 owns its implementation and paired acceptance for the user's
Super+O workspace overview. Hagia h002 remains the feature owner, with the
cross-repository plan at Hagia's
`docs/notes/plans/64ac6jf6-workspace-overview-across-hagia-narthex-and-sophia.md`.
The user authorized that feature across Hagia, Narthex and Sophia, and assigned
coordination of the three Herdr agents to the director on 2026-09-25.

Hagia owns overview layout, navigation and selection policy. On 2026-09-25
niltempus corrected the first checkpoint's Narthex-owned navigation. The generic
candidate now keeps these decisions entirely in Hagia; Narthex is not part of
the overview path. Engine samples retained scene sources and publishes exact
presented actions. Only confirmed selection changes committed workspace/focus.
Client geometry and allocations do not become thumbnail-sized.

The overview uses independent output-local strips and preserves each output's
workspace. Super+O toggles it; arrows and hjkl navigate; Page Up/Down change the
selected workspace; Enter or a presented pointer target selects; Escape cancels.
Hotcorners, dragging and
unrelated shell features are outside this scope. No installation or live reload
is authorized by the development work.

## Buildable checkpoint, not feature acceptance

The implementation agent reported signed checkpoints:

- Sophia `46dfc4da825b94003684dc828d732b0687dc09b3`, on `97a6b6f6`.
- Hagia `e6cfb41df2f8d8c2564a4032a93931a33941cadd`.
- Narthex `7f51175be24e8a4930d2c12ca2c35e520f879c9d`.

Its reported checks cover native-session compilation, four Hagia overview
tests, four independent WM wire tests, four Narthex overview tests, three Rust
WM overview tests, one CPU clipped-preview pixel test, code generation and both
Nim layout gates. Session integration and end-to-end ownership/input acceptance
are unfinished. These results do not close t241 or h002.

## Sophia acceptance boundary

The joined production path must prove:

- Negotiated, bounded generic WM records, independent Nim/Rust codec checks,
  and unchanged behavior for peers without the presentation capabilities.
- A hidden surface referenced only by a preview is available to CPU and native
  frame construction. Its content updates repaint the thumbnail without
  changing client geometry, workspace or focus. Submitted frames retain their
  exact sources until retirement, including after overview close or withdrawal.
- Target mappings belong to the exact WM connection, publication, target, output
  and presentation generations. Only an actually presented candidate admits
  selection; a stale reply cannot install a mapping or resurrect a closed overview.
- Close, topology change, surface loss and WM reconnect revoke input
  authority immediately. Swallowed key/button releases remain accounted for
  after modal capture ends. Existing switcher, help and launcher paths retain
  their semantics.
- Headless joined tests cover navigation, empty workspaces, selection, cancellation,
  source replacement, stale candidates and failure/recovery. The full relevant
  workspace, family compatibility and layout gates pass on the signed paired
  candidate before merge. Physical acceptance remains separately identified.

The director's first source review found source-generation normalization in the
production overlay and preview references in head-plan source selection. The
pixel test directly constructs a clipped layer; it does not yet prove those
production joins. A preview-only source update and close-during-retirement
control was requested from the implementation agent.

This work extends the [WM contract](../../sophia-wm-api.md) and
[native component contract](../../native-desktop-capabilities.md); those
normative documents must be reconciled with the final implementation.

## Generic joined candidate, 2026-09-25

The preceding prototype checkpoint remains historical evidence. The generic
implementation is now joined on Sophia `rendering/foundation` through
`f677317d`: protocol `89f5edf4`, renderer/source work through `8329705b`, runtime
capability ceiling `829cba99`, paired Hagia controls `af92d767`/`982ac6dd`, and
session input `eaac9451` (cherry-picked as `f677317d`). Hagia `983dd83` includes
the independent generic codec, WM-owned policy, adapter targets and receipt
lifecycle, and the four paired controls in its contributor gate.

The full Hagia `nimble verify` run passed: 319 Nim cases, both eleven-scenario
policy corpora, profile admission, pointer focus, four real-Hagia presentation
controls, launch origin, eight Alloy assertions, Z3 expectations, and four TLA+
lifecycle checks. Its log is
`~/.local/state/hagia/development-evidence/h002-joined-verify/verify.log`, SHA256
`c29d48537bb1d5519ccc3f04161007bb2772573664234d6083c72a5214686ff1`.
The earlier run stopped at a disk-capacity error during Rust compilation; its
log remains under `h002-84e717d-verify`. The rerun uses a disk-backed Rust target.

The four transport controls supply synthetic receipts. Their evidence covers
real independent WM encoding and reducer settlement, not physical completion.
The ignored reducer-only epoch-reuse probe records why membership alone cannot
authenticate input. Session controls instead check the original connection and
monotonic receipt epoch at enqueue and reply admission, rejecting an old identity
even when relabeled with the new connection epoch.

Renderer evidence is in
[a16e9iwc](a16e9iwc-surface-instance-source-and-ownership-inventory.md); session
completion and mirrored visibility evidence is in
[vxmhx2u4](vxmhx2u4-presented-policy-input-follows-completed-frames-and-independent-revocation.md).
Joined production-routing controls, review fixes and the final family gate remain
required before source acceptance. No physical display acceptance is claimed.

## Final joined source

Signed Sophia `6251aa7915266f79e700c0997267f4361d46d70d` joins the final
renderer review `a529b2c4` and session caller `fffc8b6e`. The paired Hagia source
is signed `12d314290ce441c853cb8cd7502c367685a15f1f`. Narthex remains the
accepted `50b9014d96f675f515b5e092c071427fb8e34423`; overview requires no
Narthex change. The shell compatibility gate also retains Lom diagnostic
`97b6f63`, whose separate t005 remains open.

The final reviews closed three production gaps: replacement now waits for
existing application captures; a completed stamp-free frame revokes the old
input receipt; and admission requires a clipped draw for every target on every
current head. Tiny targets use outward raster geometry. Border admission uses
clipped bands, and retired target membership intersects every head. Revalidation
at settlement and deferred installation prevents stale topology from installing
a candidate that cannot present.

The default workspace suite passed 3,781 tests with no failures and 33 ignored.
The native backend/session suites passed 1,741 with no failures and 39 ignored.
Engine, renderer and native sampling-model suites passed 651 with no failures;
their crate trees are unchanged between the tested `58354e10` and the joined
source. Strict affected-crate all-target Clippy passed, and three shader sources
compiled. Main-tree formatting, metadata and layout checks passed.

Durable owner logs, the original negative controls and source/binary identities
are retained under
`~/.local/state/sophia/development-evidence/rendering-6251aa79/`.
Main gate logs are under
`sophia/.artifacts/integration-6251aa79-DBHFkhEI/`.
All eight native-protocol-family phases passed with unchanged clean source
identities at entry and exit. The WM phase includes 320 Nim checks, both
eleven-scenario policy corpora and all five real-Hagia presentation controls.
The shell phase includes retained Rust/C/Nim clients and the independent C/Lom
popout lifecycle controls. This accepts the implementation exits for t241 and
t242–t245, paired with Hagia h002. The queue records their closure.

No live installation or reload has occurred; physical GPU/KMS acceptance remains
separate. The earlier unintended hardware-smoke attempts remain recorded in the
renderer investigation.

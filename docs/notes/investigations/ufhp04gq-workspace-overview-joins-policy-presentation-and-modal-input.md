---
id: ufhp04gq
date: 2026-09-25
kind: investigation
status: investigating
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

Hagia owns overview layout, navigation and selection policy. On 2026-09-25 the
implementation agent reported Mason's correction of the first checkpoint's
Narthex-owned navigation; that ownership is being moved to Hagia before further
session integration. Narthex may present bounded remapped shell slots and forward
input, but does not decide overview navigation or selection. Engine samples
retained scene sources; the shell receives no surface IDs, buffers or application
pixels. Only confirmed selection changes committed workspace/focus.
Client geometry and allocations do not become thumbnail-sized.

The initial overview appears on the active monitor and preserves each output's
workspace. Super+O toggles it; arrows, hjkl, Home/End navigate; Enter or a
presented pointer target selects; Escape cancels. Hotcorners, dragging and
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

- Negotiated, bounded WM and shell records, independent Nim/Rust codec checks,
  and unchanged behavior for peers without the overview capability.
- A hidden surface referenced only by a preview is available to CPU and native
  frame construction. Its content updates repaint the thumbnail without
  changing client geometry, workspace or focus. Submitted frames retain their
  exact sources until retirement, including after overview close or withdrawal.
- Slot mappings belong to the exact WM epoch, catalog generation, shell epoch
  and output generation. Only an actually presented candidate admits selection;
  a stale reply cannot install a new mapping or resurrect a closed overview.
- Close, topology change, surface loss and either peer's reconnect revoke input
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

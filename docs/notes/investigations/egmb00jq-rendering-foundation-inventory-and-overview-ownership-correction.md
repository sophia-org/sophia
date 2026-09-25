---
id: egmb00jq
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, rendering, policy]
---
# Rendering foundation inventory and overview ownership correction

## Question

Which parts of the proposed generic presentation architecture already exist, and
which parts of the overview experiment belong in the window manager rather than
Sophia or a reference shell?

On 2026-09-25 niltempus corrected overview policy ownership and asked for a
rendering foundation document, an ASCII diagram, a todo and development notes
through zk. The request also clarified that reference-client names must not
define the architecture.
The design now names roles; concrete prototype clients are recorded here only
as evidence. Sophia t242 owns contract definition, t241 owns paired overview
acceptance, and Hagia h002 owns the feature.

## Source inventory

Read-only inspection of Sophia master `ba42f97e` found existing:

- opaque WM snapshots/proposals and staged reducer settlement;
- shell content grants, allocations, resources and action targets;
- `CompositorDisplayList`, `OutputSceneSnapshot` and per-head composition plans;
- CPU/native rendering, source retention and native frame retirement;
- presented input identities, capture and revocation for existing paths;
- presentation-only WM translation groups, but no public generic WM instance API.

Representative source owners are `sophia-engine/src/compositor_graphics.rs`,
`composition_plan.rs`, `policy_projection.rs` and `input/content_capture.rs`,
the WM/shell KDL schemas under `protocol/`, and the backend's
`production_visual_runtime` composition/presentation modules. This inventory is
source evidence, not a new execution of all master acceptance gates. The inspected
shell input binding still lacks popout role/parent identity; outside-dismiss
issuance belongs to the independently coordinated t099 work.

## Preserved prototype identities

All three clients/worktrees remain isolated on branches named `overview`.
Sophia was rebased onto `97a6b6f6` with the t159 default catalog/admission fix
retained. No main-tree edits, live install/reload or hardware acceptance occurred.

| Repository | Signed checkpoint | Evidence scope |
| --- | --- | --- |
| Hagia | `e6cfb41df2f8d8c2564a4032a93931a33941cadd` | Workspace projections preserve committed state; atomic workspace/window selection; independent Nim WM codec. |
| Narthex | `7f51175be24e8a4930d2c12ca2c35e520f879c9d` | Provisional shell navigation/settlement reducer and codec. This ownership is superseded by the WM ownership correction. |
| Sophia | `46dfc4da825b94003684dc828d732b0687dc09b3` | Internal preview primitive, CPU scaling/clipping, native lowering, and provisional overview WM/shell wire records. |

The first buildable checkpoint passed both Nim builds/layout gates, four Hagia
overview tests, four independent WM wire tests, four Narthex settlement tests,
three Rust WM overview codec tests, one direct CPU preview test, protocol
generation and the isolated native-session compile check. These focused results
do not establish a usable overview or complete modal lifecycle.

Later source work remains uncommitted and separate from this docs-only candidate.
It includes prototype projection/input/authority helpers and unfinished session
integration. A subsequent compile check reports unused integration methods;
it is not a warning-free contributor gate. No overview input plumbing was added
to session `input.rs` or `input/routing/pointer.rs`.

## Production-path observation and negative controls

The additional test is
`preview_only_updates_damage_and_hold_sources_until_copy_then_backings_until_retirement`
in `crates/sophia-backend-live/tests/support/lifecycle_tests/mirrored_intake_tests.rs`.
It uses a surface referenced only by a preview, absent from normal presentation
order and positioned off-output in the committed client geometry. It exercises
the real output display-list builder, CPU composition, retained source lookup,
native lowering and existing simulated mirrored installation/copy/retirement.

The test found an omitted lookup: retained source collection traversed only
normal `Surface` commands. Including preview references in both source identity
collection and retained lookup fixes that failure. Updating source content with
fixed selection/geometry advances preview damage and changes the sampled pixels.
Removing the overlay leaves queued and installed frames owning their source.
Completed copies release source leases; the copied native backings remain owned
through submitted/displayed states until retirement, including a lagging head.

Durable evidence is stored at:

```text
~/.local/state/hagia/development-evidence/h002-preview-20260925/
  controls.json
  missing-preview-source.log
  missing-preview-source.original
  missing-source-generation.log
  missing-source-generation.original
  production-pass.log
```

`controls.json` records the command, original source hashes and exit statuses.
The two `.original` files preserve the exact tested production files. Each
negative control temporarily removes one join, runs the test, then restores the
original bytes in a `finally` block. Removing preview-only lookup fails with
`retained head plan has no authority-owned source`. Removing source generation
propagation fails the exact generation assertion. Both tests reach a failing
test result with exit 101; they do not merely fail compilation. Restoring the
implementation passes with exit 0. No changes from those controls remain.

The production test and lookup fix are a working-tree delta after `46dfc4da`;
these results must not be attributed to that signed checkpoint alone. The test
simulates native completion and proves no physical GPU/KMS behavior.

## Ownership correction and remaining limits

Hagia must own overview arrangement, modal navigation and selection. Its current
prototype implements only preview calculation and final selection, so that
correction is not yet fully implemented. The provisional Narthex reducer and
overview-specific Sophia service are preserved experiments to be superseded,
not the architecture to freeze. Narthex need not participate in WM presentation.

Sophia should admit generic bounded spatial presentation and return authorized
actions against actual presented identities. It should not choose an overview
workspace or encode that feature's navigation policy. Client buffers remain
private to the renderer/Engine, and presentation geometry does not replace normal
client allocation or grant application input through a preview.

Generic wire records, capability/compatibility rules, modal publication and
revocation, old-generation input, delayed replies, output loss and reconnect
remain design and integration work. The existing red/green rendering controls do
not close those exits. t242 authorizes documentation and contract definition;
implementation admission and paired t241 acceptance are separate decisions.

## Connections

- [Rendering foundation](../../rendering-foundation.md): role-based architecture
  and explicit existing/proposed distinction.
- [t242 plan](../plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md):
  measurable contract-definition exit and exact proposed todo handoff.
- Hagia h002 note: `docs/notes/plans/64ac6jf6-workspace-overview-across-hagia-narthex-and-sophia.md`
  in the Hagia repository; feature ownership remains there.
- Sophia t241 note: `docs/notes/investigations/ufhp04gq-workspace-overview-joins-policy-presentation-and-modal-input.md`
  on master; the director owns its queue and acceptance record.

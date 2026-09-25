---
id: mjnpxubs
date: 2026-09-25
kind: plan
tags: [plan, rendering, architecture]
---
# Generic WM presentation foundation and input contract

## Scope and authorization

Sophia t242 defines the generic WM presentation and input contract. niltempus
requested documentation of the rendering foundation, an ASCII diagram,
development notes, and a todo. The director reserved t242 as a planning prerequisite
to t241. Documentation and contract definition are authorized; this planning row
does not authorize the proposed implementation, a wire freeze, a main-tree merge,
or a live install/reload. The director owns queue edits and integration windows.

The [architecture proposal](../../rendering-foundation.md) describes roles rather
than reference clients. A WM owns spatial presentation and navigation policy;
shells own their content; Engine owns source access, composition, presentation and
physical input. Existing renderer mechanisms are the foundation, and generic WM
surface instances and their input contract are the proposed extension.

## Contract-definition exit

### Subsequent authorization on 2026-09-25

After the documentation checkpoint, niltempus authorized implementation of the
generic rendering architecture as the next top priority, while explicitly
allowing both agents to finish their current tasks first. Coordination transfers
to the overview agent. The earlier documentation-only scope above records the
original admission; it does not limit this subsequent authorization. The new
director owns the implementation breakdown and priority update in the queue.
The original reserved planning row is integrated below without preempting t099
or the current X11 propagation work. No live installation or reload is authorized.

### Required contract

The implementation contract is [WM presentation and input](../../wm-presentation.md).
niltempus approved execution of the paired implementation plan on 2026-09-25.
The director owns the schema and WM integration, the renderer owner owns source
instances/composition, and the session owner takes generic input after completing
t099. The completed t220 propagation slice is the starting baseline `3b61790f`;
its remaining unrelated exits stay queued. Archived overview WIP is signed
`914858fa`, retained separately from the generic implementation.

The reviewed contract must specify:

1. The passive instance proposal: authorized source, distinct instance identity,
   generation, destination, clip, stacking and input disposition. State all bounds
   and malformed/unauthorized refusal rules without widening metadata or pixel
   disclosure.
2. The relationship between client layout and presentation geometry. Previewing
   must preserve client allocation, normal input geometry, workspace and focus.
   Multiple instances must retain distinct damage and hit-target identities.
3. Capability negotiation, snapshot/proposal settlement and exact action identity.
   Existing clients retain current behavior without the new capability; an
   explicitly requested unsupported feature fails clearly. Shell permissions
   remain separate from WM surface access.
4. Source and frame ownership through rendering/copy completion, submission,
   mirrored presentation and retirement. Specify damage from source changes with
   unchanged instance placement, as well as hidden-source and removal behavior.
5. Presentation-bound input publication and immediate revocation on close, source
   loss, topology change or reconnect. Specify stale/late reply rejection, release
   debt after capture revocation, bounded queues and failure outcomes.
6. An acceptance matrix covering independent codecs, existing-client compatibility,
   pure WM policy, production rendering/retention and input lifecycle. Separate
   deterministic evidence from physical acceptance and identify remaining design
   decisions before proposing implementation admission.

The document must explain how generic mechanisms cover the use case without
embedding a particular reference client's overview or shell vocabulary in Engine.
No new toolkit, shader-upload interface, GPU negotiation policy, or unrelated
desktop feature is implied. The subsequent authorization above admits the
implementation; writing the contract alone does not close t242 or t241.

## Proposed queue handoff

The director allocated `(B)`, `+parallel @planning`, and `order:240.9` on
2026-09-25. The exact proposed line, supplied for the director's queue edit, is:

```text
(B) Define generic WM presentation and input contract +parallel @planning id:t242 order:240.9 [plan](docs/notes/plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md)
```

The director will add `depends:t242` to t241. This block is the requested handoff
text, not a second queue; active state and ordering belong exclusively to
`todo.md`. t241 owns paired Sophia overview acceptance; Hagia h002 retains feature
ownership. t099 owns the separate shell-popout allocation/dismissal seam and must
be coordinated before shared input changes.

## Implementation tracks

These scopes describe the admitted breakdown; status and ordering remain in the
queue. Existing work on t099 finishes before its owner moves to the foundation.

### t243: Protocol and admission

The protocol owner implements capability-gated fixed records, independent codec
corpora, complete-set validation and atomic projection settlement. The exit is
an independently decoded proposal accepted through the production policy reducer,
with stale, malformed, unauthorized and legacy-client controls. The WM side stays
in Hagia h002; protocol code never interprets overview action names.

### t244: Composition and source ownership

The renderer owner threads the passive instance/region records through immutable
snapshots, CPU/native planning and rendering, per-instance damage, source lookup
and retirement. The exit includes two instances of one source, a preview-only
source, content-only updates, independent source/backing lifetime and the retained
negative controls. Instance rendering never manufactures application input layers.

### t245: Presented input and session lifecycle

After t099 releases shared paths, the session owner joins admitted publications
to actual presentation receipts, modal action delivery, retirement-bound input,
local revocation, timeout and reconnect. The exit includes queue saturation,
unsent-action refusal, stale/late replies, source/topology loss, capture continuity
across content-only repaint and retained swallowed-release obligations. It depends
on the protocol and composition records; t241 accepts the complete Hagia feature.

## Accepted implementation evidence

Signed Sophia source `6251aa79` and Hagia source `12d3142` satisfy the admitted
headless exits. The [paired acceptance record](../investigations/ufhp04gq-workspace-overview-joins-policy-presentation-and-modal-input.md#final-joined-source)
records exact identities, durable logs and limits. Independent C/Nim/Rust wire
controls and legacy-client compatibility pass. Production rendering controls
cover preview-only and repeated sources, content-only damage, source/backing
retirement, CPU/native sampling models, mirrors and clipped target membership.
Session controls cover actual retired projections, capture deferral, exact
receipt/connection epochs, queue saturation, local revocation and release debt.

The default workspace suite, affected native suites, strict Clippy, layout and
the complete eight-phase family gate pass. These results close the implementation
exits for t242–t245 and paired t241; active state remains solely in the queue.
Narthex requires no overview service or source change. No live install/reload was
performed. Physical display acceptance remains separate, and the renderer
investigation retains the earlier unintended smoke attempts and their limits.

## Connections

- [Rendering foundation](../../rendering-foundation.md): ASCII architecture,
  implementation inventory and proposed mechanisms.
- [Development investigation](../investigations/egmb00jq-rendering-foundation-inventory-and-overview-ownership-correction.md):
  inspected sources, prototype evidence, negative controls and scope correction.
- [Compositor graphics](../../compositor-graphics.md): current primitive and
  authority rules that this proposal must preserve.
- [Work tracking](../../work-tracking.md): queue and acceptance ownership.

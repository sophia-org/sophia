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
desktop feature is implied. Acceptance of the contract and implementation
admission remain explicit follow-up decisions; writing this plan does not close
t242 or t241.

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

## Connections

- [Rendering foundation](../../rendering-foundation.md): ASCII architecture,
  implementation inventory and proposed mechanisms.
- [Development investigation](../investigations/egmb00jq-rendering-foundation-inventory-and-overview-ownership-correction.md):
  inspected sources, prototype evidence, negative controls and scope correction.
- [Compositor graphics](../../compositor-graphics.md): current primitive and
  authority rules that this proposal must preserve.
- [Work tracking](../../work-tracking.md): queue and acceptance ownership.

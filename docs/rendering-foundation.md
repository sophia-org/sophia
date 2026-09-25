# Rendering foundation for window managers and shells

**Role:** architecture proposal and implementation inventory, recorded
2026-09-25. The generic WM presentation protocol remains proposed. This document
does not claim that the complete diagram is implemented or supersede the current
wire contracts.

The [WM presentation contract](wm-presentation.md) defines the admitted first
implementation, including passive identities, frame ownership and presented input.

The window manager owns layout, overview arrangement, navigation, and
workspace/window selection. Shells own their interface and content. Sophia
supplies validated composition, source ownership, presentation, and physical
input routing.
A shell is not a required intermediary for WM presentation. Reference clients
exercise these roles; they do not define the architecture or its dependencies.

## Architecture

```text
   WINDOW MANAGER               SHELLS                APPLICATIONS
  Layouts / overview       Panels / launchers        Window content
  Navigation / selection   Own content and actions   Normal geometry
          |                        |                        |
          v                        v                        v
+-------------------------------------------------------------------+
|                            SOPHIA                                 |
|                                                                   |
|   WM API                    Shell API           Application        |
|   Opaque surface IDs        Owned content       authority          |
|   Placement proposals       Action targets          |             |
|          |                        |                  v             |
|          |                        |          Committed buffers      |
|          |                        |                  |             |
|          +------------------------+------------------+             |
|                                   |                               |
|                                   v                               |
|                         SURFACE INSTANCES                         |
|                   Source + destination + clip                     |
|                   Stacking + input disposition                    |
|                                   |                               |
|                                   v                               |
|                          IMMUTABLE FRAME                          |
|                   Retained sources + hit targets                  |
|                                   |                               |
|                                   v                               |
|                         CPU / GPU rendering                       |
|                                   |                               |
|                                   v                               |
|                       PRESENTED PIXELS + TARGETS                  |
|                                   |                               |
|                                   v                               |
|   Physical input ----------> INPUT ROUTING                        |
|                         Validate displayed identity               |
+-----------------------------------+-------------------------------+
                                    |
                 +------------------+------------------+
                 |                  |                  |
                 v                  v                  v
          Window manager          Shell           Application
           Policy actions     Shell actions       Client events
```

The diagram describes authority and data flow, not new processes. WM and shell
APIs remain separate capability boundaries even where they share an internal
compositor representation. Sophia also publishes reduced spatial facts to the
WM; that return path carries no application pixels or metadata.

## Client geometry and presentation geometry

A client can retain a 1200 by 800 allocation while Sophia displays its committed
content in a 300 by 200 thumbnail. That presentation must not resize the client,
replace its normal layout, or grant application input through the thumbnail.
The WM proposes spatial presentation and selection using opaque surface handles.
Sophia resolves the corresponding retained content and validates the proposal.

The proposed reusable primitive is a surface instance with:

- an instance identity and generation, separate from the source surface;
- an authorized source and its current generational identity;
- a destination rectangle, clip, stacking position, and bounded opacity;
- an explicit input disposition and, when applicable, an authorized action
  target tied to that presentation.

Visual-only instances, policy action targets, and ordinary application
interaction are distinct permissions. A preview does not acquire application
input merely because it samples the application's image. Multiple instances
must not duplicate source ownership or confuse their hit targets.

This is a bounded spatial proposal through WM IPC, not permission to upload
shaders, read screenshots, acquire renderer handles, or submit arbitrary drawing
programs. Shell content continues through its own grants and allocation rules;
a shell does not gain arbitrary application-surface access. Engine retains
control of protected layers, clipping, admission, and resource limits.

## What exists

This inventory is based on source inspection of Sophia checkpoint `ba42f97e`.
Implementation presence is separate from acceptance on particular hardware.

| Mechanism | Current implementation | Remaining gap for this proposal |
| --- | --- | --- |
| Application content and ownership | Application authorities submit transactions; Engine holds committed surface content. | Validate source eligibility and lifetime for additional presentation instances. |
| WM boundary | Opaque snapshots, bounded projection proposals, staged validation and commit are implemented. Presentation-only translation groups already exist. | A generic WM instance proposal and its capability/settlement contract. |
| Shell boundary | Content grants, allocations, resources, candidate placement and action targets are implemented. | Reuse common internal composition without widening shell source authority. Popout outside-dismiss is separately tracked as t099. |
| Compositor representation | `CompositorDisplayList` carries surfaces, content images, rectangles, borders, text and indicators. | Independently placed application-surface instances are not exposed through the master WM API. |
| Frame planning and rendering | Immutable output snapshots, per-head plans, CPU/native lowering and output scaling are implemented. | Join instance geometry, source damage and bounded repetition into those paths. |
| Retention and retirement | Source ownership, native submission, mirrored completion and scanout retirement have existing owners. | Prove that new instance references use every required ownership path. |
| Presented input | Existing application and shell paths use presented geometry, generations, capture and revocation. | A generic presentation-scoped WM action/modal contract, including late-reply and reconnect controls. |

The current compositor is therefore the foundation. A new renderer or a
parallel frame scheduler is not required to implement the proposal.

## Evidence boundary

An isolated preview prototype exercises source scaling, clipping, damage and
retention through production rendering paths with simulated native completion.
That evidence does not establish a public generic WM instance API, a complete
modal input lifecycle, or physical GPU/KMS acceptance. Reference-client names,
candidate identities, negative controls and the ownership correction belong in the
[development investigation](notes/investigations/egmb00jq-rendering-foundation-inventory-and-overview-ownership-correction.md).

## Presentation and input lifetime

A frame acquires ownership of the sources it needs. Source pixels remain owned
through queued and executing rendering/copy work. Once copies complete, those
source leases may end; the resulting native scanout backings remain owned through
submission, display and retirement. Closing an overlay cannot free either class
while its consumer still needs it. Mirrored heads must settle their own ownership.

Submitting or preparing a candidate grants no new input authority. Its targets
become usable only when the matching presentation retires. Revocation can remove
input authority immediately while old pixels remain visible. Releases belonging
to swallowed presses must remain swallowed after revocation.

The generic contract must bind input to the issuing connection, publication and
instance generations, output topology, and actual presented identity. Close,
source removal, topology change or reconnect must invalidate stale work. Neither
a delayed candidate nor an old action may recreate the revoked authority. The
exact record layout and cancellation exchange remain design work.

## Wire direction and compatibility

The WM owns overview arrangement, modal navigation and selection. Sophia
should receive bounded presentation proposals and deliver reduced authorized
policy actions, without interpreting an overview workspace catalog or choosing
its selected window. Shells remain responsible for their own shell features.

The prototype's overview-specific WM messages and shell revision-9 exchange are
unreleased experiments. Preserve their checkpoints as evidence, then replace
them with the reviewed generic WM contract. They do not establish a stable wire
obligation. Negotiate the new capability explicitly: existing clients without it
retain their existing behavior, and an explicitly requested unsupported feature
must fail clearly. Any shell protocol change needs its own demonstrated shell
requirement; the WM overview alone does not justify one.

Final design must settle bounds, source eligibility, instance/action identities,
presentation settlement, damage propagation, revocation and restart behavior
before treating the protocol as ready for integration. The
[foundation plan](notes/plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md)
holds the measurable exit; task state belongs in `todo.md`.

## Related contracts

- [Architecture](architecture.md): process and authority ownership.
- [Compositor graphics](compositor-graphics.md): display-list primitives and
  the compositing operator rule.
- [Multi-monitor composition](multi-monitor-composition.md): per-head planning
  and joined presentation.
- [Renderer import boundary](renderer-import-boundary.md): renderer-private
  source resources.
- [Building on Sophia](building-on-sophia.md): WM and shell capabilities.

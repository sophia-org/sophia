# Rendering foundation for window managers and shells

**Role:** rendering architecture and implementation inventory, updated
2026-09-25. The generic WM extension and paired Hagia overview pass joined
headless acceptance. This document does not supersede the wire contracts or
claim physical display acceptance.

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

The reusable primitive is a surface instance with:

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

The original inventory inspected Sophia checkpoint `ba42f97e`. The table below
records the accepted source `6251aa79`, including protocol,
composition, session input and paired Hagia controls. Hagia's paired source is
`12d3142`; Narthex requires no overview changes.
Implementation presence is separate from integration and hardware acceptance.

| Mechanism | Implementation | Acceptance boundary |
| --- | --- | --- |
| Application content and ownership | Engine resolves each authorized instance source to committed content; missing sources refuse the whole candidate, and source removal revokes the whole publication. | Preview-only sources and content-only updates have production-path controls. |
| WM boundary | Capability-gated complete presentation records accompany ordinary projections; fixed wire layouts, bounds and generation rules are validated before commit. | Independent C/Nim/Rust codecs, real Hagia transport controls and all eight family gate phases pass. |
| Shell boundary | Existing content grants, allocations and action identities remain separate. Popout dismissal landed in accepted `aff26bac`. | No new shell authority or protocol is needed for WM presentation. |
| Compositor representation | Independently placed `SurfaceInstance` and Engine-owned regions share a bounded presentation tier. Instances never become application input layers. | Repeated sources retain separate node geometry and damage. |
| Frame planning and rendering | Immutable snapshots, head transforms, clipped instance drawing, opacity and CPU/native sampling models include the tier. | Fractional and cross-output controls pass; model equivalence does not establish physical GPU output. |
| Retention and retirement | Source collection includes ordinary and instance-only references, deduplicates leases and preserves separate copied backing ownership. Every mirror contributes completion evidence. | Source removal, copy completion, close and lagging-head controls pass under simulated device completion. |
| Presented input | Session joins completed stamps to receipts, modal admission, exact actions, local revocation and release debt. | Production routing, reconnect, queue saturation, protected precedence and retained application captures have deterministic controls. |

Admission checks every current physical head before both preparation and final
settlement. A target with no clipped pixels on any head refuses the whole
candidate and preserves the prior publication. Tiny targets round outward;
border checks use their clipped bands. Retired target membership is intersected
across all heads as a separate completion check. Replacement waits for existing
application captures to settle, then revalidates before installation.

The existing compositor supplies this foundation. The implementation adds records and
joins to its owners, without adding a renderer or a parallel frame scheduler.

## Evidence boundary

The preserved preview prototype established the first source lookup and damage
controls. Its [investigation](notes/investigations/egmb00jq-rendering-foundation-inventory-and-overview-ownership-correction.md)
records those checkpoints and the ownership correction. The generic candidate's
[renderer investigation](notes/investigations/a16e9iwc-surface-instance-source-and-ownership-inventory.md)
records production controls, negative controls, sampling limits and the two
unintended real-card smoke attempts. Both attempts were refused; their complete
output was not retained. Subsequent suites run with device nodes hidden.

Real Hagia transport tests exercise generic publication, repaint, exact actions,
timeout, revoke receipts and reconnect. Their receipt fixtures are synthetic;
they do not prove that a frame actually completed. That proof belongs to the
backend/session completion controls. No headless result establishes physical
GPU/KMS acceptance.

## Presentation and input lifetime

A frame acquires ownership of the sources it needs. Source pixels remain owned
through queued and executing rendering/copy work. Once copies complete, those
source leases may end; the resulting native scanout backings remain owned through
submission, display and retirement. Closing an overlay cannot free either class
while its consumer still needs it. Mirrored heads must settle their own ownership.

Submitting or preparing a candidate grants no new input authority. Its targets
become usable only when every required head retires the matching presentation.
A modal publication waits for all covered outputs. Revocation removes input
authority immediately while old pixels may remain visible. Any head still
displaying the tier keeps its output shielded; absence of all-head consensus is
not evidence of withdrawal. Releases belonging to swallowed presses remain
swallowed after revocation.

The generic contract must bind input to the issuing connection, publication and
instance generations, output topology, and actual presented identity. Close,
source removal, topology change or reconnect must invalidate stale work. Neither
a delayed candidate nor an old action may recreate the revoked authority. The
record layouts and lifecycle outcomes are specified in the
[WM presentation contract](wm-presentation.md). Software-only diagnostic sessions
without a native retirement owner omit both presentation capabilities. CPU
composition through the native retirement owner remains supported.

## Wire direction and compatibility

The WM owns overview arrangement, modal navigation and selection. Sophia
receives bounded presentation proposals and delivers reduced authorized
policy actions, without interpreting an overview workspace catalog or choosing
its selected window. Shells remain responsible for their own shell features.

The prototype's overview-specific WM messages and shell revision-9 exchange are
unreleased experiments. Their checkpoints remain as evidence; the generic WM
contract supersedes them. They do not establish a stable wire
obligation. Negotiate the new capability explicitly: existing clients without it
retain their existing behavior, and an explicitly requested unsupported feature
must fail clearly. Any shell protocol change needs its own demonstrated shell
requirement; the WM overview alone does not justify one.

The contract defines bounds, source eligibility, instance/action identities,
presentation settlement, damage propagation, revocation and restart behavior.
Joined implementation gates establish those guarantees within the documented
headless evidence boundary. The
[foundation plan](notes/plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md)
holds the measurable exit; task state belongs in `todo.md`.

## Reference implementations

Hagia implements the WM side: overview layout, navigation, selection and generic
presentation proposals. Its design ports spatial policy from Triad's Super+O
overview, originally modeled after niri. Triad and niri are reference designs,
not build or runtime dependencies of Hagia or this protocol.

The early proof of concept paired Hagia with Narthex and an overview-specific
Sophia exchange. Those signed experiments remain evidence, but the generic design
moves navigation and selection fully into Hagia. Narthex remains a separate shell
and is not an intermediary for overview. The role-based ASCII diagram applies to
other WMs and shells without adopting these reference clients' vocabulary.

## Related contracts

- [Architecture](architecture.md): process and authority ownership.
- [Compositor graphics](compositor-graphics.md): display-list primitives and
  the compositing operator rule.
- [Multi-monitor composition](multi-monitor-composition.md): per-head planning
  and joined presentation.
- [Renderer import boundary](renderer-import-boundary.md): renderer-private
  source resources.
- [Building on Sophia](building-on-sophia.md): WM and shell capabilities.

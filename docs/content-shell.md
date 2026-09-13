# Content Shells

**Role:** behavioral contract for a content capability within
`sophia_shell_v1`.
**Status:** the CPU-byte wire and lifecycle now have an
[implementation](lom-content-implementation.md); production launch admission,
discrete content input and native acceptance remain incomplete. The
[content ADR](notes/decisions/6ndjwffd-content-capability-design-for-sophia_shell_v1.md)
owns the wire and numeric budgets. The
[execution ADR](notes/decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
owns the accepted GPU permission model; it does not enable the runtime gate.

The [architecture](architecture.md), [native protocol family](sophia-policy-ipc.md),
[compositor graphics](compositor-graphics.md), and
[target-resolved input](target-resolved-input.md) contracts remain authoritative.
The [reference-client audit](shell-reference-client-audit.md) supplies the first
workflow and its feasibility evidence. This contract does not stabilize the
experimental shell interface or change the frozen WM interface.

## Two Shell Models

A descriptor shell chooses among the vocabulary Engine already renders. It
receives authorized facts and proposes ordering, selection, visibility, and
the appearance settings admitted by each feature. Engine supplies the pixels
and interaction targets. Narthex implements this model, including revision 4's
[application launcher](application-launcher.md). Its limited drawing authority
is a useful property and remains supported.

A content shell designs and rasterizes its own interface. It submits bounded
content and presentation candidates for Engine to validate and composite.
Widgets, typography, artwork, and layout inside an admitted shell surface belong
to the shell. Engine need not learn what a widget is to display it.

These models differ in trust, not merely in feature count. A content shell can
read the content it creates, but content permission grants no access to other
applications' pixels or the composed desktop. It can nevertheless draw a
misleading label or imitate another interface inside its allocation. Presented
input validation proves which target the user activated; it cannot prove that
the artwork honestly describes that target. Descriptor restrictions reduce
these opportunities without establishing a general guarantee against deception.

Sophia currently admits one native shell client. The proposed client may combine
descriptor features with separately granted content features: for example, a
custom panel beside the existing descriptor launcher. This proposal introduces
neither multiple native shell clients nor an implicit native role for an X11
panel. Narthex remains the independent descriptor reference.

## Admission And Authority

Selecting a shell executable and granting it content capability are separate
decisions. The operator's session policy must explicitly permit content before
negotiation can select it. The default is no content permission. The session
establishes the grant, limits, and protection domain at startup; a client request
cannot enlarge them. [Configuration](configuration.md) records the implemented
policy syntax and the separately identified target syntax. Effective-profile
and launch evidence must distinguish requested policy from actual admission.

GPU execution is a separate, default-denied launch permission. The accepted
first Lom path explicitly exposes one render node within its protection domain,
with no application display or input endpoint. It requires no custom kernel or
particular device-memory controller and promises no hard aggregate VRAM quota.
Content clients may instead CPU-rasterize without a GPU grant. Neither execution
choice changes content ownership, disclosure, pacing or exact presented input.
An optional GPU bridge is not a required SDK or part of this wire contract.

Negotiation selects only the intersection of implementation support, client
requests, and operator permission. A client requiring unavailable content must
receive an explicit refusal. A client offering a descriptor alternative may use
the capabilities actually selected; there is no automatic switch to a host
application or broader authority. Replacement requires fresh admission and a new
connection epoch. Session security can revoke an existing grant immediately.

The shell runs in a protection domain separate from blind spatial policy. It
receives only facts permitted for its negotiated role. A common repository,
toolkit, or executable does not permit shared writable state or ambient IPC
that recombines those authorities.

| Responsibility | Owner |
| --- | --- |
| Widgets, content generation, internal layout, and supported visual policy | shell |
| Content admission, budgets, supervision, and execution permissions | session |
| Authoritative shell placement, clipping, stacking, composition, and presentation | Engine |
| Physical target selection, input disclosure, capture, and revocation | Engine |
| Application layout in the resulting work area | blind WM |
| Application metadata disclosure and issuer-scoped actions | the existing broker or session authority |

Content permission grants no process execution, application placement or focus,
screen capture, clipboard, lock authority, synthetic input, or general desktop
service access. An example status widget does not thereby gain permission to
read host services. Each such operation needs its own existing or later
specified authority. X11 remains the active application frontend and the
development priority; content support does not require another frontend.

## First Workflow: Panel And Anchored Popout

The first implementation must let a client present one panel on a selected
output, claim a bounded work-area reservation, and expose a button. Activating
that button opens a popout anchored to the panel. A control in the popout changes
its own visual state. The user can close the popout through an admitted target
or Engine-mediated outside dismissal. Opening the popout does not launch an
application or disclose application metadata.

Engine provides the minimum shell-local allocation and scale facts needed to
produce content of the right size, bound to opaque output and allocation
generations. These facts do not expose global pointer coordinates, application
geometry, namespace identities, or frontend resources. A toolkit adapter may
construct its own local screen and window objects from authorized facts; those
objects are not protocol types.

The shell proposes panel edge, extent, reservation, and popout placement within
the session's limits. Engine resolves their authoritative geometry, stacking,
and clipping. A popout belongs to its parent allocation and cannot grant itself
a work-area reservation or security-surface priority. It must fit within its
authorized output allocation; if a complete placement cannot be produced, the
candidate is rejected. Stale output, allocation, scale, or parent generations
cannot authorize a new presentation.

Panel content and reservation changes participate in one coherent logical
presentation with the work area and the WM's corresponding application layout.
Preparation may run concurrently, but promotion requires matching reservation,
output, work-area, WM, and shell epochs. A failure preserves the preceding
coherent presentation. The shell never moves application windows directly.

## Content And Presentation Lifecycle

The public interface must distinguish content resources from candidates that
reference them. Resource identities are scoped to the admitted connection and
their generations. A content hash may support reuse; it does not authorize a
foreign resource lookup or substitute for recipient ownership.

Content is immutable once accepted. The implementation must validate dimensions,
format, storage extent, and resource limits before treating a transfer as
complete. A partial transfer cannot become visible or contribute an input
target. Changed content receives a new generation; later wire design must fix
pixel-format, alpha, color, and scale semantics rather than relying on toolkit
defaults. An initial complete-candidate contract is required. Any later delta
must name and validate its base generation.

A complete candidate binds its resources, output and allocation generations,
placement intent, reservation, interaction snapshot, and any admitted effects.
It cannot mix new pixels with old target meaning or publish a half-updated
panel/popout relationship. Every referenced resource must be complete and
authorized before preparation succeeds.

| Stage or outcome | Required meaning |
| --- | --- |
| Resource accepted | Validated content is available for candidates; it is not necessarily visible |
| Prepared | The complete candidate is validated and submitted for rendering; its targets are not yet active |
| Presented | That exact candidate has retired on the applicable output and its interaction snapshot can become eligible |
| Rejected or superseded | The candidate acquires no new presentation or input authority; the outcome names the affected candidate |
| Resource released | No pending or retained rendering work can still consume the resource |

Superseding a candidate or disconnecting its producer does not make storage safe
to reuse while rendering still references it. The eventual resource protocol
must make release obligations and terminal outcomes explicit, including partial
transfer failure and peer loss. All transfer, resident-resource, target, and
candidate queues need finite, advertised limits and bounded waits. Exhaustion
must reject or supersede work through the specified lifecycle, not grow memory
without bound or discard accepted obligations silently.

## Input And Dismissal

The [target-resolved input contract](target-resolved-input.md) governs this
capability. Engine resolves physical input against the applicable last-presented
interaction snapshot after occlusion and security precedence. A shell proposes
targets only inside its admitted visual allocations. It cannot create an
invisible global click shield or intercept higher-trust content.

The first buttons use coordinate-free discrete actions bound to exact target
identities and generations. The adapter maps those actions to its local widgets.
Capture, device/contact matching, release, cancellation, and replay rejection
retain the existing input contract. Preparation alone grants no capture, and a
changed target meaning must not inherit an earlier press.

For outside dismissal, Engine observes the event and requests dismissal without
disclosing the outside location or target to the shell. The event that dismisses
the popout is consumed, not replayed into the application underneath. Popout
withdrawal and capture cleanup settle through the same presentation lifecycle.
Parent loss or epoch revocation invalidates its popout interactions immediately.

Content capability alone grants neither general keyboard input nor a pointer
event stream. Text entry, continuous values, and exceptional region-local
coordinates require separately specified and admitted extensions. Such an
extension must retain target, region, precision, rate, presentation, and
revocation bounds; it cannot become an ambient input feed for a toolkit.
The first workflow needs none of these extensions. Existing descriptor features
keep their own input contracts.

## Visual Style, Effects, And Pacing

A content shell can build cards, custom glyphs, graphs, and other artwork using
its own renderer. Engine composites the resulting content without learning the
widget model. Qt/QML, GTK, and language-specific types remain downstream details;
no toolkit becomes a Sophia dependency or required public SDK.

The [compositing operator rule](compositor-graphics.md)
separates ordinary content from compositor effects. A shell may request a
negotiated Engine effect, such as a supported backdrop blur, with bounded
parameters, generations, and a legible fallback. Engine performs scene sampling
without returning the sampled pixels to the shell. A novel scene-sampling or
renderer-specialized effect requires separately trusted renderer integration
under the graphics contract. Public shell messages cannot upload shader source,
SPIR-V, arbitrary uniforms, or renderer programs.

The first panel/popout must work without optional effects. Mandatory controls,
text, and trust indications cannot disappear when an effect is unavailable.
Effect and transition vocabulary is separately admitted; this proposal assigns
none. Engine owns transition timing, damage, and presentation. A client may
produce changing content under Engine pacing and backpressure, but cannot
schedule compositor frames or introduce a second presentation clock.

Static content should upload once and remain reusable across candidates.
Measure a static panel, a small changed region, and continuously changing
content. Record uploaded and reused bytes, resident memory, queue depth, idle
wakeups, CPU raster or GPU readback costs, Engine upload/composition costs, frame
pacing, and power where measurable. Results must identify dimensions, scale,
update rate, hardware, and driver. No refresh-rate or zero-copy claim follows
from GPU composition alone.

Cached content is the first transport experiment, not a settled transport ABI.
The existing family uses bounded byte frames and complete chunked transfers.
Shared memory, FD passing, and DMA-BUF remain candidates requiring measured
need and a separate specification for ownership, synchronization, format,
damage, lifetime, fallback, and renderer failure. The shell cannot borrow X11's
buffer contract or open a private Engine channel to avoid that work.

## Recovery And Security Transitions

Peer loss, timeout, output loss, shell replacement, or capability revocation
invalidates affected input rights and queued activations immediately. Retained
pixels remain inert. Engine and the session reconcile content withdrawal,
reservation removal, and WM projection together; a crashed shell must not leave
permanent resource or reservation ownership behind. Renderer failure preserves
the last coherent presentation when possible and reports a bounded failure.
Retaining pixels never overrides an input revocation.

Reconnect starts a new epoch with fresh resources and complete state. Reloaded
toolkit objects cannot revive old content handles, captures, or activations.
Old storage remains subject to its release obligations even while the new
client starts. Lock and other security takeover paths revoke interaction
without waiting for either the shell or WM to acknowledge; they retain their
separate preemptive authority.

## A Later Custom Launcher

The first workflow deliberately uses only local visual state. A content shell
may also implement today's descriptor launcher through its existing capability,
but arbitrary launcher artwork needs a separate extension.

That extension must retain session-owned catalogs, execution policy, exact
presented activation, one-use acknowledgement, and revocation. Content drawing
alone authorizes no launch. It must also specify how application identity is
presented and what the operator trusts the shell to represent. Engine cannot
infer an honest application label from arbitrary pixels, so the extension cannot
inherit the descriptor launcher's immutable-label assurance. This document does
not choose a trusted identity overlay, extra confirmation, or another launcher
policy by implication. The current `trusted-host` execution policy remains
distinct from shell confinement and does not sandbox launched applications.

## Acceptance And Remaining Design Work

The driving content client is now Lom, replacing the earlier Quickshell/ironbar
adapter proposals. It and an independent C client must implement the same
published contract, without Lom or Sophia libraries in the C client. Both must
present the panel, reserve space, open the popout,
change local state, and dismiss it. Existing descriptor revisions and corpora
must continue to pass.

The implementation gate must cover:

- Unsupported grants, malformed or oversized content, incomplete transfers,
  foreign resource references, stale generations, and bounded queue/resource
  exhaustion, with no partial presentation or unaccounted accepted work.
- Unpresented activation, target changes during capture, duplicate or replayed
  actions, output changes, occlusion, and dismissal without click-through or
  outside-coordinate disclosure.
- Resize and scale changes, atomic reservation/work-area/WM updates, parent
  withdrawal, peer crash, reload/reconnect, renderer failure, and safe resource
  retirement. Test security takeover without relying on peer cooperation.
- An Engine test host without an X application frontend, plus an adapter probe
  without `DISPLAY`, `WAYLAND_DISPLAY`, or an application-facing display socket.
  Merely unsetting environment variables is not proof of connection independence.
- Retained performance measurements and a separately scheduled physical
  panel/popout check in the normal X11 Sophia session. Headless and X11-panel
  results cannot stand in for native physical input and presentation evidence.

The content ADR selects transport, numeric budgets, pixel semantics, capability
assignments, wire records, release/pacing messages and the required corpus.
Lifecycle models and implemented evidence are recorded separately; new authority
transitions must still be modeled and checked under the evidence policy.
The [paired plan](notes/plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md)
and [todo](../todo.md) retain production and acceptance gates. A design amendment
does not close those tasks or establish native behavior.

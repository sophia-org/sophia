# WM presentation and input contract

**Role:** implementation contract for the capability-gated generic WM extension.
The [paired acceptance record](notes/investigations/ufhp04gq-workspace-overview-joins-policy-presentation-and-modal-input.md)
records the accepted source and headless gates for t242–t245 and t241.
Physical display acceptance remains separate.
niltempus approved this plan on 2026-09-25. Overview remains WM policy.

## Authorities and passive values

The WM proposes a complete `PolicyPresentation` beside its ordinary layout
projection. The authenticated connection epoch owns the publication. The WM
receives no buffers, pixels, renderer handles, application metadata or raw input.
Shell grants, allocations, protected layers and input precedence stay separate.

`packets/presentation.rs` in sophia-protocol owns the passive records:

| Record | Meaning |
| --- | --- |
| Presentation | Nonzero generation, optional keyboard output, complete output/instance/region/binding sets. |
| Output | Current opaque output and generation, coverage, Overlay or ReplaceApplications mode. |
| Surface instance | Opaque id/generation, output, authorized source surface, destination/clip, opacity, z order, optional action. |
| Region | Opaque id/generation, output, geometry/clip, z order, Engine chrome role, optional action. |
| Binding | Existing opaque policy action, keycode and modifiers, active only in this presented modal scope. |

Ids are nonzero and unique across instances and regions in a publication.
Engine qualifies them by the admitted WM connection epoch; it does not allocate
a second identity map. An instance's source is an already-authorized generational
surface from the current WM scene. The Engine resolves its committed content
generation at frame capture. A content-only update changes neither instance nor
interaction generation.

Publications have at most 16 outputs, 1,024 instances, 1,024 regions and 256
bindings. They additionally obey existing display-list and per-head layer bounds.
Each instance consumes a layer even when its source is shared. Exceeding a bound
rejects the complete candidate; it never silently truncates a publication.

## Geometry, order and chrome

Geometry is in the existing root logical coordinate space. Destination and clip
must be nonempty and intersect; the clip must lie within that output's coverage.
Coverage must lie within the current output bounds. Opacity is 1..1000. Instance
source geometry, client allocation, ordinary layout and application input
geometry are unchanged. Sampling scales committed source content into the
destination and clips the result using the existing head transform.

Policy targets and stamp coverage use conservative raster bounds: outer edges
round outward and a region stroke's inner edges round inward. Drawing and mirror
damage share this arithmetic. Admission also checks the actual targets of every
covered output head. A missing head or a target with no drawable clipped pixels
refuses the whole candidate before commit, preserving the previous publication.
This includes a target fully cropped by Cover or Exact mapping; retaining its ID
cannot stand in for a draw. Deferred installation rechecks the current heads.

Z order is explicit and unique within each output across regions and instances.
Engine sorts these records into one command list. The WM presentation tier is
above ordinary application content and below protected shell/trust content.
Normal occlusion and authority precedence still apply.

The tier follows ordinary tab bars and floating outlines and precedes shell
content and descriptor overlays. Replacement suppresses application chrome,
tab bars and floating outlines along with ordinary application surfaces.

Overlay preserves ordinary application presentation. ReplaceApplications covers
the entire output and substitutes the presentation tier for ordinary application
draws and hit targets there; it does not unmap clients or discard their content.
Withdrawal restores the ordinary presentation. It does not restore old layout
state over newer, independently committed application state.

Backdrop, Frame and Emphasis are Engine-owned chrome treatments using the
existing compositor palette and stroke policy. They contain no text, image,
shader or client-selected colors. A Frame supplies visible allocation for an
empty selectable region; interaction cannot extend outside its declared clipped
allocation. Each replacement output has a Backdrop covering its coverage.

Backdrop uses the frame color at full opacity; Frame uses the frame stroke and
Emphasis uses the focus-ring stroke. CPU and native rendering apply identical
source-alpha and instance-opacity composition.

## Atomic publication and identity

Every proposal using the new capability carries the complete presentation or
explicit absence, which withdraws it. It follows the existing request, staged
validation and terminal projection outcome. Ordinary output coverage and focus
remain governed by their existing records. A malformed, stale or unauthorized
presentation rejects the whole proposal before any part becomes authoritative.
Changing or withdrawing presentation requires the request to cover every output
in both the old and new publication; an identical resend does not widen scope.

Publication generations increase on structural or interaction change. Retaining
the generation requires identical passive records. Changing source identity,
destination, clip, opacity, z order, action or region role also requires a fresh
target generation. Newly introduced ids exceed the connection's previously
admitted maximum; removed ids cannot be recycled. This keeps retirement history
bounded to a high-water mark rather than an unbounded tombstone table. Changing
only source content does not alter either generation. The WM may resend identical
presentation records during unrelated ordinary projection cycles.

Source membership and output generations are checked at admission and again
before presentation. A missing renderable committed source cannot be replaced
with a dangling reference: retain the last valid presentation while normal
bounded transaction readiness/rejection resolves the candidate.

Frame capture is immutable. Sources are collected from both ordinary Surface and
SurfaceInstance commands, including sources absent from ordinary presentation.
The source table is deduplicated; instance bindings and damage are not. Sources
remain leased through queued/executing render or copy operations. Copied native
backings remain owned separately through submission, display and retirement.
Each mirrored head settles its own ownership. Failed rendering, supersession,
output loss and close use the existing frame-retirement owner.

## Presented input

Visual-only records have no action. An action must name a registered pure WM
policy action, never a session-operation token. Pointer hit testing uses the
actual presented geometry, clipping, stacking and protected-layer occlusion.
Instances never enter ordinary application input-layer snapshots.

Pointer activation requires a press and matching release from the same device
against the same eligible identity. The reduced action carries the WM connection
epoch, publication generation, output identity/generation, presentation epoch,
target id/generation and activation serial. It carries no coordinates or source
pixels. Keyboard actions use target id/generation zero and the published keyboard
output. Sophia interprets neither action names nor workspace/window selection.

A keyboard output requests a modal scope over the publication's covered outputs.
Every covered output must have replacement mode with a visible full-coverage
Backdrop. Bindings and pointer actions are eligible only once the matching
presentation has completed on all covered outputs and every mirrored head.
Protected recovery/session controls retain precedence.
Existing application captures keep their existing owner; modal admission waits
for them to settle rather than transferring ownership. Ordinary unbound input is
consumed within the admitted modal scope and is never forwarded through previews.
The session defers installation of replacement records while application keys,
leases or pending focus handoffs remain. This keeps the real application hit
layers available until those sequences settle. The regular presentation service
retries the committed records without requiring another WM proposal, revalidating
source and action admission before installation.

The owner retains requested, committed and presented identities separately.
Submission and a projection acknowledgement grant no input authority. A typed
presentation receipt reports the actual presented generation and output epochs;
revocation is an explicit lifecycle outcome, not a fabricated new presentation.
All-head consensus, completed replacement and any-head visibility are separate
facts. A lagging head which still shows a revoked tier keeps the output shielded
from new application input. A withdrawal receipt requires completed evidence that
every head has replaced that identity; missing consensus alone cannot certify it.
Target membership is intersected across all completed heads, independently of
stamp identity. A stamp shared by every head cannot attest a target absent from
one of them.
A completed frame without the publication revokes any held receipt and action
authority for that output. Requested records cannot keep invisible targets active.

The action wire identity is scoped by its authenticated session connection.
The input owner also retains a monotonic presentation epoch across reconnects;
reused WM publication or target counters cannot reuse an old receipt. The session
checks this completed authority before queueing an action and before accepting its
reply. Reducer membership validation alone is insufficient.

Close, source loss, output/topology change, suspend/control-epoch loss, policy
disconnect or reconnect revoke dependent input locally. Pending replies and
actions carry their original identities and cannot revive a revoked scope.
WM restart always begins with presentation closed. Consumed press releases remain
consumed after revocation; pre-existing application-owned sequences retain their
existing routing obligation. Source repaint may preserve a capture only when
all interaction fields and authority still match.

Revocation/withdrawal obligations have bounded owner state independent of the
peer action FIFO and notification credit. Queue saturation must not delay local
revocation, and an unsent action must never receive an invented cancellation.
At most one presentation action is outstanding; each is delivered once. Further
discrete actions use the existing bounded policy cause queue, retain their
presented identity and are discarded if it becomes stale. Overflow revokes the
scope and schedules withdrawal. Timeout uses the existing policy request
deadline; acknowledgement never extends that deadline or recreates authority.

## Wire and compatibility

Reserve capability bits 18 (`surface_instances`) and 19
(`presentation_actions`); actions require instances. Use extension records after
the frozen revision-3 projection records. The canonical KDL schema owns assigned
record/message numbers, sizes and reserved fields. Generate Rust/C framing and
keep Hagia's independent Nim implementation covered by shared conformance data.

Keep ordinary projection outcomes separate from presentation receipts. Add a
dedicated typed presentation action request; do not overload normal Action,
OutputAction or raw pointer interaction payloads with instance semantics.
Message 54 carries PresentationActionRequest; message 55 carries
PresentationOutcome, with Presented (1), Revoked (2) and Withdrawn (3) outcomes.
Each receipt names a nonzero actual presentation epoch. An unpresented candidate
receives its projection settlement and cannot manufacture a presentation receipt.
Unnegotiated records, unknown flags, duplicate identities/order, stale epochs and
count mismatches are refused before publication. Existing clients send no new
records and retain their behavior. A requested unsupported feature fails clearly.
The unreleased overview-specific WM and shell revision-9 prototype is archived,
not a compatibility obligation or a second supported service.

Production sessions advertise these capabilities only with a native frame
retirement owner. Software composition through that owner is supported. The
software-only diagnostic cycle without native scanout does not render this
tier or establish presentation completion, so it omits both capabilities.
Loss of the retirement owner revokes dependent input and withdraws publication;
it never substitutes committed fallback state for actual completion.

## Acceptance and implementation ownership

The director owns protocol/schema and WM integration; the renderer owner owns
composition/source lifetime; the session owner owns presented input and delivery.
The t099 shared input/native-owner paths remain with that task until handoff.
The [implementation plan](notes/plans/mjnpxubs-generic-wm-presentation-foundation-and-input-contract.md)
records the queue relationships and authorizations.

Acceptance requires independent codecs; repeated and preview-only sources;
source-update damage with stable interaction identity; CPU/native equivalence;
fractional scale and mirrored retirement; release after close; no application
input through instances; late replies, source/output loss, reconnect and full
queues; and paired WM policy settlement. Retain the source-lookup and
source-generation negative controls. Headless evidence does not establish
physical GPU/KMS behavior. No live install or reload is part of these gates.

# Sophia Window Manager API

**Role:** normative native spatial-policy protocol.
**Status:** `sophia_wm_v1` interface major 1 wire revision 3 is stable.

The optional [window translation contract](window-transitions.md) adds generic
shared positional targets without changing frozen revision-3 records. WMs own
spatial policy; Engine owns the presentation timeline and GPU rendering.
Revisions advance as the protocol grows; negotiation, not a freeze, is what
keeps clients and servers compatible. The experimental Rust API v7 transport
has been removed.

Sophia has one spatial-policy role. A WM speaks that role directly over the
language-neutral `sophia_wm_v1` IPC protocol. It is neither an application
authority nor an alternate compositor. Legacy X11 WMs must be ported; Sophia
does not provide a synthetic X server or compatibility adapter for policy.

The common endpoint, framing, negotiation, transaction, versioning, recovery,
and stability rules are in the [Sophia Native Protocol
Family](sophia-policy-ipc.md). This document is the `sophia_wm_v1`
specialization: it defines the policy facts and proposals allowed across that
connection without creating alternate transport or lifecycle semantics.

## Ownership

Engine owns physical input, shortcut matching, authoritative visibility and
focus, scene validation, atomic commit, rendering, and scanout. The session
owns endpoint admission, process supervision, application launch, logout, and
protocol-specific polite-close execution. A WM owns only its private policy
model and proposes bounded changes.

A WM never receives:

- physical input streams, grabs, client sockets, or protocol handles;
- XIDs, namespaces, titles, classes, PIDs, paths, or credentials;
- client pixels, renderer or DRM handles, or portal payloads; or
- another WM's tags, views, trees, columns, stacks, or checkpoint.

Engine preserves the last committed projection while policy is absent,
incompatible, malformed, timed out, or restarting.

## Form-Neutral Policy

“WM” names the spatial-policy slot, not a required desktop form. A client may
tile, scroll, stack, float, combine those approaches, or implement a
single-application session. Sophia packets carry opaque identities,
capabilities, constraints, state, and geometry; they do not carry a tiling
tree, workspace array, tag mask, scrolling column, or global stacking model.

Shell, metadata, portal, and session powers remain separate. A combined
desktop may implement several clients, but independently authorized endpoints
are insufficient when those clients can collude through one process. The blind
spatial-policy role must occupy a different supervised protection domain from
metadata-bearing shell, broker, portal, and application-frontend roles. One
source tree or executable may launch those processes; they may not retain
ambient IPC that recombines their authority.

## Removed Experimental API v7

The client-hosted Rust socket, revision negotiation, workspace reducer, policy
reload frames, demo server, and Engine transport were removed once revision 3
passed its gates. Configuration accepts only
`--wm-interface=sophia_wm_v1`; `api_v7` fails closed. There is no alternate
workspace-policy transport.

## Public `sophia_wm_v1` Negotiation

The Sophia session hosts an owner-only WM endpoint and admits one supervised
client. The endpoint and message kind select `sophia_wm_v1`; `ClientHello`
carries the client's minimum and maximum revisions and
requested capabilities. `ServerWelcome` selects a supported revision and
capability subset and supplies a server-owned connection epoch plus effective
limits.

The current server selects revision 3 only; the ordered nonzero range must
include it. Revisions 1 and 2 were experimental development stages, not retained
stable endpoints. Hello and welcome use transaction zero. Welcome advertises
16 outputs, 1,024 surfaces, 256 bindings and 65,520 chunk-data bytes (the
16-byte chunk prefix uses the remainder of the common payload ceiling).
Unlike shell's required-capability negotiation, WM capabilities are requests.
Snapshot/projection begin, chunk and end repeat the same nonzero transaction
and connection epoch; candidate base-snapshot identity is separately validated.
These frozen layouts do not inherit output's independently chosen proposal IDs.

Capabilities are orthogonal and fail closed at the point of use. The initial
set covers registered bindings, opaque session actions, reduced pointer
interactions, and bounded Engine chrome policy. Unsupported bits do not reject
negotiation: the server intersects the requested set with what it offers and
returns the result, so a client must read `ServerWelcome.capabilities` and
treat any missing bit as unavailable. The wire has no negotiation-error
message, which makes the intersection deliberate — a silently dropped
connection would be the only alternative, and it would explain nothing. An
operation is legal only when its selected revision and negotiated capability
admit it; using an unnegotiated capability fails the operation, not the
handshake.

Revision 2 separates action registration from physical shortcut ownership.
The policy client advertises a bounded catalog containing one nonzero opaque
action ID, a unique semantic name of at most 128 bytes, and an optional
session-operation slot. It never supplies keycodes or modifier masks. The
trusted session coordinator resolves the shortcut authority's prepared profile
against the admitted catalog, then gives Engine only normalized chords and opaque IDs.
The offered catalog may include standard session-operation slots (1 through 7)
that this desktop has not admitted. Session omits those actions before shortcut
resolution and snapshot publication; an offer never creates an operation grant.
Unknown slots remain invalid. The first snapshot carries the accepted catalog.
An explicit shortcut naming an unavailable operation still fails resolution.
Only unavailable compiled-default shortcuts already reported by startup are
omitted; the prepared source and its digest remain unchanged.
Unknown names, unavailable explicit bindings, unsupported pointer gestures,
excessive registrations, duplicate chords, and the emergency chord fail
closed. The slot, not an action-number range, associates a committed
activation with one session-owned capability.

Revision 3 adds the optional `profile_activation` capability without changing
normal snapshot/projection settlement. Before policy configuration or a first
snapshot, Sophia names one staged desktop-profile generation and fixed 32-byte
digest through prepare and activate commands. Hagia acknowledges only the exact
candidate it loaded from Sophia's owner-only fragment. Every command and
completion carries the connection epoch and a nonzero transaction; rejection,
timeout, disconnect, or rollback leaves the graphical startup gate closed.
Prepare, activate, and rollback outcomes use a profile-specific closed enum and
do not share the action/chrome configuration generation namespace. Every
`sophia_wm_v1` startup requires the capability and negotiates it before
graphical resources are opened. The former `--wm-profile-activation` proof
switch is accepted as a compatibility no-op; it cannot bypass or strengthen
the mandatory barrier. Supervised restart reattaches the same generation and
digest under a fresh connection epoch.

## Complete Scene Snapshot

Engine sends one generation-tagged snapshot containing:

- one explicit active output and up to 16 opaque outputs with their full and
  policy work rectangles;
- up to 1,024 live manageable opaque surfaces;
- current visible focus per output when one exists; and
- advertised opaque session-action tokens.

A surface record contains:

- `SurfaceId` and surface generation;
- broad toplevel, dialog, utility, popup, or unknown kind;
- reduced transient owner when valid;
- movable, resizable, focusable, closable, and fullscreen capabilities;
- frontend presentation requests and current committed state;
- minimum, maximum, and exact-size constraints; and
- current committed outer geometry.

The snapshot contains no workspace, tag, `ViewId`, application identity, or
policy-private layout state. It is complete even when some surfaces are hidden.
Chunking is a wire detail and cannot expose a partial snapshot to the policy
reducer.

Engine sends a fresh snapshot after a relevant scene or output change. One
request may also carry an opaque action activation, reduced focus request, or
bounded interaction request as its cause. The cause does not weaken snapshot
validation.

Each non-idempotent action activation is ordered and delivered once. A bounded
queue may reject new activations under pressure, but it must not merge repeated
focus, movement, view, or layout actions. Replaceable scene refreshes and
continuous interaction geometry may coalesce to their latest state.

### Logical Output Contract

`SnapshotOutput` carries only opaque output identity and generation, visible
focus, full logical bounds, and policy work area. It carries no scale,
transform, mode, connector identity, enabled flag, or reserved expansion
space. Revision 3 omits them deliberately, and each omission is a decision
with a reason rather than a gap waiting to be filled:

- **Scale never crosses.** Engine hands policy a logical space already expressed
  in the applicable scale.
- **Transform never crosses.** Rotation is absorbed into the logical bounds.
- **Mode and timing never cross.** Refresh, pixel clock, and connector timing
  remain output-authority facts.
- **Enablement is expressed by omission.** A disabled output is absent from the
  complete snapshot.
- **Mirroring is invisible.** One logical output backed by several connectors is
  still exactly one `SnapshotOutput`.

A policy needing connector mode or head identity has been handed the wrong
abstraction. Output authority may use those facts through its separate role,
but it cannot publish them through `sophia_wm_v1`.

## Projection Proposal

A request repeats the latest strictly admitted private policy generation. A
response repeats the connection epoch, transaction ID, and base snapshot
generation, and explicitly selects one live active output. It lists every
output affected by the decision. Each listed output contains a complete ordered
projection; list order is bottom-to-top stacking order.

Each projected surface may specify:

- requested client-content size;
- committed outer geometry;
- optional crop; and
- protocol-neutral transform.

The proposal may name one focused surface for each affected output. A surface
that was visible on an affected output and is absent from its replacement is
hidden. An unlisted output retains its committed projection. Moving one
surface between outputs therefore requires both outputs in the same proposal.

Session materialization reconciles fresh content only for the replaced outputs.
Although the staged reducer exposes a complete scene, unlisted outputs retain
their already committed content layers. Those layers must not cross chrome
clearance again or replay an earlier configure request. A partial interaction
proposal therefore does not require the WM to restate the other monitors.

Engine validates the complete candidate before mutation:

- connection epoch, transaction, snapshot, output, and surface generations
  must be current;
- every output and surface must exist and remain authorized;
- counts, geometry, crop, transform, sizes, and constraints must be valid;
- no output or surface may be duplicated;
- a surface may appear on at most one output; and
- focus must name a visible focusable surface.

Changing the active output requires both the old and new output in the
affected set. Fullscreen geometry must equal the full output rectangle.
Ordinary, maximized, and minimized geometry must remain inside the work area,
and a minimized placement cannot hold focus. A minimized placement remains in
the semantic projection but is omitted from the renderer's layer candidate.

Success commits all affected outputs as one logical scene transaction.
Committed, stale, invalid, and timed-out outcomes are explicit. A rejected
proposal does not launch a process, change focus, partially change visibility,
or alter the last committed projection.

An accepted proposal may require frontend configure and content settlement
before it becomes committable. Engine retains the prior coherent scene during
that interval and sends `committed` only after authoritative frontend state and
renderable content match the candidate. If settlement changes a relevant fact,
Engine requests policy again from a fresh complete snapshot rather than
silently rewriting the proposal.

A committed policy response settles the cause that raised it, and the settlement
stands until snapshot-visible facts change. The owner requests policy again on a
fact change, never on a timer and never because the condition that raised the
cause still holds. A window manager may answer a surface's `Manage` request by
placing nothing at all -- a layout that shows one window does exactly that, and so
does a policy that maps a surface minimized. Those are answers rather than
failures, and re-asking them is not patience but a loop: the session commits, the
scene re-projects, and the same question returns on the next turn for as long as
policy keeps its layout. A surface waiting on such an answer is admitted by
whichever later proposal does place it, whatever cause raised that proposal, so
the settlement withholds a request rather than the surface.

For X11 surfaces, a public-policy presentation transition also crosses a
bounded frontend control. The frontend installs `_NET_WM_STATE` plus ICCCM
`WM_STATE`, emits selected `PropertyNotify` events, flushes them, and only then
acknowledges the control. That acknowledgement is a commit prerequisite. A
timed-out or invalidated proposal queues restoration of the last committed
frontend state; client `ChangeProperty`, `DeleteProperty`, and delete-on-read
cannot overwrite these Engine-owned feedback properties.

Physical outputs retire independently. The logical projection transaction
feeds the existing visual-preparation and retirement model; it does not claim
that separate displays flip at one simultaneous physical instant.

## Actions And Interactions

The proposed [scripting contract](scripting.md) reuses the registered,
argument-free policy-action path after session-owned caller authorization.
Command meaning remains private WM policy. The WM neither serves a scripting
endpoint nor receives caller credentials, namespace identity, or application
metadata. A script request is not a synthetic physical-input event. The public
CLI is unimplemented; parameterized commands require a separate bounded
argument contract and do not follow from the existing action ordinal.

Engine matches registered physical chords before client routing, emits one
activation on the initial press, suppresses repeat activations until release,
and exposes only the action ID and reduced snapshot context. No rejected action
falls through to application input.

Session actions are advertised opaque tokens. A WM may request an advertised
token with an optional opaque target. It cannot supply an executable, argument,
environment, signal, or protocol handle.

The token is meaningful only under its session issuer, issuer/revocation
epochs, WM recipient epoch, operation class, and optional target generation.
It is not interchangeable with a broker, shell, or policy action carrying the
same integer representation. Activation identities are ordered and deduplicated
inside the recipient epoch; reconnect does not replay an ambiguous request.
A committed binding may name one advertised operation slot. The numeric action
ID itself has no operation meaning, and execution occurs only after the
projection transaction commits. Stale, expired, revoked, wrong-recipient,
wrong-generation, and duplicate activations fail closed.


An unmodified primary press on an unfocused visible surface may produce a
reduced focus request. Engine retains the ordered input handoff until Engine
focus and frontend protocol focus settle on the same exact generational
surface. Before release, every buffered target is revalidated against both the
last-presented interaction projection and current frontend route membership.
Timeout, removal, replacement, or route loss drops the complete handoff instead
of routing a prefix to stale focus.

Move, resize, drag, and scrolling interactions remain Engine-owned grabs.
Policy receives only the target, operation kind, and bounded geometry update
and returns an ordinary projection proposal. It never receives raw motion,
button payloads, device identity, or cursor authority.

### Continuous Interaction Vocabulary

Revision 3 permanently defines `Move`, `Resize`, `Drag`, and `Scroll`.
`Begin`, `Update`, `End`, and `Cancel` are the complete phase vocabulary.
Move, resize, and drag carry output-local geometry with no axis. Scroll carries
a signed delta pair, leaves width and height zero, and identifies a horizontal
or vertical source axis. A non-cancelled zero scroll is invalid; cancellation
may carry zero because it terminates authority rather than motion.

Replaceable `Update` values may coalesce only for the same exact target, kind,
axis, capture, and authority epoch. Begin, End, and ordinary cancellation remain
ordered. A security transition clears capture, discards queued updates, and
prioritizes one `Cancel`; policy restart advances the observed capture epoch
before physical input resumes. Drag and scroll have stable wire meanings but no
live Engine producers yet.

A policy whose private state or validated configuration changes may request a
fresh cycle with a strictly increasing private generation. The request contains
no placement and cannot mutate Engine state; Engine replies with an ordinary
complete snapshot and projection request carrying the admitted generation. One
pending request coalesces by unioning its affected outputs. A newer generation
arriving during an in-flight relayout remains pending for one later complete
refresh; it is not duplicate-elided. Action activations retain their bounded
order.

## Recovery And Replacement

The connection epoch changes whenever the active policy process changes. Work
decoded from an older connection cannot commit after replacement, even if it
reuses a transaction or surface generation.

Disconnect, malformed transfer, timeout, and crash discard incomplete work and
leave applications and committed visuals alive. A replacement negotiates from
the beginning and receives a complete current snapshot. Engine does not store
or interpret the predecessor's workspaces, tags, views, or layout history.

The owner re-offers a cycle after stale rejection or timeout because those
outcomes mean the scene may have moved and recovery begins by reading a fresh
snapshot. Invalid rejection and disconnect are terminal for that connection;
repeating the same candidate would spin rather than recover. A client waits for
the owner to send the replacement snapshot instead of polling or retrying on a
timer.

A WM may maintain a private session-local checkpoint. It must reconcile that
checkpoint against the new snapshot and discard stale opaque IDs. Switching to
a different WM never transfers that checkpoint.

For the installed Hagia profile, the session provides an owner-only checkpoint
path inside the policy endpoint directory. Atomic replacement lets it survive
only supervised child restarts within that session; session teardown removes
it before endpoint cleanup. The checkpoint is implementation-private and is
not a `sophia_wm_v1` record or durable user configuration.

## Porting Existing WMs

An existing WM may reuse its private layout, workspace, and focus model, but it
must expose that model as a native `sophia_wm_v1` client. The port consumes
opaque Sophia snapshots and produces complete Sophia projections. It cannot
depend on root-window ownership, real X Authority metadata, synthetic X
windows, raw input, application pixels, or global command execution.

Every implementation passes the same semantic conformance suite. A design that
requires forbidden authority is rejected rather than widening Engine or adding
a client-specific adapter.

## Stability Gate

Revision 3 required retained Triad behavior to stop exposing missing WM facts
or operations, and required the independently implemented Hagia, Rust, and C
clients to pass identical negotiation, snapshot, projection, action, focus,
multi-output, rejection, timeout, restart, and last-layout tests. Those gates
passed, including the retained physical-output apply/rollback evidence and an
immutable archived C99 client.

Revision 3 is therefore complete, which is not the same as final. This
protocol is a work in progress and later revisions may add records, message
kinds and vocabulary. What holds compatibility together is negotiation: a
client names the range it can speak in `ClientHello` and the server selects
one in `ServerWelcome`, so a newer client meeting an older server operates at
the revision they agree on with the newer vocabulary simply absent.

That contract carries real weight here, because the policy client and the
Engine can be replaced independently -- the client lives where its owner can
rewrite it and the Engine ships in a signed release. A version skew is
therefore ordinary rather than exceptional, and the rule for it is: operate at
the selected revision, or refuse naming both revisions. Never parse a record
one side meant differently. The immutable archived revision-3 C99 client
exists to keep older-revision service honest as newer ones land.

A record that has shipped in a released revision does not change shape within
that revision; a new revision is how a shape changes. Later shell, broker,
portal, and output work remains separately gated. Released revisions remain
supported according to the [Sophia Native Protocol
Family](sophia-policy-ipc.md).

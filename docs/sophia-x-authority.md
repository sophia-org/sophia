# Sophia X Server Frontend

**Role:** subsystem contract and current implementation status.

The Sophia X Server Frontend is Sophia’s long-term modern X server
implementation. It presents the **X11 API and wire protocol** directly to
applications, then emits `SurfaceTransaction` values to Sophia Engine. It takes
the Phoenix strategic approach: a clean-room implementation of the modern X11
subset real applications require, expanded by compatibility evidence rather
than by reproducing all of Xorg.

It is not a plan for a separate application-facing Sophia display protocol. X11
is the native application API of this path; forward progress happens in the
server architecture, DRM/KMS presentation, and targeted X11 extensions. XLibre
is a retired prototype and possible future compatibility provider, not an
active integration lane. The implementation crate is currently named
`sophia-x-authority`; the name reflects its protocol role and does not narrow
the product direction.

The frontend is not the compositor. It terminates X protocol, owns X resource
semantics, applies the selected classic or confined session profile, and
translates client-visible X state into Sophia-owned data.

## Naming

Use **Sophia X Server Frontend** for the component and **X11** for the API it
implements. Use “X Authority” only as a shorthand for its protocol-semantic
role or the existing crate name. Do not describe it as an X11 compatibility shim
or an “X12” replacement protocol.

## Authority Boundary

The frontend owns:

- X client sockets and protocol parsing;
- X resource IDs, atoms, properties, windows, pixmaps, GCs, cursors, and
  colormaps where required for compatibility;
- namespace-aware resource lookup and event subscription;
- X focus, grabs, selections, configure, map/unmap, and lifecycle semantics;
- X drawing completion and buffer readiness;
- reduced metadata candidates for the metadata broker;
- portal request facts for cross-namespace transfers.

The frontend must not own:

- physical input devices;
- compositor scene graph or hit-testing;
- final layout, workspaces, or global shortcuts;
- compositor chrome;
- portal policy decisions;
- renderer imports, frame scheduling, or scanout.

## Production Boundary And Current Implementation Status

The production boundary is deliberately a small, bidirectional contract. It
keeps X11 semantics in the frontend and all physical display authority in
Sophia Engine:

| Direction | Contract | Owner and rule |
| --- | --- | --- |
| X11 client → frontend | local Unix connection, setup authentication, X11 requests, and X resource lifetime | The frontend owns parsing, client identity, XIDs, atoms, windows, properties, selections, grabs, and client-visible replies/events. Production setup authentication must be explicit; an owner-only socket is a transport guard, not a replacement for X11 authorization. |
| Frontend → Engine/live-presentation intake | `XAuthorityObservedTransactionBatch` containing the originating frontend client when available, authoritative `XAuthoritySurfaceRouteObservation` owner facts for transaction or presentation-intent surfaces, `SurfaceTransaction` values, surface removals, complete `SurfaceOutputReservations` replacements, CPU buffer updates, DMA-BUF registrations, fence registrations, and Present submissions | This is the only visual ingress. The causing client is an actor, not an ownership claim: classic-shared clients may operate on one another's surfaces, while each route fact retains the creating client and its admission. Passive helper windows do not gain an Engine route merely because a frontend request reported their state. Every request receives one frontend-global `TransactionId`; the connection-local X sequence remains wire-only. The queue is bounded and lossless: production backpressures one connection's X11 request dispatch until Engine accepts its next ordered batch. Concurrent clients may interleave, but each client retains request order. A live consumer must transfer native FDs immediately into renderer-private ownership; Engine scene records receive Sophia surface, output-reservation, and buffer facts, never raw X11 parsing, XID ownership, or native renderer objects. |
| Engine → frontend | `RoutedInputRequest` plus `XAuthorityClientControlCommand` | Engine selects the physical-input surface and owns global/local coordinates. The frontend resolves the surface to its connection, applies authority-local XKB/pointer state, and requests X-visible focus or configure results. It returns client-labeled delivery/control acknowledgements. |
| Engine → frontend | bounded `OutputTopologySnapshot` | Engine remains the source of physical output facts. Setup and populated RandR resources derive from the current validated generation; accepted live updates produce mask-selected screen, CRTC, output, and resource notifications. |
| Engine/backend → frontend | `XServerFrontendProtocolRouter` presentation feedback | The cloneable protocol-only router emits Present Complete and Idle by exact `TransactionId`. It cannot route input, mutate scene state, import buffers, or submit scanout. Feedback phases are independently ordered: compositor Copy releases and idles its captured source before Complete, while a future direct Flip may complete before Idle. The route remains live until both phases arrive once. |

The existing implementation covers every row. X11 socket dispatch
emits bounded actor-attributed batches with separate immutable owner routes;
Engine routes key/pointer events plus focus/configure commands back to the owning client; accepted Engine
topology updates drive RandR notifications; and the broker can clone a
protocol-only Present feedback router. The persistent native session transfers
registrations into backend-owned resource storage, renders mixed CPU/DMA-BUF
frames, and routes the final row only from matching page-flip retirement. The
frontend supports bounded concurrent
workers: callers use
`serve_next_concurrently[_traced]` to accept independent clients and
`wait_for_clients` to reap the accepted batch. It shares only independently
synchronized runtime, atom, property, and connection-lease state; the default
cap is 16 clients and `XServerFrontendConfig::with_max_concurrent_clients`
sets a different nonzero cap. A simultaneous-client regression holds one
client live while a second maps its window, then proves cleanup returns the
lease count to zero. `XServerFrontendRouteBroker` is now the explicit bounded
Engine-facing ingress for simultaneous workers: callers enqueue client-addressed
input or control, call `route_pending`, and each connected routed worker owns
only its private queues. Unknown and disconnected input routes are rejected.
Saturating a private input queue quarantines only that endpoint: the broker
removes all of its route senders, reports `RouteRejected` for a tracked event,
and continues serving other clients. Worker teardown unregisters the remaining
client state before its cleanup batch is observed. The persistent live session
now uses the bounded brokered service loop: it admits and reaps routed workers
up to the configured cap, and
on shutdown it stops admission before draining connected workers. A two-client
socket regression proves each worker receives only its own routed key and
configure request, returns a client-labeled acknowledgement, and tears down
cleanly. `x-authority-xterm-two-client-smoke` now launches two independent
real xterms against a two-worker frontend, waits for both client routes and CPU
surfaces, routes distinct key sequences by client ID, and proves two successive
pixel changes before draining the service. The live launcher still starts one
xterm by default, but `--secondary-terminal` starts and supervises a second
xterm on the same bounded frontend. The earlier two-xterm and paired Milestone 3
KMS launchers are retired; their verifiers preserve historical evidence only.
Those runs required physical keyboard and pointer input, authenticated RandR
delivery, configure-plus-pixels resize, two retained CPU layers, and clean KMS
teardown. Use the [native-session validation paths](validation.md#native-session-integration)
for current sessions.
Initial Engine focus is
acknowledged by the owning X11 client before the input proof begins, so
either focused terminal can demonstrate delivery.
Focus transitions are authority mutations rather than input-writer side
effects. A routed physical surface change resolves the old and new client once,
then emits each mask-selected core Focus event followed by its selected XI2
Focus event from the same passive transition packet. Repeated focus does not
emit, cross-client queues retain per-client order, and no later key event is
required to complete a focus handoff. A frontend focus acknowledgement alone
cannot release buffered pointer input: each exact generational target must also
remain in the last-presented Engine projection and current frontend route
table. Stale membership discards the whole buffered sequence.

Per-output remembered focus is distinct from seat focus. When a committed
policy projection selects an output without a focused surface, Engine clears
the previous client's keyboard focus through the frontend control path. The
other output keeps its remembered window for a later return. A rejected
projection and a session operation alone cannot clear focus. This keeps the
workspace receiving keyboard input consistent with the active output used by
application launches and the picker.

Explicit pointer-grab preparation carries the requesting connection's last
actually enqueued authority-observation receipt. Engine must apply and account
for that prefix before validating mapping, admission, and owner eligibility.
The frontend releases shared authority-state locks while awaiting the existing
synchronous reply, then revalidates exact X state before mutation. Neither
Prepare nor activation waits for pixels or scanout; physical delivery waits
for eligible presented evidence under the
[application lease contract](target-resolved-input.md#explicit-grab-preparation).
This distinction lets a client draw after grab success without granting it
unpresented input. Scene revisions require current target and scope evidence;
namespace, control, session, device, and output boundaries remain exact.

Client-bound keyboard events use the analogous nonspatial boundary. Engine
resolves shortcuts first, then retains unmatched keys while Engine focus is
ahead of frontend focus. The bounded sequence reaches XKB/event-mask delivery
only after the matching exact-surface acknowledgement; it is discarded rather
than retargeted on invalidation. Original libinput timestamps remain attached
to released events so proof and latency accounting describe physical ingress,
not the later acknowledgement tick.
X11 map/configure lifecycle
updates no longer overwrite a surface's committed-pixel generation. Live setup
and populated RandR CRTC/output/mode resources use Engine-derived topology
facts. Each accepted client now gets
a disjoint X11 setup resource-ID range. Every currently supported XID-creating
wire path—window, pixmap, GC, font, colormap, glyph cursor, and reduced
MIT-SHM segment—rejects an XID outside that range with X11 `BadIDChoice` before
it reaches runtime state. Core window, font, pixmap, GC, and cursor creation
also shares one resource namespace: one kind cannot replace a different live
core resource with the same XID. Collisions return `BadIDChoice` without
mutating the original resource. Every successful setup also receives a monotonic
frontend client identity and retains an XID-range lease until its connection
ends. The lease is the cleanup ledger key; it does not restrict ordinary
same-namespace references in the classic shared-X profile. Resource cleanup
now reaches an Engine-visible surface-removal path. The classic shared-X
existing-resource policy is explicit: an authenticated client may refer to,
mutate, or destroy an existing resource in its shared namespace, even when a
different connection created its XID. XID-range checks apply only to resource
creation; the lease remains a teardown ledger, not an access-control list.
`XServerFrontendConfig`/`XServerFrontend` make the local socket path and
namespace explicit, reject invalid namespaces, restrict the socket to its
owner, and refuse to replace a non-socket path. The configuration can now
consume an immutable session-created `NamespaceContext`; the legacy constructor
creates a classic-shared context for existing smoke callers. Production config
installs a protocol-neutral admission policy: after setup authentication and
before X client/resource allocation, the policy receives bounded authentication
provenance plus kernel peer credentials and returns a distinct immutable
`ClientAdmissionContext`. Denial receives a normal X11 setup-failure reply. An
admission lease revokes the context after resource/route cleanup and also on
early worker errors. The live classic session requires the peer UID to match
the session user and deliberately assigns each connection the same shared
namespace through the session registry. The configuration can also require a
session-scoped `MIT-MAGIC-COOKIE-1`; raw cookie bytes never enter the admission
record. Legacy smoke helpers still default to unauthenticated local sockets.
The live supervisor publishes a fresh kernel-random cookie in a standard
owner-only Xauthority record, supplies only the path to launched terminals, and
removes the file on teardown. Multiple independently credentialed confined
groups on one listener remain before treating the listener as a general local
X server.

For child-launch placement, Linux admission also captures a process start identity
while a socket-derived pidfd proves the connector remains alive. Failure to obtain
that optional evidence leaves ordinary X admission intact. Session-owned ancestry
resolution runs on the admission worker, outside the display loop; frontend route
facts associate each new managed top-level with its creating admission. The causing
client never supplies another window's launch identity. Sophia copies only a
committed opaque policy context into a child's first admission snapshot; process
and namespace evidence stays outside Hagia. See
[child-launch origin](sophia-policy-ipc.md#optional-child-launch-origin).

The live launcher accepts `--namespace-profile=classic|confined`. Classic is
the default and intentionally shares its namespace among launched terminals.
Confined allocates a fresh group namespace with explicit zero portal
capabilities. Admission contexts preserve that immutable profile. A concurrent
wire regression assigns two clients separate confined contexts and proves
cross-namespace map, property mutation, and selection ownership fail with
native `BadAccess`; foreign selection conversion returns
`SelectionNotify(property=None)`, and denied property mutation emits no metadata
candidate. These proofs caught and closed missing validation on map, property,
selection-owner, selection-requestor, drawable, and event-target paths. A
routed two-client regression additionally proves that a rejected foreign
event-mask request cannot redirect a confined worker's key events: input stays
on the broker-addressed client and retains that worker's root target. Separate
confined-group credentials on one listener remain active work. The routed
service now accepts targeted revocation by `ClientAdmissionId`; it disconnects
only the matching worker and leaves route/resource/surface/admission cleanup in
that worker's ordinary teardown path. The simultaneous classic-client proof
verifies its peer remains usable.

### Connection Lifecycle

After successful setup, the frontend records the client ID and its setup XID
range as a connection lease. The lease is released only after the client stream
and optional input/control writers have stopped, including an observer or
writer error. This establishes a durable ownership key without changing classic
shared-X semantics: it identifies resources a connection was allowed to create,
but does not prohibit another trusted client in the same namespace from
referring to them.

`DestroyWindow` and connection teardown now use the same authority-side
destruction primitive. Teardown releases the disconnecting lease's supported
windows and CPU buffers, pixmaps, GCs, fonts, cursors, SHM segments, window
properties, and selection ownership. Every destroyed window produces a
surface-removal fact; the Engine removal intake prunes its committed snapshot,
while the live layout, CPU scene, and focus state prune their corresponding
state. A sequential-client proof confirms that a later client receives
`BadWindow` for the former client's window while resources from a different
XID range remain intact.

The existing-resource policy is deliberately profile-specific. The cleanup
ledger establishes which resources disconnect teardown must reclaim, but it is
not an access-control list in the classic profile. The regression suite proves
that a peer with a disjoint creation range can map an existing window in the
same namespace. A confined profile must instead add connection routing and
capability checks before it can permit any cross-client resource operation.

The two session profiles have these precise semantics:

- **Classic shared-X:** one trusted local session assigns all participating
  clients the same namespace. Ordinary X11 inspection, coordination,
  selections, and window-manager interaction remain available within that
  session. Any authenticated client in that namespace can use existing X11
  resources; XID allocation remains per-connection solely to prevent creation
  collisions and make disconnect cleanup precise.
- **Confined:** the launch/authentication layer assigns a distinct namespace
  and explicit capabilities for a client group. Cross-namespace discovery,
  properties, selections, and input are denied unless a narrow portal grants a
  transfer. This profile is explicitly launchable, uses client-aware routing,
  and has paired Milestone 3 session evidence; it is not an alias for the
  classic shared namespace.

## Modern X11 Compatibility Subset

The first practical target is not all of X11. It is enough protocol to run real
toolkit applications while preserving Sophia's authority boundary.

Required baseline:

- core connection setup, errors, requests, replies, and events;
- window lifecycle: create, destroy, map, unmap, reparent, configure, circulate;
- pixmaps, graphics contexts, basic drawing, copy area, clear area, and expose;
- atoms and properties for ICCCM/EWMH enough to support titles, classes,
  protocols, states, selections, and WM hints;
- event masks and event delivery for structure, property, focus, keyboard, and
  pointer events;
- selections for clipboard and primary selection handoff;
- XKB for keyboard compatibility;
- XFixes for selection owner tracking and modern client expectations;
- Sync for compatibility with `_NET_WM_SYNC_REQUEST`;
- Render for modern toolkit drawing paths;
- SHM for software-backed pixmap/image updates;
- DRI3/Present for GPU-backed buffer handoff;
- RandR surface/output facts enough for clients to observe screen size;
- bounded direct-Mesa GLX bootstrap for clients that render through the proven
  DRI3/Present path. The catalog exposes depth-capable, double-buffered
  TrueColor visuals and FBConfigs; both legacy visual-based and modern
  FBConfig-based direct contexts normalize to the same bounded runtime records.
  Direct MakeCurrent binds a validated context/drawable pair while actual
  buffers continue through DRI3/Present.

The core color contract is fixed TrueColor, not a mutable indexed palette.
Setup advertises the conventional 1/4/8/15/16 auxiliary pixmap depths without
visuals, one 24-bit XRGB visual, and one 32-bit ARGB visual with 8-bit red,
green, and blue masks. Pixmap depth is retained for masks, stipples, cursors,
and drawable geometry; auxiliary pixmaps never become Engine color surfaces.
X Authority owns passive colormap records and requires every window depth,
visual, and colormap to agree. `AllocColor`
quantizes each RGB16 component to the advertised high eight bits and expands
the returned screen values by `0x0101`; ARGB allocation supplies an opaque
alpha byte. `QueryColors` performs the inverse mask lookup and rejects bits
outside the selected visual. `AllocNamedColor` accepts only the bounded,
checked-in retained-client table and returns `BadName` for anything else.
Both allocation requests retain per-client reference counts for each RGB
component. `FreeColors` releases the requested component references, including
plane-mask combinations, and reports `BadAccess` for an unallocated component;
valid components are still freed when another component is invalid.
`CopyColormapAndFree` moves the requesting client's references into the new
colormap. References are released with the client's resources (respecting
retained close-down modes), and freeing a colormap removes all its references.
Invalid colormaps, pixels, visuals, depths, allocation modes, and duplicate
resource IDs retain their core X11 errors. No color identity crosses into
Engine; only normalized XRGB8888 or ARGB8888 content and opacity facts do.

Deferred unless a real app requires them:

- network transparency beyond local Unix sockets;
- full legacy font server behavior;
- indirect GLX as a primary rendering path;
- XTEST-style synthetic input as a privileged general API;
- broad extension coverage that does not affect real target clients.

## Probe-Driven Coverage

External-client coverage is admitted only when a real probe exposes a missing
request, reply, lifecycle, or error path. Current real-client smokes cover
root/window introspection (`xwininfo`, `xprop`), root property mutation
(`xsetroot`), drawing clients (`xclock`, `xeyes`, `xlogo`, `xmessage`), output
introspection (`xrandr --query`), Athena widget behavior (`xcalc`), and
terminal setup/lifecycle and drawing-transaction behavior (`xterm`), and GTK
startup behavior (`zenity`).

The maintained [X11 compatibility matrix](x11-compatibility-matrix.md) records
the exact command, evidence level, narrow proven behavior, and next gate for
each client class. It is the source of truth for admitting new X11 work; the
historical prose below explains why individual compatibility slices exist.

Each external smoke must keep `first_error=none`. New compatibility code should
remain bounded and narrow: for example, xcalc admitted `AllocNamedColor`,
`UnmapWindow`, padded one-character `PolyText8`, and normal client-disconnect
teardown without turning the frontend into a broad X11 conformance
project. xterm admitted `ConfigureWindow` and the bounded setup/drawing paths
needed to reach committed `ImageText8` transactions. Core drawing now applies
GC colors and raster operations to bounded XRGB8888 software buffers. Text uses
the vendored public-domain X.Org 6x13 ISO-8859-1 bitmap with its exact 6-pixel
advance, 11-pixel ascent, and 2-pixel descent. The real xterm proof pins that
font and white-on-black colors, locates a mixed-case glyph sequence in
materialized replacement/patch pixels, and requires both `ImageText8` and
`CopyArea` scrolling. A separate key-channel smoke
injects `sophia` plus Return and proves later xterm buffer generations change.
The two-client xterm smoke repeats that proof for two separate client IDs with
distinct `alpha` and `bravo` input sequences, proving the broker does not
broadcast those Engine routes. It remains CPU-buffer evidence, not KMS-backed
multi-app session promotion evidence.
zenity admitted selection-owner lookup, root colormap creation, core
ClientMessage delivery, reduced `MIT-SHM`, `RANDR`, `BIG-REQUESTS`, and the
probe-backed XKB/XI2 startup subset. Its regression now copies the client-owned
SysV SHM payload through a bounds-checked safe adapter and requires a committed
nonzero software image with `first_error=none`.

`XKEYBOARD` now advertises the probe-backed UseExtension, event-selection,
per-client flags, and GetMap subset. Engine-routed evdev keys are translated
inside the frontend with a bounded deterministic RMLVO configuration (default
`evdev`/`pc105`/`us`) and per-seat XKB effective-modifier state. Core
`GetKeyboardMapping`, `GetModifierMapping`, `KeyPress`, and `KeyRelease` remain
the client-visible compatibility baseline.

XI2 exposes the master pointer/keyboard pair (2/3) and one fixed virtual pointer
source (128) attached to pointer 2. This is a protocol device, not a disclosure
of the seat's physical inventory. Its ID lies outside XI1's device range, so
XI1 continues to enumerate only the core pair. AllDevices includes the source;
AllMasterDevices does not. Selected source events precede the corresponding
master event, whose source ID is 128. Both streams derive from the same
Engine-admitted target and namespace state.

The source cannot be detached or independently captured. A direct source grab
returns BadAccess before reserving an Engine lease. XIQueryPointer on the
attached source returns BadDevice; query the master for pointer position and
XIQueryDevice for source valuators. Ungrabbing the source cannot release a
master grab. Device hierarchy mutation and independent source control remain
outside the admitted frontend subset.

XI2 2.1 exposes relative pointer X/Y on valuators 0/1 and cumulative horizontal
and vertical scroll positions on valuators 2/3. Standard axis labels let clients
refresh their baselines when the pointer enters a window. FP3232 values encode
the signed integral word followed by the unsigned fraction word, each in client
byte order; they are not serialized as a native 64-bit integer. Query replies use current
authority-local state rather than class-template zeros. Pointer queries and
device events resolve the immediate descendant of the selected window, encode
coordinates relative to that selected window, and carry current button and
effective modifier masks. Only the compatibility button pair carries `XIPointerEmulated`; the smooth
Motion event is the source event. Engine axis packets remain free of X11 policy.

Core `QueryPointer`, `XIQueryPointer`, and the position and scroll valuators in
`XIQueryDevice` read shared, namespace-scoped input state. A new connection in
a trusted shared namespace sees its peers' latest admitted observation without
selecting events. A confined namespace receives no updates from its neighbours.
An unobserved or revoked pointer answers the initial zero position and empty
button state; these replies do not sample physical devices on demand.

Input publication follows epoch and freeze checks and precedes socket delivery.
Button replies carry the state after a press or release, and modifier changes
do not require another motion event. The query anchor remains the surface
Engine selected when an X grab redirects delivery. Engine's root and transformed
local coordinates remain distinct; the frontend derives X descendant offsets
from its current hierarchy. Configure and reparent changes adjust a stationary
anchor, and child lookup rejects unmapped, destroyed, or foreign resources.
Connection cleanup retains an observation while its namespace still has clients.

X window geometry reports the outer border corner; drawable and event
coordinates start inside the border. Descendant origins accumulate each
ancestor's border width. The top-level Engine surface and its raster start at
the same interior origin, and Engine configure geometry converts back to the
X outer corner. Borders reserve space without painting border pixels. Child
composition and IncludeInferiors are clipped to each ancestor's interior.

`WM_TRANSIENT_FOR` is reduced at the X boundary into two protocol-neutral
facts: property presence makes the root child client-positioned, while a
resolvable value supplies an optional presentation owner on `AuthoritySurface`.
A group transient naming the root therefore remains outside blind-WM placement
without inventing an Engine owner. The live session keeps attached visibility
tied to a mapped owner when one exists, and explicit unmap, property deletion,
or owner loss cannot leave a stale popup in composition.

Authority unmap takes precedence over a cached WM visibility projection,
including when an attached popup resolves to a managed owner. The live backend
also removes departed surfaces from pointer projections when the presentation
layout changes. Native retirement intersects its retained frame with current
eligibility, so an older frame cannot restore a dismissed target. This only
removes targets; surviving windows retain their last-presented geometry until
new pixels reach scanout.

For policy-managed admission, a successful frontend `AdmitSurface`
acknowledgement supplies the mapped fact. The session records it only after
matching the pending admission, transaction and surface lifetime; it cannot
wait for another client request to carry a presentation snapshot. A withdrawn
or destroyed admission cannot be revived by a late acknowledgement.

Unmapping also revokes the hidden surface's Engine input ownership, including
focus, pending focus handoffs, key repeat, pressed-key state and application
route leases. A popup loses that ownership when its presentation owner hides.
The session synchronizes focus clearing through the existing frontend control
acknowledgement; the WM chooses replacement focus. Retained pixels do not grant
a withdrawn surface membership in a WM snapshot, and passive updates while
hidden do not constitute another map request. A remap may request admission
without destroying the retained content.

A queued focus clear can outlive its target. A correlated `UnknownSurface`
acknowledgement for `ClearFocus` retires the request without failing the
session: destruction has already ended that target's focus. This does not
relax target validation for commands that establish new surface state, or
accept missing, mismatched or rejected-authority acknowledgements.

`_NET_WM_WINDOW_TYPE` is reduced at the same boundary because the external
blind WM never receives application properties. The decoder honors the EWMH
ordered ATOM list, skips unknown extension types, leaves `NORMAL` under policy,
and maps recognized dialog/menu/utility/splash/popup-like functional types to
`ClientPositioned`. Property replacement or deletion publishes the resulting
role change; no X atom crosses into Engine state.

Committed public-policy presentation state travels in the opposite direction
as a protocol-neutral frontend control. X Authority materializes fullscreen,
paired horizontal/vertical maximize, hidden/minimized, and ordinary/restore as
`_NET_WM_STATE`; it also publishes ICCCM `WM_STATE` as Normal or Iconic. Both
properties change as one bounded table operation. Selected `PropertyNotify`
events are flushed before acknowledgement. Mapped windows and pending policy
admissions cannot replace these Engine-owned feedback properties. A withdrawn
window may replace `_NET_WM_STATE` with initial hints for its next map, as GTK
does when reusing a dialog. This namespace-checked write changes only the X
property record; it does not commit Engine presentation state or bypass policy
admission. ICCCM `WM_STATE` remains protected, as do deletion and delete-on-read
of either feedback property.
The live policy transaction waits for this acknowledgement and restores the
last committed values if its candidate aborts.

## Namespace Model

The normative identity, profile, admission, capability, and grant contract is
in [namespaces-and-portals.md](namespaces-and-portals.md). This section records
the X-specific consequences.

Every client connection belongs to a `NamespaceId` before it can create
resources. In a classic shared-X profile, trusted clients deliberately share one
namespace and retain ordinary X11 inspection and coordination. In a confined
profile, launch tokens, socket routing, credentials, or a later broker select
separate namespaces. Namespace identity is frontend state and must not leak to
the WM.

Resource rules:

- XIDs are local to the authority and wrapped as `AuthorityLocalId` before
  becoming Sophia data.
- A successfully recreated client XID receives the next nonzero Sophia
  `SurfaceId` generation. Rejected creates do not consume the candidate, and a
  successful removal retires the exact frontend surface route before that XID
  can name a replacement.
- Cross-namespace resource lookup fails closed unless a specific portal flow
  grants a narrow transfer; same-namespace classic-X behavior remains intact.
- Event subscriptions are namespace-scoped in confined profiles.
- Properties may be visible within a namespace, but cross-namespace property
  discovery must not expose titles, classes, PIDs, paths, or atoms that reveal
  another namespace's private clients.
- Grabs and focus are authority semantics. Sophia Engine supplies target
  surfaces and local coordinates; the authority still applies X delivery rules.

## Surface Transactions

Each visible top-level or protocol surface maps to an `AuthoritySurface` owned
by the frontend and a Sophia `SurfaceId` owned by Sophia Engine.

The authority emits `SurfaceTransaction` records when a surface has new visual
state:

- `Pending` while target geometry or matching pixels are not ready;
- `Ready` when geometry, buffer, damage, and previous committed generation are
  coherent;
- `Failed` when the protocol path cannot produce a valid commit;
- `TimedOut` when timeout policy closes a slow transaction.

Sophia Engine commits only ready transactions whose previous committed
generation matches. Pending, failed, timed-out, stale, or invalid transactions
preserve the last committed visual state.

`target_geometry` is the logical policy-managed surface rectangle;
`target_content_size` is the exact extent expected from the transaction's
buffer. They are equal for a direct toplevel buffer. When a descendant window
presents, the frontend keeps the toplevel geometry, publishes the descendant's
content extent, and carries the accumulated child offset in the Present
submission. The live layout gate compares the imported buffer with the content
extent, while native retirement commits the logical extent. This mirrors
XLibre and yserver's separate window-tree geometry and Present offsets without
exporting X11 hierarchy into Engine. This applies equally to CPU, SHM, and GPU
Present sources. CPU/SHM copies are materialized in the presentation root's
backing, so their advertised raster extent is that backing's measured size;
GPU sources retain their imported size and accumulated child offset. A retained
CPU snapshot may supply raster variants only when its handle is the exact
source selected by the transaction. Its mere existence must never replace a
DRI3 source or change Present's fence/feedback route.

## Drawing To Buffer Readiness

Authority drawing paths should reduce to one readiness model:

- `PresentPixmap`: preferred explicit handoff; ready when the presented pixmap
  and damage region are known.
- DRI3 DMA-BUF: ready when the buffer handle, dimensions, format, and fence
  state are importable by the renderer boundary.
- SHM/software updates: ready when the affected software buffer range is copied
  or otherwise made immutable for the transaction.
- Render/core drawing: ready when drawing commands have updated an authority
  owned backing buffer and damage is bounded.
- XSync resize: compatibility signal only; use it to avoid presenting a resized
  state until matching pixels exist, not as the native transaction model.

The authority should prefer explicit ready buffers over XComposite-style
mirroring. XComposite/Damage remains prototype evidence, not the long-term
surface boundary.

## Portals And Selections

The normative broker and grant lifecycle is in
[namespaces-and-portals.md](namespaces-and-portals.md). The native X frontend
owns namespace-keyed selection state. The session broker, bounded
policy-provider IPC, expiry/revocation lifecycle, and first concrete
cross-namespace `CLIPBOARD`/`PRIMARY` source-proxy executor are complete for
`TARGETS`, `UTF8_STRING`, and bounded UTF-8 `text/plain`. Other portal kinds
still require their own evidence-driven executors.

Selections, clipboard, drag-and-drop, URI open, notifications, screenshots, and
file handoff are protocol-specific inputs to Sophia Portals.

The authority owns X requestor context:

- requestor XID;
- selection atom;
- target atom;
- property atom;
- timestamp;
- source and target namespace attribution.

Portals receive reduced transfer facts:

- source namespace;
- target namespace;
- transfer kind;
- MIME or target name;
- byte count or bounded placeholder;
- generation token.

Portal approval is generation-bound and single-use. Denial becomes native X
failure, such as `SelectionNotify` with `property = None`, not synthetic input
or client freezing.

## Lifecycle And Chrome

The frontend translates compositor lifecycle commands into normal X
semantics:

- polite close uses `WM_DELETE_WINDOW` when advertised;
- force close may become X client termination only after Engine/session policy
  decides the polite path failed;
- map/unmap/destroy events become reduced lifecycle facts for Engine and WM
  relayout;
- raw titles, classes, icons, PIDs, and paths remain **authority-private**. They
  are never WM layout inputs, and they do not leave this process — not even to the
  metadata broker.

What leaves is a reduced label. The broker publishes a `MetadataDisclosureRule` per
surface, this authority applies it to text it already legitimately holds, and only
the result crosses. A `ClassOnly` rule reduces the complete retained window identity
to `WM_CLASS` even when a later title property changes; `Full` prefers
`_NET_WM_NAME`, then `WM_NAME`, then the class. The emitted
`ReducedMetadataCandidate` carries only the bounded display label (if one is
permitted), disclosure level, opaque surface, and private monotonic generation.
Property names, types, bytes, XIDs, and namespace identity do not cross this route.

The reason is least privilege. Concentrating raw identity in the broker would be
convenient — one sanitizer, one policy — but it would move every client's title,
class, and PID across a process boundary and make one component the place where all
of it lives, across every authority that will ever exist. Reduction belongs where
the data already is; policy belongs where it can be consistent. Those are different
places.

The broker still owns what only it can: disclosure policy, trust assignment, icon
tokens, and aggregation across authorities. It emits sanitized `ChromeDescriptor`
data. Sophia Engine owns the chrome presentation and chrome hit-testing.

Production Hagia sessions host this loop over `sophia_broker_v1`. The session admits
the surface to its protected metadata-broker process, routes the returned rule to
the exact X frontend client, and commits only the broker's sanitized descriptor to
Engine's `ChromeDescriptorTable`. Removal retires the same surface on both sides.
The current production default is `ClassOnly`; descriptor rendering by a native
shell remains a later milestone.

## Input Delivery

Sophia Engine reads physical input, owns the scene graph, performs spatial
hit-testing, and sends routed input intent to the frontend:

- target Sophia `SurfaceId`;
- authority-local object ID when known;
- local coordinates after inverse transform;
- seat, device, time, and event kind.

The frontend then applies X semantics:

- focus and grabs;
- event masks;
- XI/XKB delivery rules;
- namespace checks;
- sync-frozen device state.

The authority returns reduced accept/reject outcomes. It must not expose raw
client streams or general event injection capability back to Sophia Engine.

For the persistent X11 session, each Engine-addressed input event also carries
an opaque delivery token. The owning client worker acknowledges that token only
after it has serialized and flushed the corresponding X11 event; route rejection
and socket-write failure are distinct reduced outcomes. The live proof waits for
every final keyboard token before it starts its bounded redraw/presentation
window. Tokens contain no XID, keycode, text, or device identity.

The persistent xterm proof waits until the focused surface's own CPU buffer has
visual detail instead of treating its initial uniform background fill as
application readiness. Another client drawing cannot satisfy this gate. This
prevents synthetic proof keys from racing the proof shell's first prompt.

## Remaining Production Gaps

The frontend is sufficient for paired xterm sessions, mixed Vulkan presentation,
and deterministic GTK3 Zenity behavior. Retained classic and confined QEMU profiles now require a committed 640x360 resize with changed CPU/SHM pixels in addition to exact input, native presentation, normal exit, and clean teardown. Its remaining production gaps are
explicit and evidence-driven:

- GTK3 promotion requires classic-shared and confined QEMU sessions proving
  virtio text, a presented cursor, pointer selection, resize redraw, normal
  exit, and clean native teardown.
- XFixes selection notifications and complete client-selected cursor-image
  presentation are not yet general contracts. Existing selection-input, region,
  and cursor request handling covers only retained probes.
- Large selection transfers through `INCR` and full Xdnd execution remain
  deferred behind the bounded clipboard reference flow.
- Render is not advertised. Sync advertises only version negotiation and the
  DRI3-fence teardown used by Mesa. GLX advertises only the captured
  direct-Mesa bootstrap needed by current clients: GLVND vendor selection,
  deterministic depth-capable visuals/FBConfigs, legacy and modern direct
  context lifecycle, direct MakeCurrent, GLX windows, and drawable attributes.
  Indirect rendering and server-side GLX buffer submission remain deferred.
- RandR provides Engine-derived observation and notifications, not general
  client-controlled mode management. MIT-SHM intentionally copies admitted
  image ranges and does not expose shared pixmaps.
- A general local listener still needs independently credentialed confined
  groups; the current classic and explicitly launched confined profiles remain
  the supported admission shapes.

Before expanding those surfaces, the protocol-neutral `runtime_driver` owns visual phase order and production X uses one backend service poll; the remaining promotion gate is the unattended paired QEMU GTK acceptance run. Machine-specific hardware runs remain optional compatibility diagnostics. The frontend contract does not change: it emits
bounded observed batches and consumes routed input, control, topology, and
presentation feedback without owning scene, renderer, or KMS state.

## Phoenix Strategic Direction

Sophia adopts Phoenix’s strategic direction, not its code: build a modern X
server from a clean implementation, deliberately support the X11 features real
applications use, and improve the server beneath the established X11 API. The
useful study areas are:

- connection setup and request dispatch shape;
- minimal resource tables;
- basic window/pixmap/property behavior;
- extension prioritization based on real toolkit compatibility;
- tests or examples that prove GTK/GL/Vulkan application paths.

Sophia-specific differences must remain intact: Engine-owned atomic visual
commits, blind WM policy, and a user-selectable choice between classic shared-X
semantics and confined namespace/capability policy.

## Historical v0 Internal Socket Runtime

The first executable authority seam is an internal Sophia frame protocol over a
Unix socket. It is not the X11 wire protocol. It exists to prove that the X
Authority can run across a process boundary while preserving the same reducer
behavior already covered by tests.

The v0 socket path uses the shared 24-byte Sophia IPC header and two message
kinds: `XAuthorityRequest` and `XAuthorityResponse`. Payloads are decoded with
explicit little-endian parsing, bounded counts, and bounded text. No generic
serializer is used.

The internal request surface covers only the reducer-backed behaviors that exist
today:

- create and map a window;
- present a pixmap as a ready `SurfaceTransaction`;
- set a selection owner;
- convert a selection request into a portal prompt or native failure artifact.

The CLI smoke command is:

```sh
cargo run --offline -q -p sophia-cli -- x-authority-runtime-smoke
```

Real X11 connection setup and first core request parsing now sit beside this
internal socket seam. The wire parser translates X11 bytes into existing
internal request packets rather than bypassing the authority reducers.

## X11 Wire Start

The first X11 wire milestone is connection setup, not application
compatibility. The frontend now has a bounded setup parser for byte-order
markers, protocol version fields, authorization name/data fields, and resource
ID allocation facts. It also has setup success/failure encoders and a local
Unix socket smoke that completes a setup handshake with a synthetic client.

The covered setup fixtures are:

- little-endian and big-endian setup requests;
- valid setup with resource ID base and mask;
- truncated setup input;
- unsupported major protocol version;
- overlarge authorization fields;
- setup success and setup failure reply encoding.

Core request parsing now starts with fixtures for `CreateWindow`, `MapWindow`,
`InternAtom`, `GetAtomName`, `ChangeProperty`, `GetProperty`,
`SetSelectionOwner`, `ConvertSelection`, `CreateGC`, `FreeGC`,
`GetInputFocus`, `QueryExtension`, `ListExtensions`, and `QueryBestSize`.
Runtime-backed wire requests translate into existing internal
`XAuthorityRequestPacket` values before they reach `XAuthorityRuntime`;
property writes and reads land in a minimal namespace-keyed property table.

Minimal client-visible output now covers bounded X error records and selected
core lifecycle events. `CreateWindow` emits only `CreateNotify` to clients that
selected `SubstructureNotify` on the parent; it never fabricates a
`ConfigureNotify`. A real geometry change emits `ConfigureNotify` to
`StructureNotify` selectors on the window and `SubstructureNotify` selectors
on its parent. Map, visibility, and exposure events follow their corresponding
per-client masks. Variable-length replies cover `InternAtom` and
`GetAtomName`/`GetProperty`.

The X11 socket smoke completes setup, interns atoms, creates a window without
leaving a lifecycle event queued for its owner, selects the desired masks,
maps it, writes a title property, reads it back, and observes only the selected
events. A two-client regression separately proves parent-selected
`CreateNotify`, direct and parent `ConfigureNotify`, and direct and parent
`MapNotify` delivery.

Atom naming is authority-owned and bounded. Sophia preloads the small predefined
set needed by the prototype, preloads the X11 predefined atom range, allocates
dynamic client-interned atoms after that range, and caps atom names at 256
bytes. Metadata-relevant
property writes such as `WM_CLASS`, `WM_NAME`, `_NET_WM_NAME`, and
`WM_PROTOCOLS` produce metadata broker candidates that include only namespace,
window, atom names, type names, value length, and generation. They do not emit
raw titles, classes, icons, paths, or namespace labels to the window manager.

Minimal `GetProperty` is now present. The first real-client-library smoke uses
`x11rb` against the Sophia X Server Frontend socket. That path requires a
client-compatible setup reply with one root, one pixmap format, one depth, and
one TrueColor visual. The smoke connects through the normal X11 setup path,
interns `_NET_WM_NAME` and `UTF8_STRING`, creates a window, writes and reads a
bounded title property, maps the window, and observes the selected map
lifecycle.

Subsequent external probes remain compatibility drivers. Their first failure
should drive the next bounded opcode or reply implementation rather than
guessing ahead.

`xdpyinfo` now passes as the first broader probe. It forced a minimal root
screen in setup, empty extension discovery replies, root property reads for
standard predefined atoms, root input-focus reporting, and no-reply GC lifecycle
requests.

A tiny C Xlib client now also passes. The CLI smoke compiles the probe into
`/tmp`, connects through libX11, interns atoms, creates a simple window, writes
and reads the title through normal Xlib property calls, maps the window, and
destroys it cleanly. That probe added the first minimal `DestroyWindow`
compatibility path.

A drawing-oriented C Xlib client now passes as well. It creates a window,
creates a GC, maps the window, calls `XFillRectangle`, syncs, frees the GC, and
destroys the window. This added `PolyFillRectangle` decode support. Successful
fill requests produce no client-visible X reply, but they do emit a ready
`CoreDraw` surface transaction with rectangle damage in the dispatch path.

The live X11 socket path exposes those dispatch results through an out-of-band
observer callback. This keeps the client-visible X11 stream pure: successful
core drawing still produces no direct X reply, while Sophia Runtime can receive
the ready `SurfaceTransaction` facts through the session side channel. The
compiled Xlib drawing smoke now validates the whole reduced path: `XFillRectangle`
produces one observed transaction, Sophia Engine commits it, and the live
runtime adapter records one authority transaction without exposing XIDs or
namespace metadata.

A software image upload smoke now passes through the same path. The authority
decodes bounded core `PutImage` requests, records the uploaded image extent as
damage, emits a ready CPU-backed `SurfaceTransaction`, and still sends no direct
X11 reply on success. The compiled Xlib `XPutImage` smoke validates that the
observed transaction commits in Sophia Engine and increments Sophia Runtime's
authority transaction counters.

Core image validation owns format, depth, left padding, scanline padding,
payload length, and the connection's advertised image order. It lowers valid
packed or planar pixels into tight little-endian storage before mutation;
little-endian 32-bit ZPixmap uploads borrow their payload without conversion.
The software writer applies GC clipping, raster function, and plane mask, with
a row-copy fast path for unrestricted GXcopy. XYBitmap selects GC foreground
and background. Malformed or unreadable data never manufactures background
pixels. `SOPHIA_X11_PIXEL_TRACE=1` enables bounded RGB counts/checksums at upload
and CPU Present boundaries; normal sessions do not scan pixels for diagnostics.

Core GC and RENDER picture clips distinguish an unrestricted clip from an
explicitly empty region, which suppresses every pixel. Rectangle clips retain
their signed origins; `ChangeGC(clip-mask=None)` and
`ChangePicture(clip-mask=None)` restore unrestricted drawing. Omitted attributes
preserve the previous clip. Invalid or unsupported picture attributes are
validated before mutation. Nonzero pixmap clip masks remain explicitly
unsupported. XFIXES region constructors copy the stored rectangles without
folding in their origins, including an explicitly empty region. Raster replay
preserves that distinction and cannot turn a clipped upload into an
unrestricted one. The optional [GTK redraw probe](../tools/probes/README.md)
checks dialog and menu pixels and dialog reuse with a real GTK3 client.

A private `SOPHIA-PRESENT` extension remains as historical prototype evidence
for the first explicit buffer-handoff reducer. It is not the forward path and
must not be extended or used for application promotion. The CLI
present-pixmap smoke retains only its bounded regression value.

Standard DRI3 1.3 and Present are the active path. The socket boundary uses
`recvmsg`/`sendmsg` to carry bounded SCM_RIGHTS records in both directions,
queues ancillary FDs in Unix-stream order across requests that consume none,
and drains exactly the declared arity for each FD-bearing request. DRI3 `Open`
gets one independently opened same-GPU render-node FD from a backend provider
without storing a device path in Engine or the authority runtime. `PixmapFromBuffer`,
modifier-bearing `PixmapFromBuffers`, `FenceFromFD`, supported-modifier queries,
Present `Pixmap`/`SelectInput`/`QueryCapabilities`, and the bounded XFIXES region
lifecycle required by Mesa are implemented.

The backend revalidates the retained device and selected render node around
each DRI3 open using filesystem device, inode, device number and physical sysfs
identity. A replaced node or ambiguous sibling is a named refusal, never an
ordinal fallback. Each authenticated connection pins one immutable device bundle:
provider, measured import capabilities and pixmap allocator. Installing a later
bundle affects new connections only. Device loss refuses new opens and
allocations through the unavailable bundle; it never redirects an existing
connection. At most sixteen retained bundle generations may coexist. Rebuilding
renderer workers alone does not change any frontend connection's contract.

`GetSupportedModifiers` returns the connection's bounded, immutable snapshot of
explicit format/modifier pairs. The live bundle measures the same device used
for DRI3 Open and server allocation. Queries perform no GPU discovery. An
unavailable or unmeasured format answers an empty list; neither linear support
nor an implicit modifier is invented. External-texture-only formats are excluded
because the native renderer samples ordinary 2D textures. Equal tiled modifier
numbers do not establish cross-device compatibility: renderer import still
validates each submitted descriptor and may transfer through an admitted source
device. Import capabilities are not allocation or scanout guarantees.

DRI3 QueryVersion clamps to the client's requested version. SetDRMDeviceInUse
is an advisory device number scoped to the exact window incarnation, with no
path lookup or authority grant. Window preferences may change with an exact
surface and output-topology generation, independently of screen modifiers;
the returned preferences are restricted to the client's immutable import set.
Each fourcc has its own measured plane modifiers. XR24 and AR24 preferences
are derived independently from the strict cached plane snapshot; unknown or
empty rows offer no preference. Renderer target allocation retains its separate
admission policy. Plane support does not establish that a complete frame can flip.
Tiled preferences also require the available connection bundle and the output's
retained render node to have equal server-observed filesystem device, inode and
device number. These facts are captured outside authority locks after backend
card-to-render-node resolution. The backend brackets that resolution with held
FD/current-node identity checks for both card and render nodes. Missing identity,
node replacement or a different device restricts a healthy connection's
preferences to measured LINEAR; it never changes screen formats. An unavailable
pinned bundle offers no window preferences, including LINEAR. Queries resolve
the requesting connection's pinned formats directly; a caller cannot substitute
another device's screen list.
A client hint can further restrict preferences but cannot establish device
identity. Modifier queries perform no device I/O. Unknown device hints yield no
matching window preference. Present SuboptimalCopy
is not emitted until exact alternate-layout flip evidence exists; failed atomic
validation or IN_FORMATS membership alone is not that evidence.

An optional retired-layout comparison is attached to the exact copied Complete,
never passed as a reusable permit. Successful DMA Present dispatch captures the
requested window incarnation, presenting connection and original buffer/layout.
Later FreePixmap and XID reuse cannot substitute another subject. The session
first matches current native context and placement to acknowledged preferences;
the frontend then checks its current viewable window, geometry, accepted snapshot
generation, available connection bundle and exact-format effective membership.
Child presentations are not assigned a parent's allocation preference.

Comparison and Complete consumption share the authority-to-pending lock order.
Authority acquisition is nonblocking: contention or unavailable state rejects
only the comparison. Locks are released before protocol egress. A match records
an observation without changing Copy/Flip/Skip, completion clocks or Idle ordering.
The context crossing this boundary contains an opaque generation and logical
output only; native head and target identity remain in the backend/session.

Descriptor validation bounds dimensions, logical image bytes, plane count,
offsets and pitches. Explicit non-linear modifiers may carry opaque auxiliary
planes whose pitch and extent do not follow the RGB image dimensions. Those
planes are checked against their received FD lengths before X resource creation;
the native importer validates the layout itself. Linear and legacy implicit
descriptors retain packed-row checks. A metadata plane must never be rejected
solely because its pitch is smaller than an image row.

Legacy `BufferFromPixmap` exports require one plane, offset zero and a stride
representable by the protocol's 16-bit field. A nonzero offset or oversized
stride is refused with `BadPixmap`, rather than silently changing the layout.
Modifier-bearing `BuffersFromPixmap` preserves those layouts exactly.

For compatibility with Chromium's VA-exported images, a zero `PixmapFromBuffer`
size is resolved from the received FD's kernel-reported length before pure
request dispatch. This is a frontend compatibility rule, not permission to omit
buffer bounds. If that length is unavailable, zero or unrepresentable, normal
validation rejects the request. Explicit nonzero sizes remain unchanged;
descriptor, geometry, stride and namespace checks still apply. Measuring the FD
does not map its pixels or change its file offset, and renderer import remains
responsible for accepting the native buffer.

The [pixmap texture export contract](pixmap-texture-exports.md) defines direct
GLX pixmap capability, retained backing identity, CPU update ordering and the
renderer-provider boundary. These offscreen resources do not carry WM policy
or acquire scanout authority.

An immediately satisfied Present `NotifyMSC` is serialized with the requesting
connection's output and carries that request's sequence. A later reply cannot
overtake it. Other subscribers receive asynchronous events stamped with their
own connection sequences. This ordering guarantee does not establish complete
future-target MSC scheduling or divisor/remainder support.

The Mesa RADV `x-authority-vkcube-smoke` reaches one accepted standard Present
transaction and one committed runtime surface with `first_error=none`; it
remains the transport-only proof. `LiveDmaBufPresentationRegistry` owns reusable
source and per-Present FD lifetimes. The persistent session now connects it to
mixed rendering and exact page-flip retirement, while
`tools/live_session_milestone4_hardware_proof.sh` is the stricter GPU-to-KMS
promotion gate. The retained X13 run completes repeated CPU-plus-Vulkan mixed
exports and page flips, one controlled Skip/recovery, matching Complete/Idle
and idle-fence activity, and exact cleanup. Imported-image GL context state is
retired at the mixed-export boundary while the GBM scanout owner remains alive
through KMS retirement.

## Runtime Transport

The long-running X Server Frontend path uses a bounded side channel for observed
surface transactions and buffer-lifetime facts. Successful X11 drawing and
present requests still write no client-visible success reply when the X11
protocol does not require one. Instead the authority packages ready
`SurfaceTransaction` values, CPU updates, DMA-BUF/fence registrations, Present
submissions, and removals into `XAuthorityObservedTransactionBatch` records and
sends them through the runtime-owned bounded queue.

The live backend converts each observation into a bounded production envelope
whose ordered atomic groups preserve their originating `TransactionId`.
Deferred admission can release an older group beside a new observation without
combining their identities. DMA-BUF and fence registrations stay
envelope-scoped because import can precede consumption; transactions,
removals, and Present submissions stay group-scoped and fail validation if
their IDs disagree.

Backpressure is explicit and lossless. If the production queue is full, the
authority retains exactly the current bounded batch and pauses that client's X11
request dispatch until Runtime accepts it. It neither allocates an unbounded
spill queue nor disconnects an ordinary client for producing a finite burst.
Each connection preserves its own request order; the concurrent frontend does
not impose a second global order on batches from independent clients.
Shutdown cancellation releases a paused worker, while an unexpectedly missing
receiver remains a `Disconnected` transport failure. The nonblocking
`try_emit_x_authority_observation` helper retains its fail-fast `Backpressure`
result for bounded probes and callers that deliberately select that policy.

The reciprocal routed-worker transport is also bounded.
`XServerFrontendRouteBroker` owns input and control ingress plus a registration
table for live routed workers. `serve_next_concurrently_routed[_traced]`
registers a connection only after successful X11 setup, gives it one private
input queue and one private control queue, and unregisters those queues before
the release cleanup is reported. The Engine-side loop calls `route_pending`;
it never broadcasts an event. Unknown or disconnected input is retired, while
a full private input queue removes that client's sender set and rejects the
tracked event without returning a service-fatal error. The disconnected worker
then exits through its ordinary teardown path. Shared registry corruption and
shared acknowledgement-channel pressure remain errors. Control
acknowledgements return through the same broker labeled with the worker client
identity. This is an in-process contract that now carries the persistent live
service. The service polls the owner-only listener without blocking the Engine
path, admits at most the configured worker count, and receives `StopAccepting`
from session supervision. That command never kills a client: it closes only new
admission and drains workers whose client streams have ended.

The callback observer helpers remain for focused tests and smoke probes. They
are not the production transport shape.

## MIT-SHM Negotiation

The frontend advertises a bounded `MIT-SHM` software-image surface. It performs
an immutable copy for each admitted update rather than retaining a client
shared-memory mapping.

`QueryExtension("MIT-SHM")` returns its assigned major opcode, and minor opcode
`0` replies to `ShmQueryVersion` with protocol version `1.2`, `shared_pixmaps =
false`. Unsupported minor opcodes fail closed as native X request errors.

`ShmAttach` records namespace-local segment metadata: the synthetic segment
XID, client-provided `shmid`, read-only bit, and generation. For an admitted
`XShmPutImage`, a narrow SysV SHM adapter validates segment size, attaches
read-only, copies only the bounded image range into a new immutable CPU-buffer
generation, and detaches immediately. The authority and Engine do not retain a
client mapping.

A missing, malformed, or cross-namespace segment returns a bounded native X11
error. Pixmap targets remain local resource activity without a surface
transaction. Invalid or already-gone detach requests are cleanup no-ops, while
cross-namespace detach is rejected. When notification was requested, an
accepted window update produces the standard MIT-SHM Completion event; a
rejected update does not.

The GTK Zenity probe retains nonzero software pixels with `first_error=none`.
Core `PutImage` and MIT-SHM therefore form the software baseline. Standard
DRI3/Present is the GPU handoff path; the private `SOPHIA-PRESENT` prototype is
not a production alternative.

## External xclock Probe

`x-authority-xclock-smoke` launches `xclock` against a temporary Sophia
X Server Frontend socket and treats the client as the compatibility driver. The probe
added only the request surface xclock actually exercised: printable atom names,
pixmap resources, copy-area flow, basic font replies, list-font replies,
window-attribute and subwindow mapping no-ops, expose events, and bounded core
draw transactions for line, segment, polygon, rectangle, image, and copy damage.

The passing proof reached mapped exposure and seven Engine/Runtime committed
authority transactions with no X protocol error before the harness killed the
long-running xclock process. Its reduced report now includes the explicit
`outcome=proof_window_killed`, total request count, unique major-opcode count,
and sorted major-opcode list, so future regressions show which compatibility
surface changed without exposing XIDs or namespace IDs. The authority still does
not become a full X server: unsupported requests remain fail-closed, and only
reduced transaction facts cross into runtime.

## External xeyes Probe

`x-authority-xeyes-smoke` launches `xeyes` against a temporary Sophia X
Authority socket and keeps authority coverage probe-driven. The probe added the
request surface xeyes actually exercised after xclock: bounded `QueryColors`,
`ClearArea`, and `PolyFillArc` handling. Arc and clear operations reduce to core
draw damage transactions; color replies return bounded RGB records without
making the frontend a full colormap implementation.

The passing proof reached five Engine/Runtime committed authority transactions
with no X protocol error before the harness killed the long-running xeyes
process. The reduced report includes the total request count, sorted opcode
list, runtime counters, and `first_error=none` so future real-client regressions
identify the next compatibility gap directly.

## External xwininfo Probe

`x-authority-xwininfo-root-smoke` launches `xwininfo -root` against a
temporary Sophia X Server Frontend socket and treats root-window introspection as a
separate non-drawing compatibility surface. The probe added only the request
surface xwininfo actually exercised: bounded `GetWindowAttributes`,
`GetGeometry`, `QueryTree`, and `TranslateCoordinates` replies.

The passing proof exits successfully with no Engine transactions because the
client does not draw. Its reduced report still records request count, opcode
set, zero runtime counters, and `first_error=none`, so introspection regressions
remain visible without requiring visual transaction evidence.

## External xprop Probe

`x-authority-xprop-root-smoke` launches `xprop -root` against a
temporary Sophia X Server Frontend socket and treats root property discovery as a
read-only compatibility surface. The probe added only the request surface xprop
actually exercised after xwininfo: bounded `ListProperties` decoding and replies
for namespace-local property atom sets.

The passing proof exits successfully with no Engine transactions because the
client only introspects root properties. The reduced report records request
count, opcode set, zero runtime counters, and `first_error=none`. Sophia X
Authority does not synthesize a broad global root-property catalog here; it
reports the properties the namespace-local table actually owns.

## External xsetroot And xlogo Probes

`x-authority-xsetroot-name-smoke` launches `xsetroot -name "Sophia
Root"` against a temporary Sophia X Server Frontend socket. It exits successfully
through existing property, input-focus, GC lifecycle, and extension-query paths,
proving a small root-property mutation case without Engine transactions.

`x-authority-xlogo-smoke` launches `xlogo` and reaches committed
Engine/Runtime authority transactions through the existing create/map/property
and polygon/rectangle drawing surface. It did not require new X protocol
coverage, which makes it a useful regression probe for the bounded drawing
paths already admitted by xclock and xeyes.

## External xmessage Probe

`x-authority-xmessage-smoke` launches `xmessage Sophia` and treats
legacy text UI behavior as the next compatibility driver. The probe added only
the request surface xmessage actually exercised after xlogo: bounded
`CreateGlyphCursor`, `FreeCursor`, `SetClipRectangles`, and `PolyText8`.

Cursor support is currently resource lifecycle only. The frontend accepts
the font-backed cursor resource so legacy clients can proceed, but compositor
cursor presentation remains future Engine/session policy work. `PolyText8`
retains text items, signed deltas, and request-scoped font shifts, rasterizes
the canonical 6x13 glyphs into both window and pixmap backing, and reports
`BadFont` for invalid shifts after preserving any valid prefix work.

`ImageText8` uses the GC's retained font face but forces the core protocol's
GXcopy/Solid foreground-and-background semantics. A GC remains usable after
its source font XID closes, and `QueryFont` accepts either a font or GC
FONTABLE. `CopyArea` snapshots overlapping source pixels, clips source and
destination in one request-coordinate space, and supports pixmap/window
combinations. GraphicsExpose/NoExpose delivery remains outside this bounded
text-and-scroll slice.

Core fonts come from a session-configured font path, searched in order, with
the built-in element last. The built-in element carries the 6x13 Latin-1 bitmap
under the fixed face aliases (`fixed`, `6x13`, and its canonical XLFD) plus the
lifecycle-only `cursor` and `nil2` compatibility names opened by real xterm,
and it is always present: a session with no directories configured still
renders text, and the pixel proofs have a face that host packages cannot
change under them. XLibre embeds `fixed` and `cursor` the same way and for the
same reason.

A host path is what lets a terminal in UTF-8 mode render the repertoire it
asks for. The built-in face is indexed by one byte and reports itself that way,
so a client told to expect a single-byte font draws with the 8-bit requests;
the real `iso10646-1` 6x13 is a two-byte matrix of some four thousand glyphs,
and only a font path can supply it.

Four safeguards make exposing that path defensible, and each is tested by what
it refuses:

1. **The path is session configuration.** `SetFontPath` is decoded and answered
   `BadAccess`, so no client request can add a directory to search. It is
   answered rather than left undecoded because a client that meets `BadRequest`
   may exit -- xterm installs an error handler that does -- so a client must be
   able to ask and be told no.
2. **A client's string never becomes a path.** A name is matched against an
   index built from each directory's `fonts.dir` and `fonts.alias`, and the
   file is the one the *directory* published. A published entry naming anything
   outside its own directory is dropped rather than obeyed.
3. **Reads are bounded and follow no links.** Index and font files are opened
   `O_NOFOLLOW`, must be regular files, and are size-bounded before parsing.
   The PCF reader indexes nothing without a checked accessor; a truncated or
   hostile file yields no font rather than a panic.
4. **Loaded faces are bounded.** The cache holds a fixed number of faces and a
   fixed number of bytes, evicting least recently used, so a client opening
   many fonts costs a ceiling rather than growing memory. A face whose declared
   matrix exceeds the protocol's own two-byte maximum is refused at load.

The path is indexed once, when the frontend is constructed. A directory
changing underneath a running session cannot change what a client resolves.
`--font-path` overrides it; `--font-path=` selects none, which is what a proof
that must not depend on installed packages asks for.

The external real-client harness now treats any observed X protocol error as a
smoke failure even if the client already produced authority transactions. This
keeps `first_error=none` as an enforced compatibility invariant. Its fixed-text
scroll proof folds the latest live transaction independently for each surface,
requires four adjacent final rows, and joins accepted same-surface CPU updates
in ImageText8→CopyArea→ImageText8 order. A stale/offscreen snapshot, no-op copy,
auxiliary window, or unordered opcode presence cannot pass.

## External xrandr Probe

`x-authority-xrandr-query-smoke` launches `xrandr --query` against a
temporary Sophia X Server Frontend socket and treats output-size discovery as a
read-only compatibility surface. The probe added only the request surface xrandr
actually exercised: minimal `RANDR` extension advertisement,
`RRGetScreenSizeRange`, and `RRGetScreenResources`.

RandR size range and screen resources sample a validated, bounded Engine
topology generation. Replies contain synthetic protocol-local CRTC, output,
mode, and name IDs plus output and CRTC detail; those IDs do not claim ownership
of native connectors or KMS objects. Accepted live topology updates emit the
selected screen, CRTC, output, and resource notifications. The paired
Milestone 3 session retains an authenticated witness for those events and
configure-plus-pixels resize evidence.

The external probe trace now includes bounded parse-error request heads. That
keeps future extension work probe-driven when Xlib labels an extension failure
imprecisely in client stderr.

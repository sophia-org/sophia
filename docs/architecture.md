# Architecture

**Role:** normative system architecture.

This document defines Sophia's authority boundaries and intended process
relationships. [Data-Oriented Design](dod.md) defines the records that cross
those boundaries. [Namespaces and Portals](namespaces-and-portals.md) defines
the trust and transfer model. [Engine Architecture](engine-architecture.md)
defines the domains inside Engine and the precise scope of its compositor
role. [Compositor Graphics](compositor-graphics.md) defines the
renderer-neutral compositor-owned display list and its native lowering.
[Multi-Monitor Per-Head Composition](multi-monitor-composition.md) defines how
logical output scenes fan out into distinct physical-head render targets without
exposing head identity to policy.
[State and Transition Discipline](state-and-transition-discipline.md) explains
the transition-system, I/O-automaton, single-writer, and CALM foundations behind
these contracts without adding another normative layer.
Subsystem documents record implementation detail and current coverage.

Sophia is an X-centric product with an Engine-centered architecture. Sophia
Engine owns physical input, the scene graph, atomic visual commits, rendering,
and scanout. The active X Server Frontend terminates X11 and translates it into
namespace-checked Sophia facts. The WM proposes blind layout policy. Portals
mediate deliberate namespace crossings. Engine boundaries remain
protocol-neutral so future translators can be evaluated without redesigning
visual authority.

The accepted [9P public-interface direction](sophia-9p-control-bus.md) targets
9P2000.L for replaceable desktop roles and a new application frontend alongside
X authority. This is a target, not current support: the existing native role
protocols remain authoritative during migration. Shared transport does not merge
authorities, replace namespaces, or move Engine's internal execution onto 9P.
The [decision record](notes/decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
distinguishes accepted direction from the open filesystem contract.

## System Shape

```text
================================================================================
                         HARDWARE AND KERNEL
================================================================================
 [ physical input devices ]                                  [ display output ]
            │                                                        ▲
            │ libinput                                               │ DRM/KMS
            ▼                                                        │

================================================================================
                    SOPHIA ENGINE: VISUAL AUTHORITY
================================================================================
 ┌────────────────────────────────────────────────────────────────────────────┐
 │ Scene graph | hit-testing | atomic commits | rendering | frame scheduling  │
 └───────────────┬────────────────────┬───────────────────────┬────────────────┘
                 │ opaque policy data │ reduced portal facts  │ sanitized chrome
                 ▼                    ▼                       ▼
        ┌────────────────┐   ┌─────────────────┐   ┌──────────────────────────┐
        │   SOPHIA WM    │   │  PORTAL BROKER  │   │ METADATA BROKER / SHELL │
        │ blind policy   │   │ policy + grants │   │ redacted presentation   │
        └───────┬────────┘   └────────┬────────┘   └──────────────────────────┘
                │ layout proposals    │ executor commands
                ▼                     ▼

================================================================================
                         PROTOCOL FRONTEND
================================================================================
 ┌────────────────────────────────────────────────────────────────────────────┐
 │ Sophia X Server Frontend: X11 resources, selections, grabs, protocol state │
 └───────────────────────────────┬────────────────────────────────────────────┘
                                 │ namespace-checked transactions,
                                 │ lifecycle, routed input, configure
                                 ▲

================================================================================
                         ADMITTED CLIENT NAMESPACES
================================================================================
 ┌────────────────────────────────────┐     ┌─────────────────────────────────┐
 │ Namespace A: trusted/shared        │  X  │ Namespace B: confined           │
 │ X terminal | trusted local tools   │     │ browser | chat | untrusted app  │
 └────────────────────────────────────┘     └─────────────────────────────────┘
```

The session supervisor surrounds these components. It owns process lifetime,
namespace allocation, client admission, authorization material, broker health,
and recovery. It does not absorb protocol semantics, portal policy, rendering,
or layout policy.

In the Rust implementation, `sophia-session` owns that production lifecycle
and the adapters around Engine. It is a passive library: exact evidence crosses
a host-installed line-output boundary, and `sophia-cli` owns command selection
and concrete stdout/stderr presentation. `xtask` and `sophia-conformance` are
development-only and cannot enter the installed dependency graph. The complete
repository-tooling direction is defined in
[Development Tooling](development-tooling.md).

## Shell Rendering And Execution Permission

The shell presentation contract is renderer-neutral: a content client proposes
immutable images and their matching interaction snapshot; Engine validates,
composites and retires the complete candidate. Vello, Xilem, GTK and other
toolkits are downstream choices, not Sophia dependencies or public wire types.
The shell's own pixels are distinct from foreign scene pixels, which remain
accessible only through the existing Engine/portal authorities.

GPU execution is an independent, default-denied startup permission. The accepted
first Lom path grants one selected render node explicitly, keeps the existing
immutable CPU-byte handoff after GPU readback, and requires no custom Linux
kernel or `dmem` controller. That permission accepts GPU-driver and availability
risk; it does not promise a portable hard aggregate VRAM quota. Sophia must
enforce its own bounded content queues, backing charges and retirement leases.
No display connection, KMS, general input or additional shell authority follows
from device access.

The [presentation/execution decision](notes/decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
supersedes the earlier mandatory GPU-quota design. A GPU bridge is optional
future execution work, and a different image transport needs measurements and
its own admission. The current production gate still fails closed; the
[paired Lom/Sophia critical path](notes/plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md)
names the launch, input, recovery and acceptance work remaining.

## Current And Target State

### Implemented

- The native Sophia X Server Frontend accepts bounded concurrent local X11
  clients, owns X resource tables, emits `SurfaceTransaction` batches, routes
  client-targeted input/control, and renders two real xterms through Engine and
  KMS. The retained paired Milestone 3 proof passes under both classic-shared
  and fresh zero-capability confined profiles with physical keyboard and
  pointer input, authenticated RandR delivery, configure-plus-pixels resize,
  and clean teardown. Its X13 runs report 94/90 ms startup readiness and 13 ms
  maximum composition.
- X resources and selection state are namespace-keyed. Protocol values and a
  runtime registry model immutable profiles, directional portal capabilities,
  admission provenance, and revocation. After X setup authentication, the
  frontend asks session policy for an immutable context before allocating X
  client/resource identity. The live classic session admits same-UID Unix peers
  into distinct registry admissions that deliberately share its namespace,
  then revokes each admission after connection cleanup. The supervisor creates
  a fresh owner-only Xauthority file and cookie for every live session, passes
  only its path to clients, and removes it on teardown. Denial uses native X11
  setup failure. `sophia session run` explicitly selects classic-shared or a
  confined group with zero ambient portal capabilities. A socket regression
  assigns simultaneous clients distinct confined namespaces and proves a
  cross-namespace window map, property mutation, and selection ownership return
  `BadAccess`; selection conversion fails normally and rejected property writes
  emit no metadata candidate. Foreign event-mask selection also returns
  `BadAccess`, cannot change the receiving worker's authority-local key target,
  and brokered input reaches only the addressed client's private queue. Classic
  same-namespace existing-resource behavior remains available. Session
  supervision can revoke one admission by opaque `ClientAdmissionId`; socket
  shutdown then runs the worker's ordinary route, resource, surface, and lease
  cleanup without terminating its peers.
- Live native-X setup and populated RandR resource replies now derive their
  bounded root/output/mode facts from an Engine output-topology snapshot. Engine
  hit-testing also crosses the boundary as a `RoutedInputRequest`; the frontend
  resolves the owning client and applies deterministic authority-local XKB
  modifier state before emitting core and selected XI2 events. Live topology
  updates produce mask-selected RandR notifications, and surface resize keeps
  new geometry quarantined until matching pixels commit.
- XI2 pointer state is authority-local X compatibility state: `QueryDevice`
  exposes live X/Y and cumulative scroll valuators, `XIQueryPointer` derives
  immediate-child, local-coordinate, button, and modifier state from the X
  hierarchy, and selected device events carry the same hierarchy-relative
  facts. Engine continues to route only protocol-neutral pointer packets.
  Core and XI pointer queries share the last logically processed input in the
  caller's namespace, including connections that selected no input events.
  Socket delivery and grab targets do not own this observation. Queries refine
  Engine's surface selection through the current X hierarchy; they cannot
  sample another namespace or perform compositor hit-testing. A security epoch
  clears the observation, and frozen input does not advance it before thaw.
- A policy-managed X11 `MapWindow` first emits a protocol-neutral
  `SurfacePresentationIntent`; it does not make the window client-visible.
  Engine and the blind WM can therefore plan from geometry, constraints, role,
  and generation before any pixel buffer exists. After Engine accepts the
  proposal, one `AdmitSurface` control configures and maps the X window, and
  matching authority pixels complete the atomic visual commit. Override-redirect
  and standalone frontend paths retain their ordinary immediate-map behavior.
- X root-child transients and EWMH popup-like functional types remain outside
  blind-WM placement. The X frontend reduces `WM_TRANSIENT_FOR` presence and
  recognized `_NET_WM_WINDOW_TYPE` dialog/menu/utility/splash/popup-like atoms
  to the client-positioned role. A resolvable transient value independently
  supplies an optional protocol-neutral presentation owner. Group transients
  which name the root retain the role without inventing an owner edge. The
  session ties an attached transient to its mapped, visible owner; property
  replacement/deletion and map/unmap/owner removal publish lifecycle snapshots
  rather than leaving stale popup layers.
- `sophia-portal` has deterministic reducers for clipboard, drag-and-drop, file
  handoff, screen capture, URI open, and notifications. Owner-only bounded
  broker IPC, policy-provider IPC, expiry/revocation lifecycle, and the first
  native-X `CLIPBOARD`/`PRIMARY` source-proxy executor are complete. The other
  portal kinds do not yet have complete native executors.
- Metadata broker interface revision 1 is a bounded, owner-only production
  transport. In a Hagia session the broker runs as its own protected process,
  publishes `MetadataDisclosureRule` records to the X Authority, receives only
  reduced candidates, and returns sanitized descriptors to Engine's session-owned
  `ChromeDescriptorTable`. The production default is `ClassOnly`; raw titles,
  classes, PIDs, paths, and icons remain authority-private. Engine now has a
  title-only projection from this table: bounded generic rectangle/text nodes,
  per-head lowering, and opaque presented targets. Experimental
  `sophia_shell_v1` now consumes it through a separately protected Hagia Shell.
  Core and admitted XI explicit pointer grabs share Engine's application lease
  arbitration, so the compiled profile enables that path. Signed installed
  evidence remains open.
- Public Hagia policy and the metadata broker are launched through the production
  Bubblewrap protection backend. The backend rejects forbidden role composition,
  clears the ambient environment, inherits only selected standard streams, denies
  networking, creates private user/PID/IPC/UTS/cgroup namespaces and filesystem
  roots, and exposes only declared role paths. The session authenticates each role
  socket against the actual host peer PID inside the wrapper. Bubblewrap 0.11.2 is
  the installed-session floor. Other future external authorities must opt into an
  appropriate separately supervised domain before production admission.
- Standard DRI3 1.2 carries FD-bearing `Open`, modifier-bearing multi-plane
  pixmaps, xshmfences, and Present submissions through bounded frontend batches.
  A Mesa RADV `vkcube` trace reaches an Engine transaction without an X11 error.
  The reusable renderer-private DMA-BUF registry and cloneable Present feedback
  router now feed the persistent mixed CPU/DMA-BUF renderer and page-flip
  retirement path. Physical AMDGPU evidence requires one complete render target
  per mixed export. The context and pipeline are destroyed after export rather
  than rebound; the exported buffer retains its GBM/EGL surface through
  page-flip retirement. Binding a retained context to a fresh surface still
  caused the third command stream to be rejected. The paired software and
  CPU-plus-Vulkan hardware gate now
  passes through Engine-owned KMS with controlled rejection recovery and exact
  resource retirement.
- XLibre is absent from the production workspace and launcher. Its frozen
  prototype remains under `research/xlibre` as historical evidence.
- The former Wayland frontend is absent from the production workspace,
  dependency graph, launcher, and validation gates. Its frozen prototype and
  last Kitty/SHM and controlled DMA-BUF evidence remain under
  `research/wayland` as proof that Engine transactions were not X-shaped.

### Production Session Loop

The physical owner services native page-flip retirement before routing new
shortcut actions. External WM packets cross a bounded typed queue with at most
one request in flight; a dedicated transport worker owns the blocking socket
round trip. The owner retains policy authority: it correlates the response,
rejects proposals whose layout fingerprint became stale, validates the opaque
layout transaction, and applies workspace and session effects only after the
atomic layout commit. The worker owns no scene, focus, workspace, application,
or protocol state.

A stale reply is also a private-model lifetime boundary. The external WM may
have already applied that request to speculative state, so later work must not
continue against the same model. A stateful policy such as Hagia is stopped and
reseeded from committed Engine state. The bundled Rust proof policy is a
stateless pure projection of each received snapshot and therefore needs no
private-model rebuild. In either case, no speculative policy state survives
`RejectedStale` or `TimedOut`.
`RejectedInvalid` and `Disconnected` remain fatal transport outcomes.

Owner completion evidence separates child-reap and physical-input phase maxima
from WM transport round-trip and queue-dwell measurements. A slow policy
process may delay a policy result up to its bounded timeout, but it cannot hold
page-flip retirement or physical input on the owner thread.

Dynamic DRM topology enters through a capacity-one udev rescan notifier. The
notification is only a replaceable hint; it never carries authoritative
connector state. The session owner advances the application-input security
epoch once, clears deferred pointer ownership, and keeps only Engine shortcuts
active while it drains or forcibly detaches the old scanout generation. The
owner then performs a complete libdrm rescan and publishes one replacement
bundle to native scanout, CPU composition size, pointer confinement, the X
frontend's RandR snapshot, and public policy. Those cross-process consumers
settle sequentially under quarantine: “bundle” means every publication carries
the same complete owner generation and input cannot resume across a partial
settlement, not that separate IPC endpoints change simultaneously. An empty or
temporarily failed rescan remains quarantined and retries without consuming
another security epoch. Applications and the last committed Engine scene
remain alive.

Input does not resume merely because KMS reopened or RandR acknowledged the
snapshot. If topology changed, the matching WM projection must commit first;
only a later page-flip retirement releases the quarantine. A response based on
the retired output set becomes stale when the owner observes the replacement,
before a new policy request is issued. VT acquisition, switch rejection, and
startup scanout recovery use this same owner transition instead of treating a
changed connector set as a fatal session error. Native connector identities,
udev records, DRM handles, and rescan errors remain backend-private.

The live X11 path has completed the Milestone 4 presentation work: standard
DRI3/Present registrations, acquire-fence gating, mixed CPU/GPU composition,
KMS submission, page-flip-driven Complete/Idle feedback, controlled rejection
recovery, and exact resource retirement pass on retained hardware evidence.
Milestone 5 application promotion uses the repeatable, unattended classic and
confined QEMU session gate; machine-specific runs are compatibility diagnostics.

The physical validation stack also has a single-client production profile. It
launches one approved native-X application directly, uses the external
reference WM's metadata-blind `natural` policy, and retains the normal deferred
admission and native scanout path:

```text
 [single validation client]
            │ X11 / DRI3 / Present
            ▼
 [Sophia X authority] ── opaque intent ──► [natural-size policy]
            │                                  │ centered geometry
            └──────── transaction facts ◄──────┘
                            │
                            ▼
 [Engine admission] → [renderer] → [DRM/KMS retirement]
```

This is an isolation profile, not an alternate compositor architecture. The
selected executable belongs only to session launch configuration. The policy
sees no executable identity, Engine remains protocol-neutral, and a successful
run exercises the same production transaction and scanout owners used by a
full desktop.

Renderer-live owns the CPU scene. Backend-live owns native scanout, the concrete
per-output runtime set, and `LiveProductionVisualRuntime`, which contains
Engine commit/preparation, composition, Present scheduling, KMS
submission/retirement, cleanup, and reduced feedback sequencing. Production X
polls one backend service entry point; backend-live decides retirement, queued
Present scheduling, and pending native submission order and returns a reduced
tick. `runtime_driver` owns CPU cycle order, per-submission Present preparation
against committed visual state, the prepared retirement commit-before-feedback gate, and dynamic
asynchronous KMS phase order from reduced observations. Backend-live executes
requested retire, Present-schedule, and pending-submit phases; it is not a
second phase coordinator. The production session-loop architecture target is
complete; native Hagia/Narthex evidence owns current desktop qualification.

The production target is one protocol-neutral session coordinator in
`sophia-engine::runtime_driver`. It owns the ordered visual state machine while
calling narrow adapters that retain their existing authority:

1. accept a bounded authority envelope, preserve its ordered atomic transaction
   groups, and transfer native registrations immediately into renderer-private
   ownership;
2. validate, commit, or prepare transactions against Engine committed state;
3. compose one immutable frame from the resulting Engine snapshot;
4. ask the live backend to submit and retire that frame through KMS; and
5. route protocol feedback only after the matching retirement.

The coordinator owns sequencing, not foreign state. X resources remain in the
protocol authority; imported images and fences remain in the renderer; GBM, DRM,
and KMS objects remain in the backend. `sophia-session` retains process and
recovery policy, constructs the narrow adapters, launches supervised processes,
observes proof criteria, and requests shutdown without acquiring frame
authority. `sophia-cli` selects the installed command and presents returned
errors and evidence; it owns no session state machine and does no frame work.

`PreparedSurfaceCommit` remains the asynchronous Present gate. Preparation
snapshots protocol-neutral state without changing the committed scene. Matching
page-flip retirement revalidates and merges the candidate, then permits Present
feedback. Composited Copy idles its captured source before Complete; a future
direct Flip may complete before its retained source becomes idle. Rejection, timeout, disconnect, surface
removal, changed baselines, or backpressure discard pending work, preserve the
last committed geometry-plus-pixels state, and retire native resources exactly
once. No failure path may infer presentation from client traffic or send
feedback before backend retirement.

The session drains retirement-authorized feedback after lifecycle service,
before an authority run can take its no-work exit. Delivery does not depend on
another client request or on first arming the native service timer.

An eligible CPU composition that the backend defers remains a cadence repaint
obligation. Ordinary repaint shares the retained-publication guard: suspension,
an in-flight Present, or an unsettled software Present binding postpones it.
Postponement preserves the obligation and advances its retry deadline; it
neither spins nor supersedes another frame's retirement proof. Startup and
topology replacement retain their separate forced repaint paths. DMA-BUF
Present traffic alone must not create a synthetic CPU repaint obligation.

Each queued Present owns exactly one matching `SurfaceTransaction`. Persistent
scene state is the Engine's committed snapshot, not a table of historical
transactions. Preparation rebases only the queued surface's causal generation;
unrelated surfaces enter the immutable frame through the committed baseline and
retain their own generations and transaction history. Input and focus
projection advance from committed state after the matching retirement, never
from a pending Present candidate.

Layout changes use a separate Engine-owned epoch coordinator. It joins opaque
WM size proposals to authority-observed logical surface extents and keeps each
buffer's exact content extent as separate retirement evidence. A protocol
authority may therefore project an inset child buffer onto a larger managed
surface without scaling it or teaching Engine about the protocol's hierarchy.
Declared surface constraints remain separate from temporary recovery
constraints. A timed-out epoch preserves the complete committed scene. If a
surface has safe content at
another extent, Engine may publish `min_size == max_size == safe_extent` and
`resizable = false` for one bounded blind-WM replan. Engine supplies no
position, floating decision, client identity, or protocol object. Late pixels
from the abandoned extent remain fenced until the authority returns to the
safe content extent.

Admission is a state machine beside, not inside, committed visual state:
`PolicyPending`, `ControlPending`, `AwaitingPixels`, `AwaitingRetirement`, then
`Managed`. Planning records never contain a synthetic buffer and never enter
the committed scene.
An admission control acknowledgement proves only that protocol configure/map
effects were delivered; visual admission still waits for a concrete buffer at
the accepted content extent. Presented admission selects one exact
surface/transaction/target-buffer key and remains unfocusable until its native
frame retires. This rule is independent of whether the authority exports a
DMA-BUF or materializes immutable CPU storage. Only non-Present backing
snapshots may commit synchronously. A retained extent or previously committed
size may guide a recovery proposal, but it cannot satisfy an admission
surface's visual readiness: every admission-finalization proposal must own a
newly staged concrete transaction for that surface. A layout timeout does not
reset that lifecycle:
retry epochs retain `ControlPending` or `AwaitingPixels` surfaces in their
admission-finalization set; an `AwaitingRetirement` surface is already owned by
its exact visual candidate and cannot be replanned. Only the eventual matching
visual retirement may release quarantined pixels and Present feedback.

The first DMA-BUF candidate remains pending if scrolling or workspace visibility
keeps it outside the physical frame. It becomes runnable when its presented
geometry reaches its assigned output; another window's frames need not wait.
Skipping that first candidate would leave admission waiting for a retirement
that can never occur, with later pixels still quarantined behind it.

For X11, the protocol authority derives this boundary from its window tree.
Only a non-override-redirect direct child of the X root is policy-managed.
Descendants and override-redirect windows are client-positioned.
`MapSubwindows` applies only to the requested window's direct children; it must
not admit the requested parent or unrelated namespace windows. Parent links,
map state, and tree queries remain X authority facts and do not cross the blind
WM boundary.

X map state is the protocol's three-state value: `Unmapped`, `Unviewable`, or
`Viewable`. Deferred Engine admission is a separate authority flag and must not
be encoded as an invented map state. Mapping below a non-viewable ancestor
produces `Unviewable`; making that ancestor viewable promotes its mapped
descendants, while unmapping or reparenting an ancestor demotes the viewable
subtree. `AuthoritySurface::mapped` is true only for `Viewable`, so Engine does
not receive false visibility while the X authority still owns an incomplete
ancestor transition.

Software drawing to an X descendant is translated and accumulated into its
root child's immutable presentation buffer. Engine receives the toplevel
Sophia surface and translated damage, not a separate WM node for every X child.
The accumulated extent derives from concrete child buffers and never grows
merely because policy configured a larger toplevel.

Pixels observed before managed admission remain authority evidence, not visual
truth. The session retains ordered transaction-homogeneous groups in a bounded
quarantine, excludes them from renderer intake, and releases complete groups at
accepted geometry only when the admission state and concrete extent agree.
Transactions and Present submissions retain their causal ID across that
boundary; they are never injected into or relabelled as the frontend envelope
that happens to trigger release. Overflow or mixed identity is terminal and
fail-closed. A DMA-BUF surface transaction and Present submission have exact
one-to-one surface/buffer cardinality. Their buffer and fence release facts stay
deferred while quarantine references them, and backend intake registers the
resources, begins presentation ownership, then applies release. These cold-path
records do not add buffers to the passive Engine admission table.

Present scheduling is classified per submission. A buffer matching the pending
layout epoch is staged with that epoch. Another valid buffer for an
already-managed surface updates the committed geometry through the renderer's
pixel-aligned copy-and-clip path, but it cannot satisfy or promote the pending
resize. Malformed or superseded submissions are rejected. A complete
pre-admission Present remains recovery evidence at its natural extent, and
submissions for unrelated surfaces remain immediately eligible. A layout epoch
must not become a session-wide Present barrier. The scheduler shares one
immutable authority batch and retains only bounded submission records rather
than cloning the full batch per queued Present.

Storage classification is per atomic authority group, not per owner envelope.
One owner batch may contain a software Present group and an unrelated DMA-BUF
Present group. Each native frame receives a monotonic typed identity, and a
software Present binds to exactly one immutable CPU or retained-mixed frame.
An unrelated DMA-BUF frame may submit and retire first, but its callbacks
cannot mark or settle the software Present. The runtime then submits the bound
software frame and routes Copy/Idle feedback only from that exact page flip.
A retained-scene checksum may suppress an ordinary projection whose newest
pixels are already owned by the output; it must never suppress a software
Present. Every accepted Present queues a fresh frame on every applicable
logical output so its selected clock output produces a distinct physical
retirement. Staging retains the submission in its teardown-visible waiting
queue until complete cohort validation and native queueing succeed, so a
failure cannot strand surface-content ownership outside the cleanup ledger.
A same-owner batch containing software Present work cannot be coalesced, and a
single group that claims both storage paths is malformed and fails closed.
Future frame merging must preserve this explicit ownership relation rather
than infer it from queue order.

The owner loop may commit several authority batches in one production cycle,
in arrival order, composing once at the newest committed state. Merging is the
frame-merging case that ownership relation anticipated, so it is restricted to
batches carrying only client content: any batch with Present work, surface
removals, DMA-BUF or fence traffic, presentation intents, or output
reservations keeps its own cycle. Merging also requires a quiescent admission
pipeline, because projecting a batch drains the released-group queue and a
release landing between two projections of one cycle would emit a quarantined
group twice. A raster-requirement response may open a merged run but never
join one, since it is judged against the requirement state of its own cycle.
Without merging, a client that draws in a burst is displayed one draw per
frame, trailing itself by the whole backlog.

Normal session shutdown closes frontend ingress without cancelling accepted
work. Frontend EOF is an ingress fact, not an empty-owner-queue guarantee:
initial and buffered authority batches, including nonmergeable removals and
resource releases, still run in order before accepted coordinator work.
Topology quarantine and bounded native-frame service retain their scheduling
priority. Successful quiescence requires frontend drain, empty authority
queues, settled issued layout/coordinator work, acknowledged controls, and
settled CPU/native work. Final layout/removal processing can issue a control
after frontend EOF; even an immediately generated acknowledgement must be
consumed and accounted for before the owner leaves its service loop.
A closed receiver is not polled again; the owner continues deadline-bounded
service. Timeout remains a failure with emergency cleanup, never evidence of
successful normal teardown.

Authority and committed surface extents are client-content geometry. Before a
layout node crosses the WM boundary, Engine converts both geometry and
constraints to outer allocations using the active chrome clearance. The
matching WM response is inset through the inverse operation exactly once.
Chrome therefore cannot turn an exact recovery constraint into a smaller
client configure.

Native WMs consume the same generic constraints through `sophia_wm_v1` and
decide policy without application identity or X11 root-window authority.

### Configure, Visual-Candidate, And Rendering State

Sophia follows river's separation between scheduled policy, client configure,
and rendering state, adapted to Sophia's stronger authority boundary. A WM
proposal is not a buffer, a delivered configure is not a commit, and an
authority-observed buffer is not rendered merely because its extent is known.

```text
                    opaque constraints and geometry
              ┌────────────────────────────────────────┐
              ▼                                        │
       ┌────────────┐   control   ┌────────────┐        │
       │ requested  │────────────▶│ configured │        │
       │ WM extent  │             │ client     │        │
       └────────────┘             └─────┬──────┘        │
              ▲                        │ authority       │
              │ timeout/replan         │ observation     │
              │                        ▼                 │
       ┌──────┴──────┐           ┌────────────┐         │
       │ exact safe  │◀──────────│ candidate  │         │
       │ constraint  │ evidence  │ quarantined│         │
       └─────────────┘           └─────┬──────┘         │
                                      │ Engine prepare  │
                                      ▼                 │
                                ┌────────────┐           │
                                │ committed  │           │
                                │ visual     │           │
                                └─────┬──────┘           │
                                      │ KMS page flip    │
                                      ▼                 │
                                ┌────────────┐           │
                                │ rendered   │───────────┘
                                │ and focused│
                                └────────────┘
```

The Engine reduces complete authority observations into one passive
`SafeSurfaceObservation` per surface. The record carries the exact transaction,
surface, and target-buffer identity, plus extent, evidence class, and Engine
observation sequence. During admission, a complete presented buffer outranks
an accumulated software-backing snapshot regardless of arrival order. The
buffer identity prevents a policy-sized background clear in the same authority
transaction from impersonating the client frame. Within one evidence class
the newest observation wins. After admission, normal committed ordering
resumes. A temporary admission constraint may project only a selected candidate
whose exact transaction remains in the bounded pre-admission quarantine; a
candidate-less committed size is not admission evidence. While presentation is
still `PolicyPending`, `ControlPending`, or `AwaitingPixels`, that constraint
tracks a stronger or newer selected candidate. It freezes in
`AwaitingRetirement`, where only the already-armed exact candidate may complete
admission.

This reducer does not make the Engine understand X11. The X authority owns the
meaning and order of Present, core drawing, SHM drawing, clears, and backing
storage. A raw `XPixmap` is authority-local identity, not renderable evidence:
the authority resolves DRI3 pixmaps to DMA-BUF storage and snapshots software
or MIT-SHM pixmaps to immutable CPU storage before emitting the transaction.
The observation batch separately marks surfaces produced by a complete
presentation request, so presented-frame evidence does not depend on the
storage type. Accumulated core/SHM drawing remains backing-snapshot evidence.
Xserver is the reference for the longer-term content rule: those operations
form one ordered logical window stream rather than unrelated whole-window
owners.

Engine enforces that order through the protocol-neutral
`SurfaceContentStream`. The stream sees only exact `SurfaceTransactionKey`
owners, touched surfaces, removals, and opaque authority-group payloads. It does
not see X requests or renderer resources. A ready production group carries its
CPU mutations and transactions through the same admission decision; an
asynchronous DMA or software Present owns the surface until exact retirement or
rejection. Newly unblocked groups reenter the ordinary production cycle in FIFO
order and are rebased sequentially against committed generations. Unrelated
surfaces remain independently runnable, and configured geometry remains
separate from rendering state.

A content-stream delay may outlive the layout epoch attached to a Present.
The scheduler retains bounded commit and abort outcomes, then reconciles the
submission when it reaches scheduling: aborted work is settled as skipped,
while committed work runs only after its surface is visible. A resolved epoch
must never be recreated as pending merely because its Present arrived late.

Niri supplies a compatible policy precedent, not Sophia's transaction model:
fixed client constraints can cause a window to open floating. Sophia publishes
generic exact constraints through the blind WM protocol; a native policy may
float the constrained node. Engine never selects a floating position or
inspects application identity.

Admission quarantine follows the same order. The selected complete Present
supersedes older groups whose content it fully covers; older Presents receive
Skip/Idle settlement and older backing-only groups are discarded. Newer groups
remain fenced until the selected page flip retires, then drain in original
authority order. Removal, timeout, and disconnect settle the same bounded
records without resource debt.

A Present no head can carry is paced, not settled on arrival. The three
conditions that establish it -- absent from the presentation order, no output
it routes to, absent from the lowered head frames -- each park the candidate
until the head's next refresh instead of completing it in the owner pass its
request arrived in. Withholding the completion withholds the Idle that returns
the buffer, so an invisible client blocks on its own back buffers exactly as a
visible one blocks on retirement, and pays the same per-frame cost rather than
its render rate. The verdict is not revisited at the tick: a surface that
becomes visible presents a newer buffer, which enters the queue runnable.
Parking is bounded per surface, drains with the runnable set when a topology
transition escalates, and invents no display clock -- the completion carries
the last real display sample, because a fabricated MSC would break monotonicity
the moment the surface returned to a head.

Evidence retention bounds each record name's share of an event segment. Volume
is what decides which records survive rotation, so without a per-name bound the
fastest kind evicts every other one, and the sparse records that explain a
session are lost to a burst that explains only itself. A segment that closes
with names it had to refuse records that fact at the head of the next one.

There is no compatibility-only committed-snapshot production adapter. Every
active client path enters through authority transactions. XLibre and Wayland
remain historical evidence and cannot become alternate production loops.

## Load-Bearing Ownership Rules

| Component | Owns | Must not own |
| --- | --- | --- |
| Sophia Engine | physical input, scene graph, hit-testing, committed visual state, damage, frame scheduling, rendering, DRM/KMS presentation | client protocol parsing, protocol-local resources, portal decisions, WM policy |
| X Server Frontend | X11 sockets, authentication adapter, XIDs, atoms, properties, selections, focus, grabs, events, drawing readiness | physical devices, workspaces, final layout, portal policy, renderer imports, scanout |
| Session runtime | process supervision, namespace registry, admission, authorization publication, bounded I/O coordination, recovery | protocol meaning, visual policy, portal allow/deny decisions |
| Spatial policy process (current Sophia WM) | layout, workspace, focus and launch policy through opaque handles | XIDs, protocol objects, namespaces, titles, PIDs, payloads, rendering |
| Portal broker | bounded transfer policy, decision and grant lifecycle | protocol object tables, payload rendering, DRM/KMS, client-global visibility |
| Portal executor | concrete payload/handle transfer and native protocol completion | policy decisions or ambient namespace authority |
| Metadata broker/shell | disclosure policy, trust assignment, icon tokens, cross-authority aggregation, compositor-owned chrome | raw client identity, WM identity disclosure, client-protocol authority |

No component may acquire another row's authority merely because it currently
runs in the same process. Crate and process boundaries may evolve; ownership
does not.

**No policy process may own a client-facing endpoint.** A spatial-policy process
connects to the session runtime; it does not listen. When policy has something a
shell must display, it travels through Engine's commit as an indicator descriptor
or through a broker, never through a socket the policy itself serves.

The rule is structural rather than stylistic. Wayland compositors each grew a
private shell socket, so every shell grew a backend for every compositor;
Noctalia carries nine. Sophia's policy processes are external and already speak
one protocol, which makes a single path enforceable. Removing this rule
reintroduces the fragmentation it prevents. See
`docs/sophia-indicator-descriptor.md`.

[Scripting Sophia](scripting.md) applies this boundary to the proposed public
control interface: the session owns caller admission, authorization, and
routing; WM and shell clients retain their respective command semantics and
existing role connections. A scripting caller acquires no role authority or
application-data access merely by reaching that endpoint. The experimental
endpoint and `sophia msg` implement policy actions and confirmed WM restart as
specified in [control v1](sophia-control-v1.md). The 9P direction changes the
target public transport, not these authority boundaries or the current service's
advertised operations.

## Protocol Frontends

A protocol frontend parses one client protocol, owns its object/resource
tables, enforces the admitted namespace context, and reduces client-visible
state into Sophia records. It may own protocol focus, grabs, selections,
configure/ack state, buffer readiness, and lifecycle behavior.

A frontend emits:

- `AuthoritySurface` and `SurfaceTransaction` values;
- surface removal and lifecycle facts;
- metadata candidates, and labels reduced under the broker's disclosure rule;
- bounded portal request facts;
- accept/reject acknowledgements for Engine-routed input and control.

A frontend keeps its raw titles, classes, PIDs, and paths. They are inputs to its
own reduction, not to anyone else's.

No frontend may duplicate Engine's scene graph or infer presentation from
client traffic. Engine output snapshots and presentation retirement flow back to
the authority so it can produce native protocol replies and events.

Protocol-visible presentation feedback is likewise frontend-owned. Engine may
send a reduced fullscreen/maximized/minimized/ordinary state with a settlement
identity; the X frontend alone translates it to ICCCM/EWMH properties and
acknowledges after socket visibility. Policy never receives X atoms, and a
frontend acknowledgement cannot promote a stale Engine candidate.

### Sophia X Server Frontend

Use **Sophia X Server Frontend** for the component and **X11** for its
client-facing API and wire protocol. “X Authority” is acceptable shorthand for
the protocol role and current crate name.

The frontend follows the Phoenix strategy, not Phoenix code: implement a modern
X server cleanly, retain the established X11 API, and expand only from real
application evidence. The accepted 9P direction adds a separate application
frontend target; it does not replace X11 or put 9P semantics inside X authority.

Modernization happens beneath X11 through Sophia-owned atomic commits,
Engine-owned presentation, explicit buffer readiness, session-selected
namespaces, and narrowly justified extensions. Classic shared-X is a supported
profile rather than a defect to be silently removed.

The detailed contract is in
[sophia-x-authority.md](sophia-x-authority.md). Real-client admission is tracked
in [x11-compatibility-matrix.md](x11-compatibility-matrix.md).

### Target 9P Application Frontend and Other Candidates

X11 remains the only supported application frontend. The
[9P application frontend](sophia-9p-authority.md) is now an accepted architectural
target, with its API and integration still to be designed and validated. It owns
application protocol objects and translates admitted operations into existing
Sophia transactions and routed-input boundaries. Session and the existing role
owners retain WM, shell, administrative and portal semantics even when their
public interfaces also use 9P.

Application-owned service exports may be composed through explicit grants;
their semantics remain with the application and are not disclosed to blind WM
policy. Discovery, delegation and revocation require an explicit contract.
Mount visibility does not replace admission or checks on retained handles.

Other compatibility translators remain candidates requiring named product
evidence. Every frontend must remain subordinate to Engine and session policy
and must not import another protocol's shell, workspace, physical-input or
presentation authority. The 9P direction establishes neither unmodified Plan 9
application compatibility nor a general promise to support other frontends.

### XLibre Boundary

XLibre is not a production dependency or an active integration track. Its
XComposite mirror, namespace experiment, and routed-input extension are
historical lessons under `research/xlibre`.

A future XLibre provider may be reconsidered only when measured native-X gaps
justify its authority and maintenance cost. Such a provider could own broad X11
semantics, but it could never own Sophia DRM/KMS, physical input, layout,
namespace admission, portal policy, or session control.

## Atomic Visual Transactions

Sophia commits geometry and matching pixels together. An authority may retain
pending geometry, buffers, damage, and protocol synchronization, but Engine
presents only a coherent `CommittedSurfaceState`.

The default slow-client behavior is:

- preserve the last committed good surface;
- do not stretch stale pixels into new geometry;
- do not expose half-rendered buffers or black resize borders;
- reject stale generations and invalid readiness;
- degrade only through explicit bounded timeout policy.

X core drawing, SHM, Render, and Present/DRI3 must reduce to the same readiness
model. Any future frontend must do the same. Prototype `LayoutEpochState`,
XComposite readiness inference, and the retired Wayland adapter are historical
compatibility mechanisms, not the permanent native-X seam.

Renderer imports and KMS handles remain backend-private. An authority may pass
an opaque buffer handle with explicit ownership, dimensions, format, damage,
and fence/readiness facts; it never submits scanout.

## Native Performance Direction

Optimization preserves the same authority boundaries. Engine produces one
immutable, contiguous frame plan per output generation. The renderer keeps
contexts, pipelines, imports, and reusable frame surfaces in bounded
generation-keyed storage; the backend remains the owner of KMS submission and
page-flip retirement.

The warmed path targets no context, shader, or surface allocation. Damage and
buffer-age history reduce composition work, while incomplete history forces a
full repaint. One newest pending frame and one KMS submission are retained per
output so slow rendering cannot create an unbounded queue.

Pointer motion reaches the X frontend at the composition cadence, not at the
rate the device produces it. A 1 kHz mouse otherwise mints a delivery
identity, a route lease and an ordered acknowledgement per packet, which keeps
authority work continuously available to the owner loop and costs the client
frames: a measured shake held a 120Hz head's client at half its idle rate.
Motion is buffered latest-wins per target surface and released on the frame
boundary, so a client sees the pointer's current position once per composed
frame. Release is bounded by the frame interval as well as by the pacer, since
a session composing from client submissions requests almost no paced repaints.
Anything that changes state releases the buffered motion ahead of itself, so a
click still lands after the motion that positioned the pointer under it, a
crossing into another surface releases the position the departed client last
saw, and buttons, scroll and keys keep their immediate path. Scroll is not
coalesced: each axis packet carries a delta, so latest-wins would lose
distance.

A due paced repaint takes an owner turn from authority work only when it could
run on that turn. The repaint yields to a pending layout epoch and to a
topology preparation, and a refusal does not move the pacer's deadline, so the
repaint stays due; letting it preempt anyway is a livelock, because a held
epoch ends on the authority batch that carries the client's frame, and that
batch is what the preemption keeps refusing. One predicate answers the
preemption, the wait cap and the repaint, so the three cannot drift.

A frame a client presents before it maps its window is skipped once the map is
acknowledged and is not observed by the layout: nobody will see it, so it is
not an extent anything can resize from. A launch epoch treats such a surface
exactly as one that has drawn nothing, deferring it out of the gate rather than
holding the gate on it.

A cursor-only atomic commit is taken only while the client is quiet. The
commit blocks until the kernel applies it at a vblank -- deliberately, because
it carries no page-flip event and the owner must not guess at a completion it
did not observe -- so one issued between a frame retiring and the next
arriving spends the vblank that frame needed. The cursor rides a frame that is
going out whenever there is one, which costs nothing; it commits alone only
when no primary has retired on that head for two refreshes. That keeps the
pointer moving on an idle desktop, which is the reason the commit exists, and
keeps it out of the way of a client that is drawing.

Physical AMDGPU recovery evidence requires GL execution to be isolated from the
session owner. After the initial synchronous modeset, the production native
path moves EGL, GL, GBM targets, imported-image residency, and locked front
buffers to a bounded renderer worker. The owner sends an immutable owned frame
command and receives only a passive scanout descriptor and lease ID. The
current worker uses a duplicate of the KMS DRM file, so its GEM handle namespace
is shared and KMS consumes the descriptor directly. It must not PRIME-export
and re-import that buffer into the same DRM file. An eventual independent
render-node worker is the opposite typed mode: PRIME FDs are mandatory and its
renderer-local handles are never submitted directly. `Pending` is an ordinary
deferred scanout state, not a failed frame. KMS page-flip retirement drops the
lease token; only then does the worker release the locked GBM buffer on the
thread that owns its native context. Each output owns exactly three complete
target slots, held inside whichever worker serves it: one per head while heads
render alone, or one shared by every output of a DRM device group when they
are coalesced. A slot index alone does not identify a bundle across outputs,
so the slots and the pixel proof taken from them are keyed per output within
the worker's render context. A slot retains its EGL context, GL pipeline,
GBM/EGL frame surface, and import cache together and carries a typed slot ID
plus monotonically increasing incarnation. The worker accepts a release only
when its lease ID and complete slot token match the retained buffer. A stale or
duplicate return cannot free a reused slot. Direct CPU GBM buffers remain a
separate bounded cache beneath this target lifetime; renderer-image capture
targets remain temporary and outside it.

```text
 session/input owner                 renderer worker
 ───────────────────                ───────────────
 immutable frame ───── bounded ───▶ EGL/GL/GBM render
       │                                   │
       │   shared-file descriptor           │ retains native BO
       ◀────────── lease ID ─────────────────┘
       │
       ├─ atomic KMS submit
       ├─ input, cursor, VT, X polling continue
       └─ page-flip retirement ───────────▶ release lease/BO
```

The worker has one request in flight per output, bounded command and lease
storage, three live generation slots per physical head, a 100 ms soft-stall
observation, and a 1 s hard-stall quarantine. A
hard stall cannot block physical input or invent presentation feedback. It
fails the pending frame closed and preserves the last committed scanout.
The output slot retains its mixed-frame identity and damage snapshot through
pending, renderer, KMS, and presented ownership. Newer work can replace only
the pending slot. Shared-file GEM descriptors and independent-file PRIME FDs
are explicit, mutually exclusive transport modes rather than fallbacks.
The presentation scheduler mirrors that ownership with one mutually exclusive
in-flight record:

```text
queued Present → rendering worker → KMS submitted → page-flip retired
       │                 │                 │                 │
 exact transaction   immutable frame   resource submitted  Engine commit
 prepared commit     and layer owner   and feedback held   feedback/Idle
```

Worker deferral is a state transition, not a rejection. Newer Presents remain
bounded in the queue until the in-flight record retires, so a later client
frame cannot relabel the KMS callback or consume the admission candidate.
When displayed, KMS-submitted, and renderer-prepared generations occupy all
three target slots, the worker returns the immutable frame to the exporter's
one latest-wins pending cell. This increments slot-deferral telemetry but not
renderer failure or completion counts. Exact page-flip release makes one slot
available, and only a compatible free slot may render the retained latest
frame. VT quiescence, partial mirror failure, and normal teardown all drain the
same lease tokens; none infer availability from reference counts.

Before replacing renderer owners for a VT handoff, the backend exports the
retained image set across all enabled heads. A head's store contains only the
images it has rendered; the union of those stores must cover the retained
scene. Replacement validates head membership before importing snapshots into
the corresponding stores. Heads sharing a renderer store import each image
once. A snapshot missing from every owner remains a failed handoff. These
snapshots and physical head identities remain inside the backend; WM policy
does not participate in the transfer.

An accepted page flip has a 500 ms hard watchdog. Crossing it terminates the
graphical session and restores the display manager instead of retaining an
unbounded black or frozen seat.
Multi-output GPU groups must eventually share this same worker service rather
than multiplying native owner threads.

Startup proof observations are monotonic and keyed by `SurfaceId`. A stable,
nonzero KMS retirement may precede the asynchronous focus acknowledgement; it
is retained until focus pins a surface, but can satisfy readiness only when
its key matches that exact pinned surface. Presentation timing is therefore
order-independent without allowing a status bar or unrelated client to prove
the startup application. DRI3 presentation evidence is not revalidated against
the base committed `BufferSource`: the presentation lease and base surface are
separate state domains. Its consumer also does not require the surface to
remain in the CPU/base committed-surface list. CPU detail is queried only when
that record exists; an exact stable nonzero GPU retirement remains sufficient
when it does not. Every owner-loop phase that may service native retirement
must call the same retirement recorder. Authority-wait progress and the normal
lifecycle phase are scheduling contexts, not separate presentation semantics.

Application startup proofs are development checks, not normal desktop lifecycle
requirements. An ordinary session may have no application windows, only panels,
or startup applications that fail. Completed authority activation admits the
session; committed launch intents then advance through the bounded application
admission queue without waiting for an existing focused surface. No overall
application-startup timeout applies unless a proof explicitly requests one.
WM response, application admission, input delivery, page-flip, and shutdown
deadlines remain obligations of their individual operations.

Normal native rendering becomes eligible for direct scanout and atomic cursor
activation after the active outputs have valid presentation evidence and output
input quarantine has cleared. This evidence does not require nonzero pixels or
application focus. Explicit application proofs retain the stricter surface-pinned
composition barrier. Replacing the native owner clears its activation state.
Normal completion records application startup proof as `not_requested`; it does
not invent a successful proof timestamp or fail logout for lacking one.

Direct-scanout episode caching is a scheduling optimization, not validation of
each subsequent buffer. A submission carries an atomic-test report only when
that attempt actually issued TEST_ONLY. The report preserves its raw errno,
error kind, flags and canonical property values from the same request lowering.
Unknown request provenance stays unknown; property capture is bounded and must
never truncate the request sent to DRM. These passive records describe a past
test, never permission for a later commit or proof of a layout-specific refusal.
Cursor-drop retries and later out-fence writes remain distinct from the tested
request. Reallocation advice still requires an equivalent alternative on the
exact current device, scene and output generation.

An optional layout probe retains one original allocation through the exact
fallback render request. It expires after one second; new offers, including an
equal scene trace, and owner/topology transitions invalidate it. Capture and
probe admission have separate one-second rate limits. Only a known unsupported
explicit layout and a supported alternative of the same format and extent may
reach the comparison. Unknown capability data remains inconclusive.

On a quiescent card turn, original preparation is retried against the current
selection, cursor and VRR state. If its framebuffer can be created, consecutive
TEST_ONLY requests must differ only in the primary framebuffer. An explicit
AddFB2 refusal is a distinct observation: all source planes must have imported
successfully, and a fresh EINVAL must precede the alternative's test. The shared
production request builder substitutes the alternative's existing framebuffer
into the original selection and policy; the resulting intended request must
equal the alternative's canonical test request in full. No original atomic
result is invented. PRIME, resource-pressure, access and unknown failures do
not establish this comparison.

Temporary cleanup has its own bounded retry slot and runs even when ordinary
rendering is idle. The alternative is the ordinary compositor result, so no
extra pixel copy or probe-specific rendering is required. Every refusal preserves
the ordinary composition fallback and its partial cleanup obligation. These
observations do not authorize later commits or Present reallocation advice.

A successful comparison remains boxed on its prepared alternative until submission.
Only a fresh original rejection with EINVAL, a passing alternative and unchanged
canonical display request produce a compact witness. Real submission failure or
a cursor-drop retry discards it; completion-fence bookkeeping grants no additional
display authority. The witness follows the accepted owner, not diagnostic history.
Native retirement checks its exact device identity, head, frame, submission cycle
and target context. Topology and owner transitions invalidate it even when rollback
restores a previous numeric generation. Queued completions retain their own content
and disposition instead of reading a later head's displayed content.

Engine settlement may carry that witness only for the exact committed copied
surface candidate and renderer image/transaction, currently on a single output.
Stale candidates, different images or formats, and multi-output joins carry none.
That historical evidence accompanies only its exact copied Complete. The session
compares the current committed backing, placement and native context with its
acknowledged allocation snapshot. Native context identities never repeat across
owner reconstruction or numeric-generation rollback; exhaustion suppresses the
optional comparison. Physical head identity stays inside the native boundary.

The frontend captures the successful Present's original window incarnation,
presenter, buffer and layout before the pixmap XID can be reused. At Complete it
rechecks current window geometry, mapping, context and exact-format membership
in that presenter's available pinned bundle. The accepted alternative must still
be preferred and the original must not. A busy authority rejects the optional
comparison without waiting. Stale or unavailable state preserves ordinary Copy,
clock advancement and Idle delivery. The comparison never authorizes a later
commit. For an actual copied completion, the frontend may emit SuboptimalCopy
only when that exact Present opted in without requesting forced copying and the
current comparison succeeds. It claims at most one notice per exact surface in
an accepted preference generation, after accepting the first Complete and before
subscriber delivery. A partial or failed delivery does not restore that claim;
some subscribers may already have received the notice. Ordinary Copy does not
reset it. A valid replacement preference generation does. Retained and directly
flipped ownership keep their existing Flip mode and Idle ordering. The effective
wire mode is returned to the session for diagnostics and displayed-frame pacing.

The ordinary fallback can prefer the original fourcc without another render.
A required renderer format is strict; an optional preference may return to normal
allocation only before drawing begins, when target admission cannot supply it.
Import, context, device and post-draw failures are not reclassified as format
refusals. Actual exported format remains authoritative, and a different format
cannot support the pair. Reusable targets are admitted by actual format, extent
and modifier policy; the presence of an optional preference does not itself force
reallocation.

Renderer work captures its frame correlation before submission and returns it
with the actual exported lease. A newer pending frame cannot replace that
correlation. Successful owned exports carry it into prepared scanout; incomplete
or failed exports carry none. Request identifiers are scoped to their worker
facade and never wrap into reuse. Rejected or late results still owe release of
their exact returned output, lease and slot. A full command queue retains one
discarded-release obligation and defers further draining without blocking.

Performance cadence is accumulated from routed displayed-Present UST values in
bounded owner state. Completion emits one summary containing sample count,
advancing intervals, nonadvancing observations, overflow state, mean FPS, and
p95 frame time. Benchmark tooling consumes that summary rather than enabling
per-frame protocol tracing, so measurement does not require diagnostic I/O on
the presentation hot path.

Development takeover tooling may additionally arm an external wall-clock
deadline. That watchdog is a sibling of the Sophia session process group, not
an Engine subsystem. At its deadline it terminates the entire process group;
the launcher parent then restores the saved TTY and service state. Workload
identity and duration remain tooling policy, while Engine and backend
watchdogs continue to describe presentation progress only.

The installed release exposes the same launcher-owned boundary only through a
dedicated recovery-proof entry. Ordinary installed sessions leave the deadline
unset and therefore have no wall-clock lifetime. The recovery entry fixes its
deadline as packaging policy, while its verifier classifies the resulting
status-124 handoff separately from graceful Engine completion and the local
emergency chord.

Client DMA-BUF imports are renderer-owned, generation-keyed resources. The
renderer frame boundary carries only an opaque image generation plus the cold-import
descriptor. The native output context imports once, reuses the resident texture
for compositor-only repaints, validates the descriptor on every hit, and
evicts the predecessor before backend-live releases its idle fence. Context
reset clears native residency while the backend's still-live buffer lease
allows a bounded re-import. Neither the X frontend nor the WM can observe or
control the cache.

An import refused by the output device can use a bounded renderer transfer.
Backend discovery admits at most sixteen initialized DRM devices from the
actual seat, including devices without connected heads, and passes independently
owned render FDs to each native worker. Paths, executable names and vendor
heuristics do not enter Engine. Auxiliary contexts are initialized lazily.

The renderer first tries the actual incoming FDs on the destination. Cached
successful source-device hints can order later fallback attempts but cannot
skip direct import or establish buffer provenance. A source device that can
sample the buffer renders it into an explicit linear XR24/AR24 bridge. The
output device captures that bridge into its own immutable image. At most three
internal bridge buffers are pooled per context, charged by measured allocation
size to the existing image budget. Destination GPU completion is polled before
reuse. Externally exported destination images are never recycled through this
pool: their descriptors can outlive local references. Repaints of the same
immutable image reuse its retained texture without repeating the transfer.

The session subscribes before its initial device snapshot. Render inventory
notices compare exact device identities against that baseline separately from
connector/output changes. Identical udev replay causes no reconstruction;
failed inventory observations remain pending and retry at a bounded cadence.
Uninitialized devices are not admitted through the default seat. Replacement
render contexts and frontend bundles are prepared outside the owner loop, then
installed through exact generation acknowledgements. Preparation, queue
saturation and source-context refresh retain their pending obligations on
bounded deferral. Source inventory changes preserve destination contexts and
retained images; only completed internal bridge allocations may be discarded.

Each authenticated frontend connection pins one immutable bundle containing its
render device, measured import capabilities and allocator. A replacement is the
default for new connections only. Backings retain the originating bundle until
publication and release finish. Device loss marks that bundle unavailable and
cannot redirect an existing connection to a different GPU. The advertised
pixmap-texture capability remains fixed for the frontend's lifetime. Cached
per-output allocation preferences become optional per-window DRI3 preferences.
Tiled preferences require equal server-observed render-node identity across the
connection and output; absent or different identity retains measured LINEAR only.
The identity is cached at preparation, so queries require no device discovery.
Preferences grant no device authority and imply no successful atomic scanout test.

Each physical native head owns its plane-format observation beside its existing
allocation preferences, so sorting heads cannot detach formats from their owner.
Both come from one IN_FORMATS blob read. Strict
evidence distinguishes supported, unsupported and unknown layouts; malformed,
unreadable, implicit or over-capacity tables cannot prove exclusion. The strict
snapshot does not narrow the ordinary allocation-preference parser. Plane and
blob identifiers remain card-local, and a snapshot is not current proof across
native-owner replacement, lost device identity or recovery.

Worker shutdown uses an independent cancellation flag and never waits for an
unfinished thread or a full command queue. A bounded lifecycle registry retains
unfinished workers; a replacement on the same exact device identity waits for
its predecessor to finish. This bounds owner-side work and retained workers,
but cannot cancel a blocked driver call. Consumer imports and retained buffers
are destroyed before their EGL display.

This transfer applies only to buffers submitted to Sophia and readable by an
admitted device. It cannot repair a client-internal import before submission.
Successful server negotiation and offscreen pixel tests therefore do not alone
establish normal application launches without a device override.

Renderer maintenance remains worker-owned. Shutdown cache clearing uses a
bounded request/acknowledgement and returns the updated resource counters
before the owner emits completion evidence. A stream in which every client
frame has a new generation is expected to import each frame and may record
zero hits; reuse is required only for compositor repaints of an unchanged live
generation.

Direct scanout and hardware planes are backend capabilities, not alternate
authority paths. Engine first proves scene eligibility, the backend validates
the exact format/modifier through an atomic test, and any rejection returns to
mixed composition without losing committed visual state.

XLibre and mature Wayland compositors may provide external comparison runs and
implementation lessons. They do not enter Sophia's workspace, production
dependency graph, protocol surface, or session loop.

## Input

Sophia Engine is the single physical-input authority. For the implemented
startup path, the trusted session coordinator validates a typed input candidate
before opening the graphical session. Keyboard RMLVO and repeat values overlay
the effective Sophia configuration, explicit CLI RMLVO values remain superior,
and initial lock state is held only by the X authority adapter. The live backend
owns the translated libinput pointer policy and fails startup when a requested
device setting cannot be applied. Hagia receives neither this candidate nor a
raw device handle. Watched activation remains disabled pending the shared
prepare/activate/rollback barrier. The trusted coordinator now has the pure
seven-authority reducer for that barrier, ported from Hagia's model-checked
reference semantics. A second shared pure reducer defines each authority's
local participant state: monotonic admission, exact-key idempotence, previous
identity restoration, fail-closed same-generation digest handling, and
generation-wide tombstones for participants skipped after an earlier prepare
failure. An offline refinement harness drives the coordinator's actual effects
through seven independent authority-local candidate slots and checks identity
and payload convergence or exact recovery divergence at every failure
position. A second integration case feeds those slots the exact seven
owner-safe fragments emitted by staging and proves semantic payload promotion
for every authority. The startup launch gate is the only permitted visibility
boundary for this model. Production prepare/rollback dispatch is now wired for
public Hagia startup; activation, cross-process authority protocols, and a
live-reload visibility barrier remain unwired.

The startup configuration boundary constructs one immutable prepared desktop
candidate bundle after checking every raw authority candidate against the
profile's exact generation and digest. It then partitions that transient bundle
once: session, input, and output candidates enter cohesive typed owner records
backed by authority-local slots; shortcut remains only as a transfer payload
until the public shortcut owner creates its slot. The other authority payloads
remain in the provenance-bearing profile for their future owners. A
prepared-profile aggregate still constructs the raw profile, activation key,
and typed bundle in one load/prepare pass, while the existing raw-profile API
is a validated projection of that result. No source is reparsed, no long-lived
coordinator slot collection remains, and selection is not activation.

Each future authority handler can now load its staged fragment through one
shared admission function. The function accepts an expected authority and
activation key, reuses the trusted owner-safe bounded file reader, rejects
symlinks and cross-authority sections, and reconstructs the existing raw
candidate DTO only after schema, generation, digest, setting, and duplicate
checks pass. Round-trip tests cover every authority fragment; negative tests
cover authority isolation, both key components, unsafe modes, and symlinks.

Authority-local candidate state now has one generic slot rather than separate
identity and payload mutation paths. The slot owns a participant plus bounded
active, previous-active, and candidate payloads. It accepts both canonical
typed in-memory candidates and admitted raw fragments, delegates every identity
transition to the participant reducer, and updates payloads only after that
transition succeeds. Semantic retry comparison excludes provenance paths but
includes setting keys and encodings. No coordinator-owned collection of slots
or production activation handler exists yet.

The first production-owned preparation seam is session-local and remains pure.
Explicit CLI applications, arguments, startup ordering, and action mappings are
parsed once into a bounded immutable overlay. The session authority applies the
canonical typed profile candidate and that overlay, in precedence order, to a
clone of Sophia's trusted application registry. Unknown or ambiguous selectors,
duplicates, and bounds failures discard the clone. The accepted effective
configuration is unchanged from the prior startup behavior; this tranche does
not activate anything before the global barrier. Startup now retains the
canonical typed session payload in one session-owned generic slot and derives
the effective configuration from that slot's prepared payload. The slot remains
exactly `Prepared`; configuration assembly cannot promote it.

Authority owners construct prepared slots through one shared constructor rather
than reproducing initialization and transition order. The public shortcut
owner now retains its canonical typed payload in such a slot and resolves
registrations only from that prepared payload. Session and shortcut slots live
with their respective owners; they are not a coordinator-owned slot array and
neither is activated by this preparation path.

Input and output candidates now also live behind distinct typed owner records,
each containing exactly one prepared generic slot. Input keyboard/pointer
projection and output capability reconciliation consume only their owner's
payload. These records preserve domain ownership even while the current
single-process startup coordinator transports them to their execution seams.

Public policy filesystem preparation now has an explicit linear owner. Directly
after trusted configuration assembly—and before display, seat, input/output, or
client setup—Sophia creates the private policy directory, stages the profile,
and re-admits all seven fragments against the exact activation key. The
resulting context owns the fragment set, directory, named policy/shell/broker
slots, and prepared shortcut slot; the typed session/input/output slots remain
with their existing owners. Public policy launch consumes its context exactly
once. Failure or an unused context cleans up before any graphical process
starts. This is still preparation, not activation.

The coordinator's synchronous driver now exposes that prepare barrier as its
own typed operation. It begins one exact key, drains the seven prepare effects,
performs generation-wide rollback on any rejection, and otherwise stops at
`Prepared` with the previous active identity untouched. It emits no activation
effects. The complete startup driver reuses this operation before its activate
phase. Public Hagia startup now constructs a fixed-field dispatcher that
borrows the seven separate owners, invokes this operation before any graphical
setup, and retains the settled coordinator model at `Prepared`. Every local
failure triggers generation-wide owner rollback and aborts startup. The
dispatcher fails closed if activation is requested, so this milestone cannot
make a profile visible early.

For the implemented
application path it applies reserved shortcuts, walks transformed renderable
layers from the last presented output-frame snapshot, and selects a
generational `SurfaceId`. It constructs a
`RoutedInputRequest` containing global and surface-local coordinates and sends
that passive route to the owning authority. Keyboard selection follows Engine
focus rather than a pointer hit test. If Engine focus advances before the X
frontend acknowledges that same exact surface, Engine continues resolving
reserved shortcuts but holds client-bound keys in one bounded ordered handoff.
It releases them only to the acknowledged generational target. Target or seat
change, removal, timeout, topology/security transition, or capacity exhaustion
discards the complete held sequence; client input is neither rerouted to a
different focus nor sampled as a shortcut twice.

The native X Server Frontend then applies X11 focus, grabs, event masks, XKB/XI
state, and namespace checks. Confined profiles reject cross-namespace
authority; `classic-shared` intentionally retains traditional shared-X
behavior. A future frontend would remain responsible for its own protocol-local
delivery rules after the same Engine route.

When an unmodified primary press selects an unfocused visible surface, Engine
first sends the opaque target through the blind spatial-policy interface. It
retains the ordered press, drag motion, and release in a bounded handoff until
the frontend acknowledges protocol focus on that same surface. Before release,
every buffered target must still have its exact generational identity in both
the presented interaction projection and the frontend client-route table. Any
missing or replaced identity discards the entire handoff. Raw pointer events
and protocol-local identity never enter the WM.

The authority returns a reduced delivery acknowledgement. Engine never writes
arbitrary client events or receives a client connection handle. Route failure
does not fall back to synthetic input. Native application selection derives
from the pointer's output-local interaction projection only after that
output's accepted page flip; committed and submitted geometry cannot enter
that hit test, and independently retiring heads cannot publish one another's
input. Ordinary, passive-grab, and explicit core/XI pointer ownership uses an
exact Engine-visible route lease with frontend confirmation and ordered
release. Explicit grabs prepare before frontend mutation, after the requesting
connection's previously published authority observations have been applied.
Preparation and activation do not wait for scanout. Physical delivery requires
both activation and current presentation evidence; an unpresented reservation
has a bounded lifetime. Engine owns the readiness decision used by both lease
authorization and session routing. An unrelated scene revision refreshes
application presentation evidence rather than revoking an otherwise valid
grant. Exact identity, control, authority, device, output, and scope restrictions
remain mandatory. Shared authority-state locks are released during bridge
waits; stale state and saturation fail acquisition closed. The bounds and
cancellation rules are defined in [Target-Resolved Input](target-resolved-input.md#bounds-and-cancellation).

Input delivery stays off the WM path. The WM may choose focus policy in
response to the reduced opaque click target, but it does not receive motion,
button payloads, key events, or protocol identity.

The retired Tier-0 indicator interaction proved the first target-resolved
chrome slice from `docs/target-resolved-input.md`. The descriptor switcher uses
the same generic exact-target capture: a matching press/release returns only an
epoch-scoped opaque action and activation identity. Application, indicator, and
descriptor selection use the applicable output's last-presented interaction
snapshot. Engine retains per-output presentation epochs; an output or session
lifecycle change invalidates dependent input state. The compiled profile now
enables the live descriptor switcher. Richer Tier-1 interaction remains
deferred.

Per seat, a security/session transition first revokes old control epochs; a
reserved Engine shortcut follows; then one existing application route lease or
chrome capture retains ownership; only an unowned event performs a fresh
presented-state hit test. Shell chrome capture and application leases are
mutually exclusive. Pointer boundary crossing alone transfers neither.

The X frontend continues to own X11 event masks, XKB/XI state, and client
delivery, but reduces an admitted grab to an Engine-visible lease. Confined
leases cover application regions owned by that namespace; `classic-shared`
leases may cover application surfaces within that shared profile. Neither
covers shell, foreign-profile, trust, lock, or secure-session pixels. Ordinary
scope exit waits for a frontend release acknowledgement before shell capture;
security revocation stops delivery immediately and quarantines old-epoch
queues without waiting.

Shell targets use authority/session/slot/generation identity, presented
ownership and occlusion, deterministic overlap order, bounded per-seat capture
tied to the initiating device/contact, and explicit modal membership. Discrete
actions are coordinate-free by default and continuous values occupy one paced
replaceable slot. Exceptional local coordinates require a capability issued by
independent session or portal policy; the shell cannot authorize itself.
Normal visual revocation becomes effective on presentation, while security,
authority, device, output, or capability revocation is an immediate epoch
barrier. Bounded-queue exhaustion fails the recipient epoch closed rather than
coalescing an ordered boundary or failing every frontend client.

## Spatial Policy And Chrome

The WM consumes immutable snapshots keyed by opaque `SurfaceId` values and
emits `LayoutTransaction` proposals. It never sees XIDs, protocol object IDs,
namespace IDs, titles, classes, PIDs, paths, icons, credentials, or portal
payloads.

“WM” is the name of the current spatial-policy role, not a preferred kind of
interface. A policy process may tile, scroll, stack, float, combine those
approaches, run a single-application session, or try a model for which the
usual window-manager vocabulary is a poor fit. The packets carry geometry,
capabilities, and state; the policy process keeps its own trees, columns,
stacks, or other private model.

A complete environment is not folded into that role. For example, a native
port of a tiling or floating policy may occupy the policy slot with only a
small companion shell. An Xfce-style environment divides work among spatial policy, panels and
decorations, session services, portals, and ordinary applications. A future
Sophia-native compositor/Engine protocol would still meet these same authority
boundaries. The examples differ in form, not in who is allowed to own input,
protocol state, rendering, or scanout.

Role-specific sockets are necessary but do not by themselves preserve this
split. A protection domain includes every process that can exchange authority
through ambient IPC, shared writable memory, inherited descriptors, or
debugging. Blind WM policy may not share one with a metadata-bearing shell,
metadata/portal broker, or application frontend. A desktop family may share
source and executables only by launching separately supervised, sandboxed
processes without such ambient channels. Exact UID/PID endpoint admission is
authentication, not proof of that isolation, so the metadata-bearing role
sockets refuse it outright: they admit only the peer their supervisor launched
into a domain carrying that role, while the blind policy and output roles still
admit on a supervised PID. Production public Hagia policy and the metadata
broker therefore run in separate Bubblewrap domains. A shell-enabled session
launches Hagia Shell in a third domain carrying only the metadata-shell role
and its read-only socket path.
Their environments and inherited descriptors are cleared, networking and the
ambient host filesystem are unavailable, and only explicit read-only role/profile
paths plus Hagia's private checkpoint directory are bound. A protected broker
executable smoke checks those negative claims. This backend establishes the
current external-role floor; it does not imply that every future broker has
already been admitted.

Engine mints transaction IDs, validates every proposal, and keeps the last
committed layout when the WM is absent, malformed, timed out, or restarting. A
valid but rejected layout is a policy failure, not a transport failure.

Client-owned shell surfaces may reserve bounded root-relative edge spans. A
protocol frontend parses its native mechanism and emits only
`SurfaceOutputReservations` keyed by opaque `SurfaceId`; it does not send
atoms, object IDs, window types, metadata, or application identity. Engine
activates reservations only while the surface is mapped with the
`ClientPositioned` presentation role, clips partial spans against each output,
and reduces same-edge reservations by maximum depth. Opposing edges combine.
An invalid aggregate preserves the last valid work area.

Future native shell reservations are coordinated more strictly than these
current client-surface reservations because shell visuals and managed
application placement belong in one coherent presented scene. Engine binds an
exact shell candidate/reservation generation to the derived work-area
generation, the WM snapshot and connection epoch, and the answering projection.
Only a ready, exact bundle may replace the prior presented bundle. Normal
shell/WM failure preserves the entire prior bundle; security surfaces use a
separate preemptive path. Panel UI belongs to the user-selected shell. The
session adds no automatic indicator strip or fixed top reservation. Committed
policy indicators remain data; publishing them does not instantiate panel UI.

The full output rectangle remains the composition and hit-test space for the
client-positioned surface. The reduced work rectangle is policy input for
managed surfaces. A fullscreen managed placement remains a full-output
allocation. Nonfullscreen siblings, including maximized placements, remain confined to the reduced work rectangle.
Every native Sophia WM receives the same `bounds` field, so Engine contains no
WM-, shell-, dock-, or toolkit-specific branch.

Manage state such as size, focus, fullscreen, and workspace assignment is
separate from compositor-only render state such as position, z-order, crop,
opacity, and transforms. Engine owns frame-clock interpolation and cancellation;
the WM does not drive animation frame by frame.

Titles, icons, attention, trust badges, and close actions belong to the metadata
broker and compositor shell. Chrome output is sanitized and generation-checked.
Polite close is routed to the owning authority as `WM_DELETE_WINDOW` or
`xdg_toplevel.close`; process termination remains later session policy.

Opaque actions are issuer-scoped capabilities rather than global integer
commands. The authority role, issuer and revocation epochs, recipient role and
epoch, operation class, optional target generation, and activation identity
must all match at dispatch. Thus broker, WM/policy, and session operations
cannot be confused through numeric collision, forwarding, stale reconnect, or
target reuse.

Shell UI is supplied by separately authorized clients. The original Tier-0
bootstrap bar is retained below only to identify historical evidence; it is
not instantiated by current sessions:

| Tier | Renderer | Data source | Status |
| --- | --- | --- | --- |
| 0 (retired) | automatic Engine indicator bar | committed indicator descriptor | historical fixtures only |
| 1 | `sophia_shell_v1` display-list client | descriptor plus broker metadata | experimental title-only live slice; compiled profile enabled, signed physical proof open |
| 2 | ordinary X11 clients | X11 protocol surface | frontend compatibility |

The selected shell determines panel UI and placement. Engine supplies rendering
and input mechanisms and does not add a fallback status bar. See
`docs/sophia-indicator-descriptor.md` and `docs/sophia-shell-v1-direction.md`.

The renderer-neutral revision-1 slice carries at most sixteen exact-generation
descriptors into a centered, title-only list of solid and cached-text nodes plus
last-presented opaque targets. The shell owns order, selection, and lifecycle;
Engine owns validation, rendering, damage, per-head projection, and hit-testing.
Icons stay in the sanitized descriptor table and are not rendered by this
slice.

Sophia does not emulate an X11 root-window environment for legacy WMs. WM and
shell developers port policy to `sophia_wm_v1` and `sophia_shell_v1` directly.
The separate X Authority remains an application frontend and cannot be used as
a policy backdoor.

## Namespaces And Portals

Namespace identity is session policy, not protocol metadata. The supervisor
assigns an immutable `ClientAdmissionContext`; authorities enforce it; Engine
and WM do not reinterpret it.

Classic trusted X clients may deliberately share one namespace. Confined
clients receive separate namespaces and explicit capabilities. Cross-namespace
lookup, delivery, and transfer fail closed unless a live portal grant authorizes
one bounded operation.

Portal policy receives only source/target namespaces, transfer kind,
generation, bounded metadata, and lifetime. Runtime executors retain protocol
request context, data, handles, and UI effects. Denial maps to native protocol
failure and never freezes the session or fabricates input.

The complete admission, capability, grant-lifecycle, and first X11
clipboard/PRIMARY contract is in
[namespaces-and-portals.md](namespaces-and-portals.md).

## Runtime And Process Failure

Runtime adapters translate external readiness and completion into bounded
`SessionRuntimeObservation` values. The data-only session reducer may request
polling, WM policy, rendering, scanout, portal drain, chrome presentation, or
restart. It does not parse X11, read portal payloads, render, or own native file
descriptors.

Supervised components report bounded health state and generations. Runtime
policy owns restart delay and give-up decisions. Engine preserves committed
visual state while a WM or broker restarts. A broker restart revokes grants
whose executor continuity cannot be proven.

Logs are a boundary. Default diagnostics may contain opaque IDs, generations,
counts, outcomes, and timings. They must not contain raw XIDs, namespace IDs,
credentials, titles, PIDs, paths, payloads, icons, or buffer contents.

## Development Order

The native-X foundations, production session loop, paired software/GPU gates,
namespace and portal reference flows, native Hagia policy, and Firefox
daily-driver workload are complete. The session can explicitly install
Hagia through `sophia_wm_v1`: it binds and authenticates the endpoint before
spawn, sends complete Engine snapshots, stages complete projections in the
canonical reducer, and promotes them only with frontend/renderable-content
settlement. Policy replacement advances the connection epoch and preserves the
last committed layout while the replacement reconstructs its private state.
The removed API v7 transport cannot be selected or reached. New policy clients
must implement the public protocol directly; no compatibility transport or
legacy-WM profile is part of the product.

New X11 compatibility remains probe-driven; future application protocols are
outside the active roadmap. XLibre and Wayland stay archived unless a later
specification decision admits a bounded provider.

## Reference Boundaries

- Phoenix: clean-room modern X server strategy and evidence-driven coverage.
- XLibre: historical X11 delivery, namespace, and compatibility lessons.
- niri and the retired Smithay frontend: historical backend, renderer, and test
  lessons without adopting their compositor-policy architecture.
- picom: historical XComposite/Damage and buffer-lifetime lessons.
- river: external policy-protocol and crash-isolation lessons.
- macOS WindowServer/Core Animation: transaction-first presentation and
  fail-closed visual integrity.

The former Rust WM API v7 transport is removed. The durable public boundary is
the independently versioned `sophia_wm_v1` interface described in the
[Sophia Window Manager API](sophia-wm-api.md), inheriting the common contract
from the [Sophia Native Protocol Family](sophia-policy-ipc.md). Engine
owns physical shortcut matching, projection validation, visibility, focus
authority, and opaque
session actions. Native Sophia WMs consume this contract directly. Existing
window managers must port their policy to the language-neutral protocol.

---
id: zsx0tk4k
date: 2026-10-09
kind: adr
status: accepted
tags: [adr, architecture, protocol, security, namespaces, portals]
---
# Serve every public role from one 9P core with namespaces as composed trees and portals as binds

## Context

On 2026-10-09 Sophia's public interfaces stand as follows. Six run as exports
on the single 9P2000.L server core in `crates/sophia-9p`: the WM, shell,
output and lock roles, factotum and read-only inspection. Three still use the
legacy 24-byte socket envelope: administrative control through `sophia msg`,
the metadata broker and the portal decision owner. X11 applications use the X
authority. A 9P application authority exists as a scaffold with its own
protocol codec and a crate header promising service over FUSE mounts, and it
is not connected to the core.

Identity is established by three mechanisms: peer UID at socket admission for
X clients, a peer pidfd for the lock provider, and a factotum conversation for
unlock. Descriptors cross process boundaries by two patterns, DRI3 plane
descriptors read at the X socket and a proposed second endpoint for lock
images, with capture undecided. The bubblewrap policy both hides sockets from
a confined group and unshares the host, mixing reach within Sophia with reach
into the operating system.

The [accepted public-interface decision](1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
set the transport direction and left the filesystem contract, the namespace
API and the executor model open. The
[IPC removal inventory](../investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
found that the remaining legacy code is held in place by shared codecs,
default transport selections and the three unmigrated roles. niltempus asked
on 2026-10-09 how Plan 9's ideas should shape namespaces, portals and
portability without changing Engine, the X authority or the role protocols.
The [Plan 9 integration points concept](../concepts/ernn0bkv-plan-9-integration-points-for-sophia.md)
records that reasoning; this record states the resulting decision.

## Decision

There is one 9P server core, and every public role is an export on it. The
three roles on the legacy envelope migrate as exports under their existing
tasks, and the legacy envelope is retired once each role's replacement is
accepted. The application authority is built as an export on the same core;
its separate codec and its FUSE service claim are withdrawn before any
application API work. X11 remains its own frontend and is not a 9P interface.

The core serves 9P2000.L now. Plain 9P2000 is a later portability target, with
explicit adapters for open, stat, directory reads and errors; the existing .L
operation subset is not already common to both dialects. Role file semantics
stay the same where an adapter can preserve them. Error replies use Sophia's
error vocabulary mapped per dialect, never an unchecked host errno, because
9P2000.L defines its codes as Linux numbers regardless of host. The audit and
interoperability tests precede any plain-9P support claim.

A namespace is what a connection can reach: the role endpoints in its socket
directory and, on each endpoint, the tree its attach yields. The immutable
`ClientAdmissionContext` remains the single identity value for every
connection. The tree is derived from that context and the role grants by a
declarative recipe, and it is never stored or mutated by the client. The
server checks attach, walk, open and every operation on a retained handle.
Mount topology and host containment are conveniences for processes that need
paths; they never confer authority. The shared and confined session profiles
become two recipes rather than two code paths, with the capability set as the
recipe's portal section and host reach as a separate, per-platform, optional
section.

Identity is proven at attach. The first successful authenticated attach fixes
the connection's principal and namespace in its immutable admission context.
Later attaches must authenticate as that same principal and namespace and can
only obtain views within its admitted grants. A different identity requires a
different connection; no attach can rebase an existing fid's authority. The
afid conversation uses factotum, while the supervisor's launch record proves
custody of a child it actually spawned. Authentication alone proves no launch
custody. Kernel peer credentials become a platform-optional second check only
after the replacement's negative controls pass. X11 admission is unchanged.

Portals are the only edge between namespaces, and the recipient's frontend
executes them. Portal policy remains a deterministic reducer over bounded
facts, grants remain bound to source generation, and the portal export decides
without executing. A 9P recipient receives a bind of one object into its tree
for the grant's lifetime, and revocation makes the open fid return ESTALE. An
X recipient receives a translation into X semantics by the X authority. A
descriptor-bearing kind carries a record on the file and the descriptor on
that role's side channel, never on the 9P stream. Capture is the first new bind:
one named output, with a versioned frame record and bounded screenshot bytes
read over 9P from an immutable renderer-owned snapshot. GPU handles and
recording descriptors stay on the separate descriptor channel. Capture is
refused while locked; applying the lock cancels unfinished capture delivery.
Revocation prevents future reads and cannot recall bytes already delivered.

Composition is a per-identity socket directory. The recipe decides which
role endpoints exist in an identity's directory and writes one discovery file
listing them with their versions. Role trees stay one per endpoint, each
checked by its owner, and a portal bind lands in the tree of the role that
granted it. A single composing root that forwards into other owners' exports
is not built; it remains a later option if a measured need appears. This keeps
the reach rule of the transport investigation: endpoint reach, attach names,
fids and qids alone grant nothing.

Protocol authorities hook into Engine through one contract. The records the
session exchanges with the X authority today, admitted surface transactions,
buffer updates, allocation and metadata proposals, routed input with its
origin, configure, allocation and presentation outcomes, topology snapshots,
service commands and the injection policy, are named once in
`sophia-protocol` as the protocol authority contract. The X authority is its
first implementor with no behavior change, and the application authority its
second. Sophia's native path for window managers, shells, services and
applications is 9P on this core, and Sophia ships no other application
protocol authority; niltempus recorded on 2026-10-09 that a Wayland authority
is not planned. Other developers may add a protocol authority of their choice
out of process, as a 9P client of the application authority that carries its
own clients as sub-identities, each with an immutable namespace context. An
in-process implementation of the contract is possible but is not an extension
path Sophia maintains for third parties.

Only the live backend, the renderer, the narrow adapter crates and the
containment driver touch the operating system. The session selects a backend
by profile and never names one.

Engine, its internal typed transactions, the X authority, the WM and shell
file contracts and their binary records are unchanged by this decision.

Agents and other operator tools are ordinary admitted clients. Observer,
operator and driver are recipes of independent grants, not an inheritance
hierarchy. The observer reads status and inspection and may receive capture
grants on named outputs. The operator receives selected administrative ctl
and clipboard grants. The driver receives injection admission on one named
namespace; it gains neither capture nor administration implicitly. The default
observe-and-act workflow uses separate identities and connections. Combining
grants for one identity requires explicit operator policy; names such as
"driver" confer nothing. Sophia ships the observer recipe, and driving is
absent from the daily session unless a task admits it. Sophia builds nothing
agent-specific: no harness, named client or feature for one agent.

For 9P applications the driver tier's mechanism is an input file on the
admitted window, served by the application authority, bound into the driver's
tree by its grant and never visible to the application itself. Injected
events enter Engine's routing at the same seam as admitted XTEST injection,
so target resolution against presented state, capture, cancellation, the
lock refusal and revocation epochs apply unchanged, and the journal records
their provenance. The contract is designed under t320 before the application
authority gains an API.

```text
  X11 clients          9P clients: WM · shells · output · lock ·        third-party protocol
                       admin · broker · portal · applications · agents   clients (any protocol)
       │                               │                                        │
       ▼                               ▼                                        ▼
  X authority                 ONE 9P CORE: one export per role           translator process:
  in-process crate            9P2000.L and plain 9P2000                  a 9P client of the
                              application authority = an export          application authority
       │                               │                                        │
       └───────────────────────────────┴────────────────────────────────────────┘
                                       │
                 PROTOCOL AUTHORITY CONTRACT (records in sophia-protocol)
                 ingress: admitted surface transactions, proposals, metadata candidates
                 egress:  routed input with origin, outcomes, topology, revocation
                                       ▼
                   session owners: admission · routing · policy  ──▶  ENGINE  ──▶  DRM/KMS

   identity proven at attach · one socket directory per identity with a discovery file
   binds land in the granting role's tree · descriptors on a side channel per role
   OS touched only by backend, renderer, adapters and the containment driver
```

```text
                 attach (9P: afid + factotum)   admission (X: socket + peer)
                              │                          │
                              └──────────┬───────────────┘
                                         ▼
                           ClientAdmissionContext  (one per connection, immutable)
                             namespace.id · profile · capabilities · provenance
                                         │
            ┌────────────────────────────┼────────────────────────────┐
            ▼                            ▼                            ▼
   X authority keys every        9P core derives the tree      portal export bounds
   resource by namespace.id      tree = recipe(context, role    requests by capabilities,
   lookups fail closed across    grants); every walk, open      decides one grant per
   namespaces                    and retained fid re-checked    transfer

 ┌─────────────── namespace A: shared ──────────────┐  ┌──── namespace B: confined ────┐
 │  X terminal · trusted tools · 9P admin client    │  │  browser · chat · 9P app      │
 │  same service identities, whatever the path view │  │  own tree, zero capabilities  │
 └──────────────────────────┬───────────────────────┘  └──────────────┬────────────────┘
                            │          portal grant (the only edge)   │
                            └──────────────────┬──────────────────────┘
                                               ▼
                 execution by the recipient's frontend, never by policy:
                   9P recipient  → bind one object into its tree; revoke = ESTALE
                   X recipient   → translate into X semantics (selection, INCR, Xdnd)
                   fd-bearing    → record on the file, descriptor on the role's side channel
```

## Alternatives

Keeping administration, the broker and portals on the legacy envelope
permanently would leave two envelope families, duplicate codecs and two
admission paths in production. The IPC inventory shows that this is what holds
the remaining legacy code in place.

A separate 9P server for applications, which the current scaffold is, would
duplicate the wire codec and could not share admission, dialect handling or
the descriptor rule with the role exports.

Kernel mounts through v9fs or FUSE as the namespace mechanism were rejected on
the [host evidence](../investigations/5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
that unprivileged v9fs mounting fails, on the fact that no descriptor crosses
a mount, on the context-switch cost for frame-rate roles, and because the
server check is required in any case.

Delivering a GPU descriptor inside the 9P `Ropen` reply would put descriptors
on the stream, make mounted clients impossible and contradict the DRI3 seam
in which descriptors are read at the socket and never seen by pure dispatch.

Separate executors and endpoints per portal kind would create seven
mechanisms where a bind is one.

A draw-style content file for applications would wrap a command stream around
composition that Engine already owns.

## Consequences

One transport in two dialects, one identity model, one descriptor rule and one
containment split replace the parallel paths listed in the context. Each role
keeps its own contract, admission and authority; they share only the core.

The costs are a composition module in the session about the size of one
existing export, the attach authentication path and admission policy change,
launch custody records, the rewrite of the application authority scaffold as
an export, migration of three roles, and the per-dialect error mapping. The
estimate is low thousands of lines added, a few hundred changed and nothing
deleted in the first step.

The measurement rule in the
[public-interface design](../../sophia-9p-control-bus.md#performance-is-an-acceptance-question)
is unchanged: no legacy interface is retired until its replacement is measured
against it for the same workload. Authentication and composition act at
connect time; grants and capture act on demand. Capture can require GPU waits,
readback and copies, and shared resources can affect frame latency even when
Engine's ownership is unchanged. Bound that work and measure idle wakeups,
CPU, allocations, copied bytes and presentation latency with capture enabled.

This record promotes nothing into the critical lane. The open work maps onto
tasks in [todo.md](../../../todo.md), with the new rows owned by the
[convergence plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md):
t250 and t252 qualify the live 9P roles; t254 migrates administration; t314
and t315 serve the [broker](../../sophia-broker-files.md) and
[portal](../../sophia-portal-files.md) files whose contracts t273 designed;
t321 names the protocol authority contract and t316 rebuilds the
application authority scaffold as an export against it; t255 retires
the legacy envelope and its default selections; t133 and t256 cover the
admission review and the portability audit, and t317 proves identity at
attach; t142 and t275 cover group listeners and the recipe, and t318
implements the composition layer; t319 delivers the capture bind carved from
t046, and t045 the confined daily group; t257 adds status files. Physical
monitor recovery was accepted on release 222. The remaining t310 publication
and policy obligations precede treating output identities as a capture target;
the independent t307 QEMU/virgl investigation is parked.

The normative documents carry the accepted target, labeled as
unimplemented until each part lands: the protocol frontends and namespace
sections of [architecture](../../architecture.md), the target diagram in the
repository README, [namespaces and portals](../../namespaces-and-portals.md)
for composition and recipient execution, the
[public-interface design](../../sophia-9p-control-bus.md), and the
[application frontend design](../../sophia-9p-authority.md), which drops its
FUSE and separate-codec text.

## Acceptance and connections

niltempus separately approved the [monitor continuity slice](../plans/cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md#persistent-services-and-replaceable-display-attachments)
on 2026-10-09: persistent logical session state, replaceable physical attachment
generations, and admitted rediscovery while displays are absent. That approval
does not promote the remaining one-core, authentication, namespace or capture
tasks. Role connection epochs and display generations stay distinct; an old
inspection snapshot cannot authorize a current output operation. t257 owns the
later availability/status view, t318 the retained-handle binding rules, and t319
generation-bound capture and revocation. They do not block the physical recovery
candidate or change its current role protocols.

Proposed and accepted by niltempus on 2026-10-09 after review of this record
and ernn0bkv. Acceptance includes five clarifications: bounded screenshot
bytes may cross 9P while descriptors do not; attaches cannot change a
connection's identity; observer/operator/driver grants do not inherit;
t315's portal foundation does not require future clipboard delivery through
both application frontends before t319 capture; and unchanged Engine ownership
is not a performance guarantee. The [delivery plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order)
sets the first usable milestone and its gates. Approval establishes the target
architecture, not implementation or physical acceptance of these interfaces.

- [Plan 9 integration points](../concepts/ernn0bkv-plan-9-integration-points-for-sophia.md)
  holds the reasoning, the cost table and the seven further integration points.
- [Adopt 9P2000.L as the target public interface](1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
  is the accepted direction this record builds on and does not supersede.
- [Keep broker and portal file authority and custody separate](xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
  owns the portal authority boundary that recipient execution preserves.
- [Adopting the Plan 9 namespace model](../investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md)
  is the investigation whose exit this record answers in part.
- [9P2000.L profile](../../sophia-9p-profile.md) owns the served operation
  subset the dual-dialect core must keep.
- [Desktop role migration](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md),
  [socket directories and frontend groups](../plans/ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md)
  and [portals and confined applications](../plans/queue-16-portals-and-confined-applications.md)
  hold the tasks named above.

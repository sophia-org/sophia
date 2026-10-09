---
id: zsx0tk4k
date: 2026-10-09
kind: adr
status: proposed
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

The core serves 9P2000.L now and plain 9P2000 beside it when the portability
audit lands. Role contracts use only the operations both dialects share. Error
replies carry Sophia's own error vocabulary mapped per dialect, never the host
operating system's errno values, because 9P2000.L defines its codes as Linux
numbers regardless of host.

A namespace is the tree a connection sees at attach. The immutable
`ClientAdmissionContext` remains the single identity value for every
connection. The tree is derived from that context and the role grants by a
declarative recipe, and it is never stored or mutated by the client. The
server checks attach, walk, open and every operation on a retained handle.
Mount topology and host containment are conveniences for processes that need
paths; they never confer authority. The shared and confined session profiles
become two recipes rather than two code paths, with the capability set as the
recipe's portal section and host reach as a separate, per-platform, optional
section.

Identity is proven at attach. A 9P connection may authenticate each attach
through an afid conversation with factotum, and the supervisor's launch record
for a child it spawned establishes custody. Kernel peer credentials become a
platform-optional second check. X11 admission is unchanged, since X has no
attach to authenticate.

Portals are the only edge between namespaces, and the recipient's frontend
executes them. Portal policy remains a deterministic reducer over bounded
facts, grants remain bound to source generation, and the portal export decides
without executing. A 9P recipient receives a bind of one object into its tree
for the grant's lifetime, and revocation makes the open fid return ESTALE. An
X recipient receives a translation into X semantics by the X authority. A
descriptor-bearing kind carries a record on the file and the descriptor on
that role's side channel, never on the 9P stream. Capture is the first bind,
output-only, refused while the session is locked.

Only the live backend, the renderer, the narrow adapter crates and the
containment driver touch the operating system. The session selects a backend
by profile and never names one.

Engine, its internal typed transactions, the X authority, the WM and shell
file contracts and their binary records are unchanged by this decision.

```text
   X11 apps ──X11──▶ X authority ──┐
   9P apps  ──9P───▶ app authority ─┤  an export; no own codec, no FUSE
   WM · shell · output · lock       │
   admin · broker · portal          ├──▶ ONE 9P CORE ──▶ ENGINE ──▶ DRM/KMS
   inspection · factotum · capture  │       9P2000.L and plain 9P2000
   ──────────────9P files───────────┘
                     ▲
   identity proven at attach: afid + factotum + launch custody
   one tree per identity from a recipe; portals and delegation are binds
   descriptors on a side channel per role, named by a record on the file
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
against it for the same workload. The three rules act at connect time and at
operator cadence; they add no per-frame work and leave Engine's hot path
untouched.

This record promotes nothing into the critical lane. The open work maps onto
tasks in [todo.md](../../../todo.md), with the new rows owned by the
[convergence plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md):
t250 and t252 qualify the live 9P roles; t254 migrates administration; t314
and t315 serve the [broker](../../sophia-broker-files.md) and
[portal](../../sophia-portal-files.md) files whose contracts t273 designed;
t316 rebuilds the application authority scaffold as an export; t255 retires
the legacy envelope and its default selections; t133 and t256 cover the
admission review and the portability audit, and t317 proves identity at
attach; t142 and t275 cover group listeners and the recipe, and t318
implements the composition layer; t319 delivers the capture bind carved from
t046, and t045 the confined daily group; t257 adds status files. Monitor
recovery remains the active implementation priority.

On acceptance, the normative documents carry the target, labeled as
unimplemented until each part lands: the protocol frontends and namespace
sections of [architecture](../../architecture.md), the target diagram in the
repository README, [namespaces and portals](../../namespaces-and-portals.md)
for composition and recipient execution, the
[public-interface design](../../sophia-9p-control-bus.md), and the
[application frontend design](../../sophia-9p-authority.md), which drops its
FUSE and separate-codec text.

## Acceptance and connections

Proposed on 2026-10-09 by niltempus during the Plan 9 brainstorm. Acceptance
is pending review by niltempus and Codex and will be recorded here with its
basis.

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

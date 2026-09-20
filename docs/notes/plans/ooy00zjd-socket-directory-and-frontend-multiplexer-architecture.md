---
id: ooy00zjd
date: 2026-09-20
kind: plan
tags: [plan, architecture, security, session]
---
# Socket Directory and Frontend Multiplexer Architecture

## Scope and exit

This plan outlines the design of Sophia's static namespace socket allocation model, resolving the deployment complexity and socket sprawl of sandboxed application launching. 

By separating the filesystem paths from the display-server process lifecycles, Sophia achieves pristine, defense-in-depth isolation with zero-overhead process management.

The measurable exits for this architecture plan are:
1. **One process, several namespaces.** A confined client group runs inside
   the session process alongside the classic namespace, on its own listener,
   without a second session. (Revised from "zero process sprawl" -- see the
   review below: there is no sprawl today, there is an absence of the feature.)
2. **Clean Container Command Lines:** Confined application launch command lines remain identical across sandboxes, requiring exactly one standardized directory bind-mount.
3. **Hard Path Exclusion:** Confined applications are physically blocked from reaching or connecting to the main trusted socket path on the host.
4. **Copy and paste between namespaces works through the portal.** A
   portal-granted PRIMARY transfer completes across two listeners, proven by a
   test that today does not exist -- every cross-namespace test proves refusal.

## Task details

Refer to task `id:t141` in `todo.md`.

### Tier 1: Standardized Socket Directories
Sophia creates a single, standard runtime root directory on the host:
`/run/user/[uid]/sophia/`

Inside this folder, the session supervisor dynamically creates isolated, workspace-specific directories:
* `/run/user/[uid]/sophia/shared/X0` (for trusted, classic-shared apps)
* `/run/user/[uid]/sophia/confined-1/X0` (for sandboxed browser)
* `/run/user/[uid]/sophia/confined-2/X0` (for sandboxed viewer)

The Bubblewrap launcher mounts *only* the specific confined directory into the container's standard X11 socket path (`/tmp/.X11-unix/X0`), maintaining absolute pathname abstraction inside the sandbox:
`bwrap --bind /run/user/1000/sophia/confined-1 /tmp/.X11-unix --setenv DISPLAY :0 /usr/bin/firefox`

### Tier 2 as first proposed: an async multiplexer (superseded, kept for the record)
A single Sophia session supervisor process runs a `tokio` async event loop and binds standard `UnixListener`s to every active namespace socket path on the host on demand:
* `UnixListener::bind("/run/user/[uid]/sophia/shared/X0")`
* `UnixListener::bind("/run/user/[uid]/sophia/confined-1/X0")`

When a connection is accepted, the multiplexer matches the file descriptor to the specific listener, automatically tagging all downstream transactions on that stream with the correct `NamespaceId` and isolating their states in memory.

## Review, 2026-09-20

Read against the code and against `namespaces-and-portals.md` before anything
was built. The goal is right and the doc already mandates it. The plan reaches
it by the wrong mechanism, on a premise that misdescribes today, at a cost it
does not name.

**What is true today.** One session is one namespace, one listener, one
process: `--namespace-profile=classic|confined` is a flag on `session run`,
`XServerFrontend` holds a single `UnixListener` by construction
(`frontend/service.rs:11`), and the live session calls `create_namespace`
exactly once (`live_session.rs:618`). There is no tokio and no `async fn`
anywhere in the workspace. `bwrap` is invoked by nothing in `crates/`; it
appears only in `tools/` scripts and probes. So "subprocess-per-socket sprawl"
does not describe the tree. What is true is narrower and worth stating
plainly: **per-app confinement does not exist**, and the only way to get a
confined group is to run a second whole session on another display.

**The doc already anticipates this feature, by the opposite mechanism.**
`namespaces-and-portals.md` says "Separate group credentials on one listener
remain future supervisor work", and states the invariant "An X listener is
transport, not identity" -- identity comes from `ClientAdmissionContext`,
which `XServerFrontendAdmissionPolicy::admit` returns from peer credentials
and policy. The proposal tags "all downstream transactions on that stream with
the correct `NamespaceId`" from the listener alone, and never mentions
admission. That reverses a stated invariant without arguing for it.

**Tier 1 is the good idea.** A confined app bind-mounted one directory at the
standard path physically cannot reach the trusted socket. That is filesystem
isolation *under* credential admission -- defense in depth that composes with
t133's pidfd admission rather than competing with it -- and it needs no
runtime change. Two conditions: it must be written sandbox-agnostic, because
t135 is evaluating replacing bwrap and the proposal hardcodes bwrap's
semantics; and it must say what happens to the MIT-MAGIC-COOKIE (one per
session, shared across directories, is fine -- path exclusion is the barrier).

**Tier 2 conflates two things.** "Serve several listeners in one process" and
"introduce an async runtime" are independent, and the second is the expensive
one. The frontend is the most carefully reasoned concurrency in the repository
-- documented lock orders, the write-ahead join protocol, the custody-pin
lifetime discipline -- all synchronous and thread-shaped. Porting it to tokio
is a rewrite, not a feature. The same outcome is reachable in the loop that
already exists: `accept_next_concurrently_with_routing`
(`frontend/service.rs:268`) already does a nonblocking accept, and
`drive_routed_service` already polls it each turn. Iterating that accept over
N listeners is a small change. And there is a decisive reason to prefer it:
**the synchronous design keeps t138's premise and the async one breaks it.**
The idle-window reclaim is honest because one thread accepts every connection
and holds the frontend while it does. N listeners served by that same thread
leave that exactly true; N tasks on a runtime do not.

## Revised design

**Tier 1 stands** with the two conditions above.

**Tier 2 becomes: several listeners in the existing synchronous frontend, each
carrying a namespace candidate, with admission still deciding.**

- `XServerFrontend` holds a list of listeners rather than one, each paired
  with the namespace context it was bound for. The accept step iterates them,
  nonblocking, from the same loop thread it runs on now.
- `XServerFrontendAdmissionRequest` (`frontend_types.rs:112`) gains the
  identity of the listener that accepted the connection. The policy returns
  the `ClientAdmissionContext` for that listener's group -- namespace and
  capabilities, as the doc says confined policy must -- **after** checking
  peer credentials exactly as it does now. The listener proposes; admission
  disposes. "Transport, not identity" survives intact.
- The live session creates one namespace per configured group instead of one
  per session, and binds one listener per group under the Tier 1 directory
  layout.
- Nothing async. No new runtime, no new lock, no change to worker threads,
  workers' custody, or the service order.

## Implementation plan

Split into two rows, because the halves have different prerequisites and
different risk. Under the no-recycle rule the second takes the next identity
above every one ever used.

### Phase 0 -- prerequisites, before either half is built

1. **Decide t135, or make Tier 1 independent of it.** Write the directory
   contract as "one directory, mounted at `/tmp/.X11-unix` inside the
   sandbox, containing only that group's socket"; bwrap and unshare both
   satisfy it. Do not name bwrap in the contract.
2. **Prove a portal-granted transfer works across two listeners.** Every
   cross-namespace test in `tests/x11_wire/admission_frontend.rs` proves
   refusal (`..._reject_cross_namespace_window_property_and_selection_access`).
   Add its counterpart: two clients on two listeners, a PRIMARY transfer
   granted by the clipboard portal, the bytes arrive. Without this, shipping
   confined groups makes paste between sandboxes fail silently -- the exact
   class of bug t124 is. This is a real gap in the existing product, not just
   in this plan, and it can be written today against `clipboard.rs`'s
   `CrossNamespace` branch.

### Phase 1 -- t141: socket directories and path exclusion (Tier 1)

Small, no runtime change, real security value.

1. `live_session/startup.rs:163` creates `/tmp/.X11-unix` today. Add the
   layout: `$XDG_RUNTIME_DIR/sophia/<session>/shared/` and
   `.../confined-<n>/`, owner-only, created before any listener binds. The
   classic listener keeps its current path as well, for clients outside any
   sandbox, until Phase 2 moves it.
2. The launcher (`sophia-cli` `client_launch`) learns to bind-mount one group
   directory at the standard path when launching into a group.
3. **Proof of exit 3**, as a test: a client launched into a group directory
   cannot open the trusted socket path -- not "is denied", cannot reach it.
4. Cookie: one per session, published as today; document that path exclusion,
   not the cookie, is what separates groups.

Verification: the socket suite unchanged; a new launcher test for the mount;
`cargo xtask check layout` (touching `live_session` files near their ceilings
needs care -- check `wc -l` first).

### Phase 2 -- t142: several listeners, one synchronous frontend (Tier 2)

1. `frontend/service.rs`: `listener: UnixListener` becomes a small table of
   `(UnixListener, NamespaceContext, listener id)`. `bind` takes the table;
   `accept_next_concurrently_with_routing` iterates it, nonblocking, and
   `spawn_client_worker` is told which listener produced the stream.
2. `frontend_types.rs`: `XServerFrontendAdmissionRequest` gains
   `listener: XServerFrontendListenerId`. Built at
   `connection/dispatch.rs:472`, where the accepted stream's listener is now
   known.
3. Admission policy: `LiveXAdmissionPolicy` (`live_session.rs:629`) holds a
   map from listener to namespace context and returns that group's context
   after the existing credential check. A listener with no group is refused
   `Denied`, not defaulted -- the doc forbids inferring identity from a
   hardcoded namespace, and a default would be exactly that.
4. `live_session.rs:618`: `create_namespace` once per configured group.
   Configuration gains the group list; a session with no confined groups
   configures exactly what it does today, so the classic path is unchanged by
   construction.
5. **Re-state t138's premise and test it.** `reclaim_idle_departures` reads
   `active_client_worker_count()`, which now counts every listener's workers.
   The window is "no client on any listener", rarer but still stable, because
   the same thread accepts on all of them. Add a control: two listeners, a
   departure on one while the other has a live client, no reclaim; both
   idle, reclaim. This is the test the single-listener world could not write.
6. Registry and routing already key everything by `NamespaceId`; confirm with
   the existing confined socket suite run over two *listeners* rather than
   two sessions, which is the shape it was written to anticipate.

Verification: the full `sophia-x-authority` suite; the confined socket suite
over two listeners; the Phase 0 portal-transfer test now running in the real
frontend; the M3 and M5 acceptance gates unchanged (they run one listener and
must not notice); the ten-round t138 probe unchanged.

### Phase 3 -- live acceptance

One classic group and one confined group in one session on the installed
release: a confined browser cannot open the trusted socket, can paste into a
trusted terminal through the portal, and the session survives the confined
group's departure. Recorded as a milestone note with the gate reports beside
it, in the M-series format.

### Priority and sequencing

- **t141 (Phase 1): (A), after the t135 decision** or once the contract is
  written sandbox-agnostic, which can be done in an afternoon.
- **t142 (Phase 2): (B) until Phase 0's portal test exists**, then (A). It
  must not ship before that test, for the reason given.
- The tokio rewrite is not on either row. If someone wants an async frontend,
  that is its own proposal with its own argument, and "more listeners" is not
  it.

## What this changes in work already landed

Two standing conclusions rest on the fact that a namespace belongs to a
listener, and this plan moves that fact. Neither is a conflict today -- nothing
here is implemented -- but both must be revisited by whoever builds it.

**t138's idle-window reclaim assumes one accepting thread.** A departed
connection's continuation place is given back during the run by
`reclaim_idle_departures`, called from `drive_routed_service` when it is told
no connection frame is active. That reading is honest for exactly one reason:
the service frame is the only thread that starts a client worker, and it holds
the frontend exclusively while it does, so "zero now" is stable until it acts
again. **A tokio loop accepting on several listeners breaks that premise.**
The count could go from zero to non-zero between the reading and the mint, and
the token minted would then assert something false -- which matters because
eight of the nine consumers of that token depend on the quiesce it asserts.
The reclaim must be re-derived on the new threading model, not ported.

**t124's conclusion about selections is inverted by this plan.** A cross-client
selection works today partly because a display is one namespace: two X clients
on one socket always share one, so the clipboard portal is never on the path.
Per-namespace listeners make cross-namespace the ordinary case -- a confined
browser copying to a trusted editor is precisely two namespaces -- so the
`SameNamespace`, `UnknownRequestorNamespace` and `Portal` branches in
`clipboard.rs` become live for the first time, and the portal becomes load
bearing for ordinary copy and paste rather than an unexercised path. That is
what the portal is for; the point is that it moves from untested to
essential, and t124's evidence does not cover it.

## Connections

- Links to [PIDFD and Namespace Admission Optimizations](esnqxpqw-pidfd-and-namespace-admission-optimizations.md)
- Links to [Unprivileged Sandboxing Library Alternatives to Bubblewrap](../investigations/q2pdd8gn-unprivileged-sandboxing-library-alternatives-to-bubblewrap.md)
- Links to [Namespaces and Portals](../../namespaces-and-portals.md)

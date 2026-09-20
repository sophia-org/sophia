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
1. **Zero Process Sprawl:** A single Sophia supervisor process manages multiple isolated namespace connections simultaneously using a single async event loop (no subprocess-per-socket sprawl).
2. **Clean Container Command Lines:** Confined application launch command lines remain identical across sandboxes, requiring exactly one standardized directory bind-mount.
3. **Hard Path Exclusion:** Confined applications are physically blocked from reaching or connecting to the main trusted socket path on the host.

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

### Tier 2: Frontend Multiplexer in Rust
A single Sophia session supervisor process runs a `tokio` async event loop and binds standard `UnixListener`s to every active namespace socket path on the host on demand:
* `UnixListener::bind("/run/user/[uid]/sophia/shared/X0")`
* `UnixListener::bind("/run/user/[uid]/sophia/confined-1/X0")`

When a connection is accepted, the multiplexer matches the file descriptor to the specific listener, automatically tagging all downstream transactions on that stream with the correct `NamespaceId` and isolating their states in memory.

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

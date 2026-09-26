---
id: 5kqzwmi5
date: 2026-09-26
kind: investigation
status: investigating
tags: [session, architecture]
---
# Plan 9 belongs in the session control plane, not the engine

## Question

niltempus proposed Plan 9's architecture, rather than a Plan 9 operating
system, as a fit for Sophia. A circulated write-up went further: it cast Sophia
as a "Plan 9 desktop" on Linux, with the X authority as a compatibility bridge,
mount namespaces as the application sandbox and `draw(3)` as the engine's model.
Where does Plan 9 actually fit, and does Linux's in-kernel 9P client (v9fs)
help namespaces and portals?

## In a nutshell

Plan 9 is the model for **sophia-session's control plane**: administration,
observability and scripting served as a synthetic file tree over 9P2000.L.
It is not the model for Sophia Engine, and it does not displace X.

- **Engine stays untouched.** Sophia Engine remains a protocol-neutral
  transactional compositor over client buffers, damage and atomic commits. Its
  model is Core Animation's transaction, not libdraw's server-side drawing.
- **X stays the sole active application authority.** The control plane serves
  facts and admin actions and creates no surfaces, so it does not conflict with
  that rule. [`sophia-9p-authority`](../../sophia-9p-authority.md) remains a
  non-normative research stub.
- **First scope:** `control/` plus read-only `wm/snapshot` and `wm/events`,
  replacing `sophia msg`. Hagia keeps the binary `sophia_wm_v1` fast path and
  its formal bounds.
- **Per-role sockets are the capability.** Each role reaches only its own 9P
  socket, and the server's attach serves only that role's tree. Put
  WM-directed actions (`focus-next`, `toggle-fullscreen`) on `control`, so the
  WM-facing socket is read-only by construction.
- **The session serves; the sandbox mounts.** This extends the
  [socket-directory contract](../../namespaces-and-portals.md#socket-directories)
  unchanged. Unprivileged clients use `9pfuse` or a userspace 9P client; v9fs
  is an option only where a privileged mounter already exists.
- **Text files are still a protocol.** A line such as `0 0 960 1080` needs a
  documented grammar and versioning, even without generated bindings.

## Evidence

### v9fs cannot be mounted from an unprivileged user namespace

Observed September 26 on the development host, kernel `6.18.52_1`, with the
`9p` and `9pnet` modules loaded by niltempus through `sudo modprobe 9p`:

```text
unshare -Urm mount -t 9p -o trans=unix /nonexistent /mnt  ->  permission denied (exit 32)
unshare -Urm mount -t tmpfs none /mnt                     ->  mounted
```

The tmpfs control shows that the same user namespace can mount a filesystem
that allows it. The 9p refusal came before any socket lookup; a reachable mount
would have failed on the missing socket instead. A per-user session therefore
cannot mount its own tree through v9fs without a privileged helper. Sophia
should not add a setuid helper for that purpose.

### v9fs does not strengthen namespaces

Confined X clients already reach only their own group directory, so a v9fs
mount adds no reach isolation beyond an ordinary bind. It also weakens
identity: admission decides from peer credentials, but through v9fs the
server's peer is the mount, not the client process.

### Portals: one good fit

Portal policy stays a reducer over bounded facts; a filesystem can only be an
execution transport. File handoff fits, with Flatpak's FUSE document portal as
prior art. Plain-text clipboard access for scripts is plausible if it runs
through the same grant lifecycle. The rest fits poorly. Approval waits become
blocking syscalls with `Tflush` and `EINTR`. X selections need TARGETS
negotiation and lazy conversion. 9P cannot pass file descriptors. Synthetic
files need `cache=none` and report zero length.

### Claims rejected from the circulated write-up

- Mount namespaces do not isolate X clients from each other; namespace-keyed
  checks inside the X authority do.
- "100% application compatibility" is not established.
- Plan 9's decline had more causes than drivers: licensing until 2000, no web
  browser, weak POSIX support and an adequate Unix.
- The Wayland clipboard is `wl_data_device`; xdg-desktop-portal serves
  sandboxed applications.

Prior art for the control-plane use is wmii, which served its window manager as
a 9P tree driven by `wmiir` scripts.

## Finding and resolution

Adopt Plan 9 as the session control-plane idiom, served on per-role sockets, and
leave mounting to the sandbox. The "universal bus" framing and v9fs mount
examples in the [control-bus proposal](../../sophia-9p-control-bus.md) need
revision before that proposal becomes an ADR. Its §7 shell examples assume a
kernel mount the session cannot make.

## Validation and remaining work

This is a design conclusion and one host observation. Nothing is implemented,
and no task is admitted to `todo.md`. Before any promotion:

- Revise the control-bus proposal to per-role sockets and sandbox-chosen mounts.
- Specify the text grammar and versioning for `control/` and `wm/` files.
- Write a proposed ADR if the control plane is promoted to roadmap work.

## Connections

- The [control-bus proposal](../../sophia-9p-control-bus.md) is the design this
  note narrows.
- The [namespaces and portals contract](../../namespaces-and-portals.md) supplies
  the socket-directory and portal rules the conclusion reuses.
- The [9P frontend proposal](../../sophia-9p-authority.md) stays a separate
  research candidate; this note does not promote it.

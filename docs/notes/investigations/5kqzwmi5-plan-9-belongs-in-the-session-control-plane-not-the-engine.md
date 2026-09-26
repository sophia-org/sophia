---
id: 5kqzwmi5
date: 2026-09-26
kind: investigation
status: investigating
tags: [session, architecture]
---
# 9P replaces public role IPC; unprivileged v9fs mounting was unavailable

## Question

niltempus proposed Plan 9's architecture, rather than a Plan 9 operating
system, as a fit for Sophia. A circulated write-up went further: it cast Sophia
as a "Plan 9 desktop" on Linux, with the X authority as a compatibility bridge,
mount namespaces as the application sandbox and `draw(3)` as the engine's model.
Where does Plan 9 actually fit, and does Linux's in-kernel 9P client (v9fs)
help namespaces and portals?

## In a nutshell

Plan 9 informs Sophia's **public role interfaces**. 9P2000.L progressively
replaces the separate WM, shell and administrative IPC protocols while the
existing role owners retain their semantics and authority. Engine's internal
typed transactions, rendering and device interfaces stay unchanged.

This note initially proposed administration and observation only, without
accounting for the accepted Hagia-first work. On September 26 niltempus
reaffirmed that replacing the separate public role protocols is the point of
the migration. The [accepted ADR](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
and [execution plan](../plans/80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md)
govern that scope; the earlier read-only WM restriction is withdrawn. The host
mount observation below remains useful and does not require changing transport.

- **Engine stays untouched.** Sophia Engine remains a protocol-neutral
  transactional compositor over client buffers, damage and atomic commits. Its
  model is Core Animation's transaction, not libdraw's server-side drawing.
- **X stays the sole active application authority.** The WM migration creates
  no application surfaces. [`sophia-9p-authority`](../../sophia-9p-authority.md)
  remains a separate future frontend; this work does not activate it.
- **First scope:** the admitted Hagia WM role over direct Unix-socket
  9P2000.L, including complete binary snapshots, proposals and outcomes.
  Current IPC remains the default and explicit rollback choice during
  validation. It is not retained as a permanent compiled-client fast path.
- **Per-role sockets preserve reach boundaries.** Session still authenticates
  the supervised protected peer and authorizes operations, including retained
  handles. Socket reach, attach names, fids and qids alone grant no authority.
  The admitted WM may submit bounded proposals through its role contract;
  administrative actions remain owned by the administrative role.
- **The session serves; the sandbox mounts.** This extends the
  [socket-directory contract](../../namespaces-and-portals.md#socket-directories)
  unchanged. Unprivileged clients use `9pfuse` or a userspace 9P client; v9fs
  is an option only where a privileged mounter already exists.
- **File contents are still a protocol.** Hagia uses compact binary runtime
  records with documented bounds and independent codecs. Readable inspection
  can be derived from those records. Text interfaces also require a grammar
  and versioning; they do not replace semantic validation.

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
negotiation and lazy conversion. 9P does not natively transfer file descriptors.
Cache and length rules must follow each file's contract: the current WM export
reports exact pinned object sizes and retained journal bounds. A mounted client
cannot infer the right caching policy merely from the word "synthetic".

### Claims rejected from the circulated write-up

- Mount namespaces do not isolate X clients from each other; namespace-keyed
  checks inside the X authority do.
- "100% application compatibility" is not established.
- Plan 9's decline had more causes than drivers: licensing until 2000, no web
  browser, weak POSIX support and an adequate Unix.
- The Wayland clipboard is `wl_data_device`; xdg-desktop-portal serves
  sandboxed applications.

### wmii: a concrete filesystem interface reference

On September 26 niltempus pointed to the local `~/src/wmii` checkout. Read-only
inspection covered revision `4cae1dc7e8ae2f7a603ab3565493e035f054a3f4` from
`0intro/wmii`; no build, installation or desktop run was performed.

The useful reference is the exposed interface, not a disk filesystem:

- `man/wmii.man1` (Filesystem/Hierarchy) describes a wholly synthetic in-memory
  tree: `/ctl`, `/client/*/ctl`, `/tag/*/ctl`, bar files and `/event`.
- `cmd/wmii/fs.c` maps those paths to live reads, command handlers and pending
  event reads. Files represent operations and current state; no backing disk
  file is required.
- `man/wmiir.man1` and `cmd/wmiir.c` provide a small direct 9P client with
  `ls`, `read`, `write` and `xwrite`. Kernel mounting is optional.
- `doc/customizing.tex` consumes line-oriented events in an ordinary shell
  loop. This makes the interface useful outside its compiled primary client.

For Sophia, this supports readable derived views, discoverable role paths and
a small direct client as interface goals. The captured Snapshot/event inspector
is an initial diagnostic tool, not wmii-style live scripting. A live reader must
still have its own admitted observation contract without consuming the WM's
ACK floor or granting writer authority. Binary atomic candidates and explicit
semantic settlement remain the WM contract; text readability does not replace
them.

wmii also combines WM policy, direct X access and bar control. Sophia keeps
those authorities separate and does not copy wmii's client metadata exposure,
mutable control vocabulary or broadcast-event authority. Its libixp client has
not been verified against Sophia's strict 9P2000.L endpoint. This source reading
establishes design precedent, not dialect interoperability, performance or
security acceptance.

## Finding and resolution

Use 9P at the public role boundary, served on separately admitted endpoints,
and leave optional mounting to the sandbox. The accepted
[public-interface design](../../sophia-9p-control-bus.md) already preserves
Engine ownership and direct clients. Neither v9fs nor FUSE is required by the
Hagia implementation. The inability to mount v9fs unprivileged therefore does
not block the approved migration or justify a permanent IPC/9P split.

## Validation and remaining work

The v9fs result is one host observation, not a portable kernel guarantee. The
Hagia-first development branches already contain an independent Nim client,
Session export and opt-in launch selection, with real-Hagia settlement and
restart controls. Their evidence and remaining acceptance gates are recorded
in the [typed-driver investigation](uf2wya88-typed-wm-driver-preserves-current-ipc-phase-and-shutdown-ownership.md).
This correction creates no new task, changes no default, and authorizes no live
session or physical-device test. Other role migrations remain later milestones.

## Connections

- The [public-interface design](../../sophia-9p-control-bus.md) owns the accepted
  migration direction; this note records the mounting constraint.
- The [namespaces and portals contract](../../namespaces-and-portals.md) supplies
  the socket-directory and portal rules the conclusion reuses.
- The [9P frontend proposal](../../sophia-9p-authority.md) stays a separate
  research candidate; this note does not promote it.

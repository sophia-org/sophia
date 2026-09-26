# Proposal: Sophia Universal 9P Control Bus

**Role:** architecture proposal and unified IPC evaluation.

**Status:** proposal and discussion draft; non-normative.

The [control-plane investigation](notes/investigations/5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
narrows this proposal: per-role sockets, mounts chosen by the sandbox, and no
unprivileged v9fs mount. Revise the sections below before any ADR.

## 1. Executive Summary

Sophia currently exposes its desktop roles—window management policy, desktop
shells, and host administration—through bespoke, role-specific IPC protocols
(`sophia_wm_v1`, `sophia_shell_v1`, `sophia_control_v1`). Each protocol uses a
custom 24-byte binary envelope, KDL schema definitions, generated wire tables,
and role-specific language bindings.

This document proposes **unifying Sophia's entire desktop control and policy
plane into a single synthetic filesystem served over 9P2000.L**.

Instead of treating the desktop control plane as a collection of specialized
socket endpoints, `sophia-session` exposes a single, capability-partitioned
directory tree. Policy daemons, status bars, launchers, and administrative
scripts interact with the desktop exclusively using standard filesystem
primitives (`open`, `read`, `write`, `close`).

```text
 ┌────────────────────────────────────────────────────────────────────────────┐
 │                               SOPHIA ENGINE                                │
 │             (Unmodified protocol-neutral visual kernel)                    │
 └─────────────────────────────────────▲──────────────────────────────────────┘
                                       │ (Internal Rust transactions & channels)
 ┌─────────────────────────────────────┴──────────────────────────────────────┐
 │                      SOPHIA SESSION SUPERVISOR                             │
 │   Hosts the In-Memory 9P Universal Control Bus at $XDG_RUNTIME_DIR/sophia/ │
 └───────────────────┬─────────────────┬───────────────────┬──────────────────┘
                     │                 │                   │
    bwrap mount /wm  │bwrap mount/shell│bwrap mount/control│bwrap mount/portals
                     ▼                 ▼                   ▼                  ▼
              ┌─────────────┐   ┌─────────────┐     ┌─────────────┐    ┌─────────────┐
              │  WINDOW     │   │  DESKTOP    │     │ SCRIPTING & │    │  PORTAL     │
              │  MANAGER    │   │  SHELL      │     │ AUTOMATION  │    │  CLIENTS    │
              │  (Hagia/rc) │   │  (Narthex)  │     │ (CLI/Bash)  │    │  (Snarf)    │
              └─────────────┘   └─────────────┘     └─────────────┘    └─────────────┘
```

---

## 2. Motivation: The Cost of Bespoke Role Protocols

While Sophia's native protocol family (`sophia-policy-ipc`) achieved clean
process boundaries, maintaining distinct wire protocols for each desktop role
imposes ongoing structural costs:

1. **Protocol Boilerplate and Tooling Overhead:**
   Every new event, field, or configuration option requires updating `.kdl`
   schemas, regenerating wire tables with `sophia-policy-protocol-gen`, updating
   Rust macros, regenerating Nim/C bindings, and maintaining golden wire corpora.
2. **The SDK and Language Barrier:**
   To write a window manager or panel today, developers cannot simply write a
   script; they must link against generated codecs, understand the 24-byte
   binary envelope, and handle low-level asynchronous framing.
3. **Application-Space Confinement Checks:**
   Isolating the Window Manager from seeing client metadata currently requires
   strict validation code inside the session supervisor to ensure the WM socket
   never leaks prohibited messages.
4. **Ad-Hoc Tooling:**
   Interacting with the session from the terminal required building a bespoke
   command-line client (`sophia msg`) with custom subcommand parsing.

By contrast, the Plan 9 model—representing system resources as synthetic
filesystems served over 9P—solves all four challenges simultaneously.

---

## 3. Interface Naming and Wire Specification

Following Sophia's normative naming conventions (as defined in
`docs/sophia-policy-ipc.md`):

- **Interface Role Name:** **`sophia_vfs_v1`** (major 1, revision 1).
- **Wire Protocol Standard:** **`9P2000.L`** (Linux VFS native extension).
- **Fallback Dialect:** **`9P2000`** (Plan 9 legacy compatibility).
- **Transport Endpoint:** Local Unix domain stream socket at
  `$XDG_RUNTIME_DIR/sophia/bus.9p`.
- **Mount Target:** `/dev/sophia/` or `$XDG_RUNTIME_DIR/sophia/fs/`.

### Why `9P2000.L` is the Normative Wire Dialect

While classic `9P2000` was designed for Bell Labs Plan 9, `9P2000.L` was created
specifically to integrate natively with the Linux kernel Virtual File System
(VFS):

1. **Native In-Kernel Mounting (`v9fs`):**
   Linux includes native 9P client support (`CONFIG_NET_9P`). The kernel driver
   defaults to `version=9p2000.L`. This allows Sophia's synthetic desktop tree
   to be mounted directly into the Linux VFS without userspace FUSE overhead:
   ```bash
   mount -t 9p -o trans=unix,version=9p2000.L "$XDG_RUNTIME_DIR/sophia/bus.9p" /dev/sophia
   ```
2. **POSIX Linux `errno` Codes (`Rlerror`):**
   Instead of arbitrary English strings (`Rerror { ename: String }`), `9P2000.L`
   returns standard Linux numeric error codes (`Rlerror { ecode: u32 }`), mapping
   directly to `ENOENT`, `EACCES`, `EAGAIN`, and `EBUSY`.
3. **Direct VFS Operation Mapping:**
   `9P2000.L` introduces operations that align 1:1 with Linux filesystem
   semantics: `Tlopen`, `Tlcreate`, `Tgetattr`, `Tsetattr`, `Treadlink`,
   `Tmkdir`, `Tunlinkat`, and `Tstatfs`.

### Dialect Negotiation
During the initial handshake (`Tversion`), `sophia-session` supports dual
negotiation:
- If a modern client or the Linux kernel requests `9P2000.L`, the session
  serves the Linux VFS dialect.
- If a legacy Plan 9 tool (`acme`, `sam`, `plan9port`) requests `9P2000`, the
  session falls back to classic string-error responses.

---

## 4. The Unified Desktop VFS Hierarchy

`sophia-session` serves the following in-memory synthetic filesystem hierarchy
at `$XDG_RUNTIME_DIR/sophia/`:

```text
$XDG_RUNTIME_DIR/sophia/
├── wm/                         # [Role: Window Management Policy]
│   ├── snapshot                # Read: current epoch, output heads, and surface nodes
│   ├── events                  # Read: blocking stream of layout events (SurfaceCreated, etc.)
│   ├── client/<id>/
│   │   ├── geom                # Write: "0 0 960 1080" (staged placement proposal)
│   │   ├── state               # Write: "tiled", "floating", "fullscreen"
│   │   └── props               # Read: current geometry and state flags
│   └── ctl                     # Write: "begin <epoch>", "commit" (triggers atomic DRM flip)
│
├── shell/                      # [Role: Desktop Chrome & Descriptors]
│   ├── reservation             # Read/Write: edge struts (e.g., "top 32", "bottom 0")
│   ├── descriptors             # Write: declarative UI widget trees (Narthex)
│   ├── candidates              # Write: ordered switcher/launcher candidate list
│   └── actions                 # Read: blocking stream of user selection activations
│
├── control/                    # [Role: Host Administration & Scripting]
│   ├── status                  # Read: active heads, refresh rates, running namespaces
│   ├── restart                 # Write: "wm", "shell", "x-authority"
│   ├── reload                  # Write: "config"
│   └── metrics                 # Read: frame timing distributions and present latency
│
└── portals/                    # [Role: Cross-Namespace Data Brokers]
    ├── snarf                   # Read/Write: brokered clipboard exchange
    ├── open                    # Write: URI and file handoff requests
    └── capture                 # Read: authorized screencast / damage streams
```

---

## 5. VFS-Native Sandboxing via Kernel Mount Namespaces

The core architectural invariant of Sophia is that **the Window Manager must be
blind** and **the Desktop Shell must not dictate window management policy**.

Under the 9P Universal Control Bus, role confinement is no longer enforced by
application-level token validation. Instead, **it is enforced by the Linux
kernel VFS via mount namespaces**:

```text
 ┌────────────────────────────────────────────────────────┐
 │ BUBBLEWRAP SANDBOX: WINDOW MANAGER (e.g. Hagia)        │
 │                                                        │
 │   • Read-only root filesystem                          │
 │   • Private network namespace (no internet)            │
 │   • Mount: bind /run/.../sophia/wm -> /dev/sophia/wm   │
 │                                                        │
 │   Sees ONLY: /dev/sophia/wm/{snapshot, events, client} │
 │   Cannot see: /shell, /control, /portals               │
 └────────────────────────────────────────────────────────┘
```

- When `sophia-session` supervises the Window Manager, Bubblewrap bind-mounts
  **only** the `wm/` subdirectory into the WM's container. The Window Manager
  physically cannot read `/shell`, `/control`, or `/portals`.
- When it supervises the Desktop Shell (`Narthex`), Bubblewrap mounts **only**
  `shell/`. The shell has no mechanism to query or alter window placement.
- Confinement fails closed at the kernel system-call level (`ENOENT`), eliminating
  an entire class of permission-checking logic from application code.

---

## 6. Transaction Atomicity and Epoch Synchronization

The fundamental requirement of window management is **visual atomicity**: when a
layout changes, all repositioned surfaces must commit on the exact same display
refresh frame without intermediate tearing.

In a filesystem interface where individual attributes are updated via `write()`,
atomicity is preserved through **transaction staging**:

```text
 Window Manager                                 sophia-session (9P Server)
       │                                                    │
       │─── Tread on /wm/events ───────────────────────────►│
       │◄── Rread: "epoch 104; SurfaceCreated 3 800 600" ───│
       │                                                    │
       │─── Twrite /wm/ctl: "begin 104" ───────────────────►│ (Locks transaction context)
       │─── Twrite /wm/client/1/geom: "0 0 960 1080" ──────►│ (Stages window 1 placement)
       │─── Twrite /wm/client/3/geom: "960 0 960 1080" ────►│ (Stages window 3 placement)
       │─── Twrite /wm/ctl: "commit" ──────────────────────►│
       │                                                    │──► Submits single atomic
       │◄── Rwrite: "ok" ───────────────────────────────────│    SurfaceTransaction batch
                                                                 to Sophia Engine for scanout
```

- **Rejection Semantics:** If a physical hotplug event or client exit occurs
  while the WM is calculating, the current epoch advances. When the WM writes
  `"commit"`, the 9P server returns `Rerror: "epoch stale"`.
- The Engine never receives partial layout proposals; visual integrity remains
  absolute.

---

## 7. Developer Experience: True Language Pluralism

With 9P as the Universal Control Bus, writing desktop components requires zero
SDKs, zero compiler toolchains, and zero foreign-function interfaces:

### Example A: A Dynamic Tiling Window Manager in Pure Bash
```bash
#!/usr/bin/env bash
WM="/dev/sophia/wm"

while read -r event id w h; do
    if [ "$event" = "SurfaceCreated" ]; then
        # Tile newest window to left, previous to right
        echo "begin" > "$WM/ctl"
        echo "0 0 960 1080" > "$WM/client/$id/geom"
        echo "commit" > "$WM/ctl"
    fi
done < "$WM/events"
```

### Example B: Host Administration Without Custom CLI Tools
Standard Unix coreutils replace `sophia msg`:
```bash
# Query active display heads and refresh rates
cat /dev/sophia/control/status

# Trigger an immediate hot-reload of desktop configuration
echo "config" > /dev/sophia/control/reload

# Supervised restart of the window manager
echo "wm" > /dev/sophia/control/restart

# Query presentation latency metrics
cat /dev/sophia/control/metrics
```

---

## 8. Performance and Wire Efficiency

A common concern with filesystem-based IPC is throughput and latency:

1. **Wire Framing:** 9P2000.L is a binary protocol over local Unix domain
   sockets. Every message has a 7-byte header (`size[4] type[1] tag[2]`). It has
   lower serialization overhead than KDL or JSON.
2. **Latency:** A `read()` or `write()` over a local Unix domain socket is an
   in-memory kernel buffer transfer taking single-digit microseconds—far below the
   16.6ms threshold of a 60Hz frame or 4.1ms of a 240Hz frame.
3. **High-Frequency Input Pacing:** For interactive mouse dragging, `/wm/events`
   uses a lockless, non-allocating circular ring buffer in `sophia-session`,
   ensuring high-refresh pointer motion does not trigger heap churn.

---

## 9. Dual-Stack Coexistence: Fast-Path Binary + Declarative VFS

Rather than viewing the 9P Control Bus as an all-or-nothing replacement for
`sophia-policy-ipc`, Sophia can support both paradigms **living side-by-side**.

Operating systems have utilized this hybrid model for decades: the Linux kernel
provides high-performance binary system calls (`epoll`, `ioctls`, `netlink`) for
compiled system daemons, while simultaneously serving `/proc` and `/sys` as
synthetic filesystems for observability, administrative tuning, and shell
automation.

```text
 ┌────────────────────────────────────────────────────────────────────────────┐
 │                               SOPHIA ENGINE                                │
 │             (Unmodified protocol-neutral visual kernel)                    │
 └─────────────────────────────────────▲──────────────────────────────────────┘
                                       │ (Internal Rust transactions & channels)
 ┌─────────────────────────────────────┴──────────────────────────────────────┐
 │                      SOPHIA SESSION SUPERVISOR                             │
 ├─────────────────────────────────────┬──────────────────────────────────────┤
 │ PATH A: Native Binary Socket        │ PATH B: Synthetic 9P Filesystem      │
 │ ($XDG_RUNTIME_DIR/sophia/wm.sock)   │ ($XDG_RUNTIME_DIR/sophia/fs/)        │
 └──────────────────┬──────────────────┴──────────────────┬───────────────────┘
                    │                                     │
                    ▼ (24-byte envelope / KDL)            ▼ (open, read, write)
             ┌─────────────┐                      ┌─────────────┐
             │    HAGIA    │                      │ SHELL / CLI │
             │  (Nim WM)   │                      │ (Bash/Python│
             └─────────────┘                      └─────────────┘
          • Sub-millisecond math                 • Observability (cat status)
          • Formal Z3/TLA+ bounds                • Automation (echo reload)
          • 144Hz/240Hz mouse drag               • Simple custom widgets
```

### Division of Responsibility in a Hybrid Desktop

1. **Path A: The Compiled Fast-Path (`sophia-policy-ipc`):**
   - **Role:** Dedicated to compiled, high-performance window managers (`Hagia`)
     and latency-critical descriptor shells.
   - **Strengths:** Formally verified by Z3 SMT arithmetic bounds and TLA+
     temporal transition models; sub-millisecond placement calculation;
     zero string parsing; fluid 144Hz/240Hz continuous pointer drag tracking.
2. **Path B: The Declarative VFS Plane (9P Control Bus):**
   - **Role:** Dedicated to administrative automation, terminal scripting,
     system observability, and external status bars.
   - **Strengths:** Zero SDK burden; kernel-enforced mount confinement; natural
     terminal inspection (`cat /dev/sophia/control/status`).

### Single-Writer Authority Arbitration

Sophia enforces the single-writer invariant so two entities never fight over
surface coordinates simultaneously:

- **When a binary WM (`Hagia`) is active:**
  - Hagia retains exclusive ownership over spatial geometry proposals.
  - The 9P `/wm/` tree operates in **observability mode**: `/wm/snapshot` and
    `/wm/events` remain fully readable, while `/wm/ctl` accepts high-level
    actions (`focus-next`, `toggle-fullscreen`) that the session supervisor
    dispatches to the active WM.
- **When no binary WM is connected:**
  - The 9P `/wm/` tree unlocks full layout authority (`/wm/client/<id>/geom` and
    `begin`/`commit`), allowing a Plan 9 `rc` script or Python daemon to serve
    as the primary window manager.

---

## 10. Migration and Coexistence Strategy

Adopting the 9P Universal Control Bus does not require a risky flag-day rewrite.
The architecture allows a clean, phased transition:

1. **Phase 1: Dual-Stack Session Support:**
   `sophia-session` embeds the lightweight `sophia-9p-authority` synthetic tree
   implementation and exposes the 9P endpoint alongside the existing
   `sophia_wm_v1` socket.
2. **Phase 2: Administrative Control Migration:**
   Migrate `sophia-control-v1` to `/control/`, replacing `sophia msg` with standard
   file operations.
3. **Phase 3: Optional Policy Retargeting:**
   Retarget `Narthex` (descriptor shell) to `/shell/` and evaluate whether
   `Hagia` benefits from a native 9P client or continues using the fast-path
   binary socket.
4. **Phase 4: Selective Deprecation:**
   Deprecate only the bespoke administrative protocols while retaining the
   binary fast-path where formal mathematical bounds are required.

---

## 11. Conclusion

The 9P Universal Control Bus fulfills the core promise of Sophia: **applying the
Unix philosophy directly to the modern graphical desktop**.

By treating the desktop control plane as a synthetic filesystem, Sophia achieves
unrivaled developer accessibility, bulletproof kernel-level confinement, and
drastically simplified codebase maintenance without compromising on frame
timing, visual atomicity, or performance.

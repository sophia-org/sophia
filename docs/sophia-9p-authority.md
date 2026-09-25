# Sophia 9P Filesystem Frontend (`sophia-9p-authority`)

**Role:** subsystem architecture and target contract.

**Status:** proposed frontend architecture; non-normative.

## 1. Overview

The Sophia 9P Filesystem Frontend is a proposed protocol authority that exposes
graphical window creation, drawing operations, and user input as a **synthetic
filesystem** served over the Plan 9 filesystem protocol (9P2000 / 9P2000.L).

Traditional graphics platforms (X11, Wayland) require client applications to link
against complex protocol serialization libraries (`libX11`, `libwayland-client`,
`xkbcommon`, `cairo`). Plan 9 from Bell Labs proved that a synthetic filesystem
interface (`/dev/draw`, `/dev/mouse`, `/dev/cons`) provides a complete,
interactive, and network-transparent graphical interface using standard file I/O
primitives (`open`, `read`, `write`, `close`).

In Sophia's decoupled architecture, `sophia-engine` serves as a protocol-neutral
visual kernel consuming anonymous visual transactions (`SurfaceTransaction`).
The 9P frontend terminates the filesystem protocol, translates file operations
into anonymous surface damage and buffer commits, and maps engine input back into
readable synthetic files.

```text
  [ Acme / Sam ]         [ Shell / Python Script ]       [ C / Go Minimalist App ]
         │                           │                               │
         └─────────────┬─────────────┴───────────────────────────────┘
                       │
                       ▼ 9P2000 / FUSE / v9fs (open, read, write)
        ┌────────────────────────────────────────────────────────┐
        │      SOPHIA 9P FRONTEND (`sophia-9p-authority`)        │
        │  • Synthetic directory trees (/dev/sophia/draw/<id>/)  │
        │  • Plan 9 libdraw decoder & raw pixel stream parser    │
        │  • Mouse/Kbd serialization from routed engine events   │
        └──────────────────────────┬─────────────────────────────┘
                                   │
                                   ▼ SurfaceTransaction (anonymous buffers & damage)
        ┌────────────────────────────────────────────────────────┐
        │                 SOPHIA ENGINE: VISUAL KERNEL           │
        │  • DRM/KMS atomic page-flips & plane scheduling        │
        │  • Target-resolved physical input routing              │
        └──────────────────────────┬─────────────────────────────┘
                                   │
                                   ▼ sophia_wm_v1 (opaque spatial nodes)
        ┌────────────────────────────────────────────────────────┐
        │       WINDOW MANAGER POLICY (Hagia / Reference WM)     │
        └────────────────────────────────────────────────────────┘
```

---

## 2. Naming and Architectural Boundary

- **Component Name:** Sophia 9P Filesystem Frontend
- **API / Protocol:** 9P2000.L / Plan 9 `draw(3)`
- **Target Crate:** `sophia-9p-authority`

### The Frontend Owns:
- 9P protocol parsing (over Unix domain sockets, TCP, or FUSE mounts).
- Synthetic directory hierarchy and dynamic file-node lifecycle.
- Plan 9 `libdraw` command parsing (`allocimage`, `draw`, `line`, `string`,
  `freeimage`) and software rasterization into backing CPU buffers.
- Raw RGBA pixel stream parsing for zero-dependency scripting.
- Virtual input files (`mouse`, `kbd`) formatting from engine input events.
- Emitting `SurfaceTransaction` batches, damage rects, and buffer updates to
  `sophia-engine`.
- Mapping client session identity to immutable `NamespaceContext` allocations.

### The Frontend Must Not Own:
- Physical DRM/KMS modesetting, scanout, or hardware planes.
- Compositor scene graph, damage accumulation, or global hit-testing.
- Spatial layout or focus policy (delegated metadata-blindly to `sophia_wm_v1`).
- Cross-namespace portal handoffs (mediated exclusively by `sophia-portal`).
- Physical input device handling or hardware translation.

---

## 3. Storage and Memory Model: Zero Disk I/O

A common misconception is that a filesystem-based display server incurs physical
disk writes or degrades solid-state drive (SSD) endurance.

**The Sophia 9P filesystem is entirely synthetic and lives exclusively in RAM:**
1. **Volatile Kernel Memory:** Like Linux's `/proc` and `/sys`, files under the
   9P authority have no physical backing blocks on an ext4, btrfs, or NVMe
   filesystem.
2. **Standard Memory IPC:** When an application calls `write()` to stream pixel
   data or draw commands, bytes move directly through kernel memory buffers
   into the frontend process's memory space.
3. **Hardware Impact:** Total Bytes Written (TBW) to physical storage is zero.
   Streaming high-framerate animation through the synthetic filesystem exerts the
   exact same system load and memory bus usage as writing to a local Unix stream
   socket or anonymous pipe.

---

## 4. Synthetic Filesystem Hierarchy

The frontend mounts or serves a directory tree under the user's runtime directory
(e.g., `$XDG_RUNTIME_DIR/sophia/draw/` or `/dev/sophia/draw/`):

```text
$XDG_RUNTIME_DIR/sophia/draw/
├── new                     # Open/read allocates a fresh window ID (<id>)
└── <id>/
    ├── ctl                 # Write: configuration commands; Read: state/geometry
    ├── data                # Write: draw command stream or raw pixel bytes
    ├── refresh             # Read: blocks until damage/repaint is needed
    ├── mouse               # Read: blocking stream of pointer events
    ├── kbd                 # Read: blocking stream of keyboard events
    └── text                # Bidirectional raw terminal text stream (optional)
```

### File Operations and Semantics

#### `new`
Opening or reading `new` triggers the authority to allocate a fresh surface
identity, register a new `SurfaceId` with `sophia-engine`, and create a
matching subdirectory `<id>/`.

#### `<id>/ctl`
Accepts text-based control commands to configure surface parameters:
```text
size <width> <height>       # Sets backing buffer dimensions (e.g. "size 800 600")
title <utf8-string>         # Sets surface title hint for the broker/shell
mode <raw|libdraw>          # Toggles data parser between raw RGBA and libdraw
format <argb8888|xrgb8888>  # Configures pixel format
fullscreen <0|1>            # Requests fullscreen toggle via WM policy
```
Reading `ctl` returns current geometry, DPI scaling factor, and presentation status:
```text
800 600 0 0 24 100 presented
```

#### `<id>/data`
Accepts graphical updates depending on the negotiated mode:
- **Raw Mode (`mode raw`):** Raw byte stream of packed 32-bit RGBA/ARGB pixels.
  When the full frame byte count (`width * height * 4`) is satisfied, the
  authority marks the full buffer dirty and submits a `SurfaceTransaction` to
  the engine.
- **Plan 9 Mode (`mode libdraw`):** Byte-stream of binary `libdraw` operators.
  The frontend rasterizes vector lines, fills, glyphs, and Porter-Duff alpha
  compositing into an internal CPU buffer, calculating minimal damage bounding
  rectangles and committing them to the engine.

#### `<id>/mouse`
Delivers structured pointer events to the client on `read()`. Standard Plan 9
event format:
```text
'm' <x:i32> <y:i32> <buttons:u32> <msec:u64>\n
```
- Coordinates are surface-local, starting from `(0, 0)` at the top-left corner.
- Buttons are represented as a bitmask (Bit 0: Left, Bit 1: Middle, Bit 2: Right,
  Bit 3: Scroll Up, Bit 4: Scroll Down).
- Reads block until new motion or button events are routed to this surface by
  the engine.

#### `<id>/kbd`
Delivers UTF-8 formatted keystroke events. Each character or key event is
streamed as it arrives from the engine's target-resolved input dispatcher.

---

## 5. Security and Confinement via Mount Namespaces

The synthetic filesystem model provides a natural, airtight sandbox boundary
when paired with Linux kernel mount namespaces (e.g., via Bubblewrap):

```text
 ┌────────────────────────────────────────────────────────┐
 │ BUBBLEWRAP CONTAINER (Confined Namespace)              │
 │                                                        │
 │   • Read-only root filesystem                          │
 │   • No network access                                  │
 │   • Mount: bind /run/.../draw/42 -> /dev/draw          │
 │                                                        │
 │   ┌───────────────────────────────────────────────┐    │
 │   │ Untrusted App (Acme / Python Script)          │    │
 │   │ Sees only: /dev/draw/{ctl, data, mouse, kbd}  │    │
 │   └───────────────────────────────────────────────┘    │
 └──────────────────────────┬─────────────────────────────┘
                            │
                            ▼ Synthetic 9P Mount
 ┌────────────────────────────────────────────────────────┐
 │ HOST SOPHIA RUNTIME & 9P FRONTEND                      │
 │  • Enforces NamespaceId boundary                       │
 │  • Client has zero vocabulary to enumerate other apps  │
 └────────────────────────────────────────────────────────┘
```

1. **Zero Ambient Discovery:** A sandboxed application given only its own
   `<id>` directory cannot see or access `/run/.../draw/new` or any sibling
   directories. It is physically impossible for the client to discover other
   running applications, take screenshots, or sniff global keystrokes.
2. **Namespace Keying:** The session supervisor issues an immutable
   `NamespaceContext` during admission. The 9P authority attaches this context
   to the allocated `SurfaceId`. Any cross-boundary interaction (e.g., clipboard
   exchange) must route through `sophia-portal`.
3. **Instant Teardown:** When the client process exits or closes its file handles,
   the 9P authority detects the disconnection, tears down the synthetic node,
   and submits a surface removal transaction to `sophia-engine`.

---

## 6. Target Workflows and Use Cases

### A. The Pure Plan 9 Ecosystem (`plan9port`)
Programs like **Acme**, **Sam**, **Page**, and **Mothra** link against `libdraw`.
Instead of requiring `plan9port`'s X11 or Wayland translation daemons (`devdraw`),
the programs talk directly to the synthetic filesystem served by
`sophia-9p-authority`. They run natively without emulation layers.

### B. Shell Scripting and Zero-Dependency Tooling
Opening a GUI window from a shell script in modern Linux currently requires
spawning heavy toolkits (Zenity, Yad, GTK dialogs). Under 9P, a complete
interactive window requires only core shell tools:

```bash
#!/usr/bin/env bash
# Minimal interactive canvas in pure Bash
WINDOW_DIR="$XDG_RUNTIME_DIR/sophia/draw/$(cat $XDG_RUNTIME_DIR/sophia/draw/new)"
cd "$WINDOW_DIR" || exit 1

echo "size 320 240" > ctl
echo "title System Monitor" > ctl

# Paint red background
python3 -c "import sys; sys.stdout.buffer.write(b'\xFF\x00\x00\xFF' * (320 * 240))" > data

# Read mouse input
while read -r tag x y btn ts; do
    if [ "$btn" -ne 0 ]; then
        echo "Clicked at ($x, $y) with button $btn"
    fi
done < mouse
```

### C. Containerized TUI / GUI Dashboards
Minimalist appliances, embedded tools, and containerized utilities can export
real-time status graphs, vector shapes, or interactive buttons without shipping
Mesa, OpenGL, Wayland libraries, or X11 dependencies.

---

## 7. Engine Integration Contract

The contract between `sophia-9p-authority` and `sophia-engine` mirrors that of
`sophia-x-authority`:

| Phase | Flow | Data Transferred |
| :--- | :--- | :--- |
| **Allocation** | Frontend → Engine | `SurfaceTransaction::CreateSurface` with unique `SurfaceId`. |
| **Frame Commit** | Frontend → Engine | CPU buffer memory handle, damage bounding box (`DamageRect`), and transaction epoch. |
| **Layout** | Engine → WM Policy | Engine passes opaque `SurfaceId` to `sophia_wm_v1`. The WM tiles or floats the window without knowing it was created over 9P. |
| **Input Delivery** | Engine → Frontend | `RoutedInputRequest` delivers target-resolved pointer and key events. Frontend serializes them to `<id>/mouse` and `<id>/kbd`. |
| **Teardown** | Frontend → Engine | `SurfaceTransaction::DestroySurface` cleans up visual scene nodes atomically. |

---

## 8. Implementation Roadmap

1. **Milestone 1: In-Memory 9P Protocol Server:** Implement a lightweight, pure-Rust
   9P2000.L server in `crates/sophia-9p-authority`, exposing the synthetic
   directory tree over a local Unix domain socket.
2. **Milestone 2: Raw Mode & Engine Ingress:** Support `mode raw`, writing pixel
   buffers directly to anonymous CPU shared memory and submitting
   `SurfaceTransaction` batches to `sophia-engine`.
3. **Milestone 3: Input Serialization:** Hook into `RoutedInputRequest` to stream
   mouse motion, clicks, and keystrokes through the `mouse` and `kbd` files.
4. **Milestone 4: Plan 9 `libdraw` Decoding:** Add a software rasterizer for Plan
   9 vector drawing primitives, enabling unpatched `plan9port` binaries to run
   directly against the authority.
5. **Milestone 5: Bubblewrap Sandbox Templates:** Provide standard profiles for
   launching sandboxed 9P applications with isolated per-window mount trees.

# Protocol Frontend Candidates and Display Models

**Role:** informative architecture rationale and future candidate evaluation.

**Status:** proposal and comparative research; non-normative.

**2026-09-25 direction update:** the
[9P public-interface direction](sophia-9p-control-bus.md) is accepted, including
a 9P application frontend alongside X authority. Its API and implementation
remain open. Other candidates below remain comparative proposals, not additional
admitted work. Existing native IPC remains the current contract during migration.

## 1. Context and Motivation

Traditional display server architectures are defined by tight coupling between
their core rendering loop and their client wire protocols:

- **Monolithic X11 (Xorg):** A single, un-sandboxed address space where protocol
  interpretation, driver management, window hierarchies, and input dispatch are
  entangled. Global resource namespaces permit arbitrary pixel and keystroke
  snooping. Asynchronous window configuration causes tearing and transient
  mismatches between geometry and buffer contents.
- **Monolithic Wayland:** While solving X11's security model by isolating client
  buffers, Wayland forces hardware driver modesetting (DRM/KMS), input handling
  (`libinput`), scene-graph composition, window layout policy, and desktop
  chrome into a single executable loop. If a window manager crashes, the entire
  session and all running applications die. Furthermore, pushing core desktop
  primitives into fragmented extension families has led to design-by-committee
  stalemates across competing desktop implementations.

Sophia resolves this tension by decomposing desktop authority:

```text
                  [ Sandboxed Client Applications ]
                                  │
                                  ▼ (client wire protocol)
                  ┌───────────────────────────────┐
                  │   PROTOCOL AUTHORITY LAYER    │ ◄── [ Pluggable Frontends ]
                  └───────────────┬───────────────┘
                                  │
                                  ▼ (anonymous visual transactions)
                  ┌───────────────────────────────┐
                  │         SOPHIA ENGINE         │ ◄── [ Visual Kernel ]
                  └───────┬───────────────┬───────┘
                          │               │
  [ sophia_wm_v1 ]        ▼               ▼       [ sophia_shell_v1 ]
  opaque spatial nodes ┌──────┐       ┌───────┐   edge reservations
                       │  WM  │       │ SHELL │   and UI descriptors
                       └──────┘       └───────┘
```

`sophia-engine` operates as a **protocol-neutral visual kernel**. It consumes
only abstract, anonymous visual transactions: buffers (DMA-BUF / CPU memory),
damage regions, layout epochs, and presentation outcomes. It is completely blind
to client-facing protocols, window hierarchies, and application metadata.

The **Protocol Authority Layer** is responsible for terminating client-facing
protocols, virtualizing client resources, managing protocol-specific state
machines, and mapping them into anonymous engine transactions.

Currently, **`sophia-x-authority`** serves as the primary production frontend,
providing immediate, secure compatibility for existing Linux applications by
isolating clients into namespaces and mediating data handoffs through portals.

This document evaluates next-generation display paradigms and outlines future
protocol frontend candidates to sit alongside `sophia-x-authority`.

---

## 2. Evaluation of External Paradigms: Arcan SHMIF

The [Arcan](https://arcan-fe.com/) multimedia and display engine features a
capability-driven IPC protocol called **SHMIF (Shared Memory Interface)**.
Unlike socket-serialized protocols, SHMIF relies on memory-mapped buffers
(`memfd`) containing video frames, interleaved audio samples, and bidirectional
lock-free event rings.

### What Fits

1. **Integrated Audio/Video Synchronization:** SHMIF passes audio samples
   alongside video buffers, enabling synchronization against display v-blank
   without relying on out-of-band audio daemons.
2. **Crash Resilience and Hot Reattachment:** Because client state resides in
   independent memory segments, clients can survive a display server restart or
   migrate across endpoints.
3. **Structured Terminal Subprotocol (`shmif-tui`):** Replaces legacy VT100
   escape sequences with packed cell grids (glyphs, attributes, truecolor),
   bypassing the PTY subsystem and terminal emulator parser vulnerabilities.

### Why It Failed to Gain Mainstream Traction

1. **Scope Explosion:** Rather than focusing on being a clean display protocol,
   Arcan attempted to replace the entire userland: display server, terminal
   emulator, audio daemon, window manager (via embedded Lua), remote desktop
   (A12), and 3D game engine.
2. **Toolkit Isolation:** Mainstream toolkits (GTK, Qt, Chromium, Flutter) never
   built or upstreamed native SHMIF backends, relegating real-world applications
   to translation bridges (`waybridge`, `xarcan`).

### Relationship to Sophia's Internal Protocols

- **Bubblewrap vs. SHMIF:** SHMIF is an IPC transport; it does not replace
  OS-level sandboxing. An unconfined SHMIF client can still inspect `/proc`,
  read `~/.ssh`, or access the network. In Sophia, Bubblewrap (or equivalent OS
  namespaces) isolates the process, while the protocol boundary provides the
  confinement channel.
- **Sophia WM (`sophia_wm_v1`):** A fundamental mismatch. The WM computes
  pure spatial algebra over opaque node bounds. SHMIF's video/audio buffers and
  event rings are irrelevant to a metadata-blind window manager.
- **Sophia Shell (`sophia_shell_v1`):** `sophia_shell_v1` carries first-class
  desktop concepts: screen-edge reservations, struts, descriptor switchers, and
  cryptographic action capabilities (`ToplevelActionCapability`). SHMIF lacks
  dock and strut semantics.

Therefore, SHMIF is not an internal Sophia protocol. If implemented, its role
would be strictly as an external client frontend (`sophia-shmif-authority`).
However, given the absence of third-party SHMIF application development,
alternative frontends offer substantially higher leverage.

---

## 3. Protocol Frontend Candidates

Because `sophia-engine` is completely decoupled from application wire protocols,
new frontends can be added without altering engine composition or modesetting.
Three viable candidate frontends are identified:

```text
 ┌────────────────────────────────────────────────────────────────────────────┐
 │                         PROTOCOL FRONTEND CANDIDATES                       │
 ├────────────────────────┬─────────────────────────┬─────────────────────────┤
 │   sophia-surface-v1    │   sophia-9p-authority   │ sophia-remote-authority │
 │   (Native GPU UI)      │  (Synthetic 9P Fs)      │   (Headless Streaming)  │
 └───────────┬────────────┴────────────┬────────────┴────────────┬────────────┘
             │                         │                         │
             ▼                         ▼                         ▼
 ┌────────────────────────────────────────────────────────────────────────────┐
 │                 SOPHIA ENGINE: ANONYMOUS TRANSACTION CORE                  │
 └────────────────────────────────────────────────────────────────────────────┘
```

### Candidate 1: `sophia-surface-v1` (Native GPU / Modern UI Frontend)

**Objective:** A lean, zero-overhead native graphics protocol for modern
applications built on Rust, C, or Go (e.g., SDL3, Bevy, winit, egui, Slint, MPV).

Modern GPU-rendered applications do not require legacy windowing constructs
(atoms, server-side font rendering, complex X11 extensions) nor do they benefit
from the sprawling extension surface of Wayland. They require only:
1. Negotiating an anonymous surface token.
2. Passing a DMA-BUF or Vulkan swapchain image handle with a damage bounding box.
3. Receiving target-resolved pointer, keyboard, and touch events.

**Wire Characteristics:**
- Binary framing over local Unix stream sockets (following Sophia's KDL wire
  specifications).
- Explicit layout epochs: client commits are transactional and synchronized
  with engine frame scheduling.
- Direct DMA-BUF import with Linux sync fences (`sophia-drm-out-fence`).
- Extremely compact client implementation: a complete `winit` or `SDL3` backend
  can be implemented in under 500 lines of code.

### Accepted Direction: `sophia-9p-authority` (Application Frontend Target)

**Objective:** expose application content and routed input through a documented
synthetic filesystem served over 9P2000.L, alongside continued X11 support.
The [frontend design](sophia-9p-authority.md) owns this target. The checked-in
crate is a scaffold; it does not establish an integrated or conforming frontend.

The broader direction also targets 9P public WM, shell and administrative APIs.
Those services retain their existing semantic owners and separate admissions;
they are not new responsibilities of the application authority. Shared protocol
machinery does not combine application, spatial-policy and shell authority.

Mounted file operations and generic direct clients could make independent
applications easier to build. Applications could also export their own services
to explicitly granted consumers. Both uses require versioned file semantics,
bounded events, transaction outcomes, revocation and resource-retirement rules.
Mount namespaces supplement server authorization rather than replace it.

The content format, input encoding, classic Plan 9 compatibility, remote access
and any specialized buffer transport remain separate design questions. Neither
unmodified plan9port compatibility nor low-overhead graphics follows from the
wire dialect alone. Migration requires independent WM, content-shell and
application clients plus equivalent behavior, recovery and measured performance.

### Candidate 3: `sophia-remote-authority` (Headless Streaming Frontend)

**Objective:** Direct headless virtual desktop hosting and remote development.

Rather than running an external VNC/RDP daemon that captures and scrapes an X11
root window:
- Directly consumes engine surface damage streams.
- Encodes damage regions using hardware video encoders (VA-API / NVENC for
  H.264, HEVC, or AV1).
- Streams frames over RDP (via FreeRDP integration) or WebRTC (browser-accessible
  canvas).
- Delivers remote keyboard/mouse inputs into the engine's input dispatch.

---

## 4. Architectural Analysis: Why a "Terminal Authority" is an Anti-Pattern

It is tempting to look at the traditional Linux terminal pipeline and view it
as excessive indirection:

```text
Shell / TUI  ──►  PTY  ──►  Terminal Emulator  ──►  X11/GLX  ──►  Frontend  ──►  Engine
```

One might hypothesize a `sophia-pty-authority` that eliminates the standalone
terminal emulator by parsing PTY byte streams, rasterizing glyphs, and feeding
damage directly into `sophia-engine`.

However, upon rigorous architectural inspection, this concept is an
**anti-pattern** that violates the core separation of concerns:

1. **The Fallacy of "Collapsing Indirection":** Calling the terminal stack
   "absurd indirection" is the exact same rationale that created the Wayland
   monolith. Wayland viewed the separation between compositor, window manager,
   and display server as wasteful, collapsing them into a fragile single-process
   loop. In reality, each layer of the terminal stack has a singular, decoupled
   responsibility:
   - **Kernel PTY:** Manages OS process groups, signals (`SIGINT`, `SIGTSTP`),
     and line discipline (`termios`).
   - **Terminal Emulator (Kitty / Alacritty):** An ordinary user-space client
     that parses escape codes, rasterizes fonts, and formats cells.
   - **Protocol Authority (`sophia-x-authority`):** Virtualizes client
     resources and isolates namespaces.
   - **Visual Kernel (`sophia-engine`):** Schedules atomic DRM/KMS scanout.
2. **Attack Surface and Vulnerability Bloat:** VT100 / ANSI escape-sequence
   parsers have a notorious history of security vulnerabilities and arbitrary
   code execution flaws. Pulling an escape parser into a privileged display
   authority needlessly exposes the display infrastructure to hostile output
   streams.
3. **Scope Creep (The Arcan Mistake):** Moving terminal emulation into the
   display stack forces the authority to become a font layout engine (FreeType,
   HarfBuzz shaping, fallback chains, glyph caching, DPI scaling). This is the
   exact trap that bloated Arcan's `shmif-tui`.
4. **Preserving Client Autonomy:** In Sophia's architecture, a terminal
   emulator is simply an ordinary, sandboxed client application presenting GPU
   buffers. Kitty, for example, renders directly to a DMA-BUF and presents it to
   Sophia—achieving direct zero-copy hardware scanout without polluting the
   display architecture with terminal emulation logic.

---

## 5. Architectural Invariants for New Frontends

Any new protocol authority implemented for Sophia must adhere to the following
system invariants:

1. **Protocol Neutrality:** `sophia-engine` must never import, link, or parse
   protocol structures from any client wire format. It accepts only
   `SurfaceContentStream` transactions.
2. **Namespace Confinement:** Every client admitted through any frontend must be
   assigned an immutable `NamespaceContext` by the session supervisor.
   Cross-namespace sharing must fail closed and require explicit mediation
   via `sophia-portal`.
3. **Metadata Blindness:** Window managers, through current `sophia_wm_v1` or
   target 9P role interfaces, must never receive
   application titles, classes, PIDs, or protocol-specific atoms from any
   frontend. Layout operates strictly on opaque `SurfaceId` spatial nodes.
4. **Failure Isolation:** A crash or stall in any protocol frontend must not
   compromise engine scanout or terminate unrelated frontends.

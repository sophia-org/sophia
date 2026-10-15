# Sophia

Sophia is a secure Linux desktop inspired by Plan 9, with atomic display transactions, a namespace for every client, and a native X11 server written in Rust. Engine owns the input devices and the display, and it commits each window's geometry and pixels together. The X server, the window manager, the shell and the portals that move data between applications all run as separate processes.

Window managers and shells talk to Sophia through small file trees served by one shared 9P2000.L core. Each role keeps its own endpoint and its own permissions. You can write one in whatever language you like without touching the compositor.

X11 is a full frontend, not a compatibility layer. Sophia's X server is new code, written from scratch in Rust. It owns X11 resources and delivery rules, while Engine only ever sees surfaces, buffers and layout transactions.

The project site is [sophia.gg](https://sophia.gg). If you want to write a window manager, a shell or a whole desktop, start with [Building on Sophia](docs/building-on-sophia.md).

## Architecture

Sophia has five main parts:

- **Engine** owns the input devices, the scene graph, frame timing, visual commits, rendering and the display.
- **The X server** (`sophia-x-authority`) accepts X11 connections, manages resources, enforces protocol rules, and turns client requests into Engine transactions.
- **The window manager** proposes layout and focus. It handles workspaces and key bindings using anonymous layout nodes and surface handles, and never sees titles, process IDs or pixels. [Hagia](https://github.com/sophia-org/hagia) is the reference window manager.
- **Shells** supply panels, launchers and switchers, and can reserve space along a screen edge. They receive window metadata only after the broker has filtered it. A shell can describe UI elements for Engine to draw or, with permission, submit its own pixels; either way Engine decides placement, presentation and input. [Narthex](https://github.com/sophia-org/narthex) is the reference shell for the first model.
- **Portals** move data between namespaces: clipboard, drag-and-drop and screen capture. Each transfer needs a grant for one specific recipient.

All public roles share one 9P implementation but keep separate exports and permissions; the [one-core decision](docs/notes/decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md) explains why. The session builds each client's socket directory and file trees from the identity it proved and the grants it holds, and the server keeps checking those grants on files the client already has open. The window manager, lock, output and shell now authenticate when they connect; the [factotum guide](docs/factotum-launch-authentication.md) spells out what that does and doesn't prove. Per-client namespaces and an authorized developer launcher are described in the [namespace client guide](docs/namespace-client-guide.md). The [delivery plan](docs/notes/plans/jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order) gives the order of the remaining work; the native application frontend and input driving come later, as separate pieces.

The drawing below shows the target, including the planned 9P application frontend. The session admits and routes every public role operation before it reaches Engine.

```text
===============================================================================================
                                      HARDWARE AND KERNEL
===============================================================================================
[ Physical input devices ]                                                  [ Display output ]
    |                                                                                     ^
    | libinput                                                                    DRM/KMS |
    |                                                                                     |
    |   +----------------------+   +----------------------+   +----------------------+    |
    |   | EXTERNAL WM          |   | EXTERNAL SHELL       |   | PORTALS              |    |
    |   | Opaque snapshots     |   | Sanitized metadata   |   | Authorized transfers |    |
    |   | Layout / focus       |   | Descriptors, content |   | Between namespaces   |    |
    |   +----------------------+   +----------------------+   +----------------------+    |
    |               ^                          ^                          ^               |
    |               |                          |                          |               |
    |               |   Shared 9P core, separate role exports             |               |
    |               |                          |                          |               |
    |               v                          v                          v               |
    |   +----------------------------------------------------------------------------+    |
    |   | SOPHIA ENGINE: COMPOSITOR AUTHORITY                                        |    |
    +-->| Scene graph | hit-testing | damage tracking | frame scheduling             |    |
        | Atomic geometry + pixel commits | rendering | scanout                      |----+
        +----------------------------------------------------------------------------+
                          ^                                        ^
                          | Admitted surface / buffer transactions |
                          | Routed input / configure / lifecycle   |
                          v                                        v
        +-----------------------------------+    +-----------------------------------+
        | X11 APPLICATION AUTHORITY         |    | 9P APP AUTHORITY (planned)        |
        | X resources / protocol state      |    | Fids / app files / buffer intake  |
        | Native Rust, built from scratch   |    | Translate client state            |
        +-----------------------------------+    +-----------------------------------+
                          ^                                        ^
                          | X11                          9P2000.L  |
                          v                                        v
               Namespace admission routes each app to its protocol authority.

        +-----------------------------------+    +-----------------------------------+
        | PRIVATE NAMESPACE A               |    | PRIVATE NAMESPACE B               |
        | X11 and/or 9P applications        |    | X11 and/or 9P applications        |
        | Only admitted endpoints mounted   |    | Only admitted endpoints mounted   |
        +-----------------------------------+    +-----------------------------------+

        Namespaces separate trust domains; either may contain both client types.
        Portals authorize specific transfers without joining the namespaces.
```

Administration now uses the shared core by default, with the old transport retained for explicit rollback. The metadata broker still uses the old socket protocol. The 9P application frontend and the shared contract between frontends and Engine are planned; the drawing doesn't show them as finished.

## Design

### Files and namespaces

The session builds each identity's socket directory and discovery file from trusted policy, and each endpoint serves its own role tree. When a launched client attaches, it proves its identity, and that identity is fixed for the connection. The server checks permissions on every new operation and on files that are already open. X11 keeps its own admission checks. Mounting a path doesn't grant permission to use it.

The roles share file operations, not a record format. Small commands and discovery use lines of text; window-manager and shell traffic keeps compact binary records. Sophia borrows Plan 9's namespaces and file interface but not its draw protocol, because Engine already does the drawing.

The portal service decides whether a transfer may happen, from a few fixed facts, and the receiving frontend carries it out. A 9P client gets one object bound into its tree, and if the grant is revoked, its open files go stale. An X client gets ordinary X semantics. Screen capture is the first portal: a frame record and the screenshot bytes for one authorized output. File descriptors travel on a separate channel, never in the 9P stream, and a capture grant never lets anyone inject input.

### Geometry and pixels together

A resize is a transaction: the new size and the matching pixels commit together. If a client stalls, Engine keeps showing the last complete frame until the new one is ready.

### A window manager without client data

The window manager gets anonymous layout nodes and returns proposals. It can't see XIDs, window titles, process IDs, namespace identities or clipboard contents. It runs outside the rendering loop, so you can change layout policy without touching presentation or input.

### Namespace isolation

Trusted applications can share a namespace and use the familiar X11 object model. Applications that need isolation run in separate namespaces, where lookups across the boundary fail. A portal grants one transfer without opening either namespace to the other.

## Documentation

[Building on Sophia](docs/building-on-sophia.md) describes the component interfaces and how to use them. The [strategic roadmap](docs/strategic-roadmap.md) describes how parts of this repository will move into their own repositories under `sophia-org`. The [documentation index](docs/README.md) links to the architecture, protocol, configuration and security specifications.

Investigations and design decisions live in the [development notebook](docs/notes/README.md). Open work is tracked in [todo.md](todo.md); contributors should follow the [work-tracking guide](docs/work-tracking.md).

Agents can load the context for a task with `zk context task t319` (or `cargo-slot sophia cargo xtask context task t319`). The [context guide](docs/agent-context.md) covers section reads and handoffs.

## Status

Sophia is a research prototype, though it runs my desktop every day. X11 is the only application protocol so far, and the Rust X server implements part of it. Application compatibility and end-to-end testing are still in progress.

The window-manager, shell, output, lock, factotum, inspection and portal roles all run on the shared core. The portal decision files drive the session's X clipboard, though they still need testing on real hardware. Authenticated connections and per-client namespaces have landed. Screen capture works on both physical displays, and locking the screen or changing a display revokes old grants on time; capture of mirrored displays is still being tested under [t319](docs/notes/investigations/hok3zvgt-deliver-the-admitted-observer-output-status-and-capture-path.md#current-resumption).

The output role goes only to a protected output process named in the profile; without one, the session applies the profile itself and nobody else gets the role. [Read-only window-manager inspection](docs/sophia-wm-inspection.md) has its own permission and endpoint. Administration, the metadata broker, the shared contract between frontends and Engine, and the 9P application frontend are unfinished. The [delivery plan](docs/notes/plans/jsschoen-converge-public-roles-on-one-9p-core.md) keeps their implementation and acceptance separate.

Wayland is possible but off the roadmap. Someone could write a compatibility layer as a client of the planned 9P application frontend, carrying its clients as separate identities, once that frontend exists. Agents use the same interfaces as every other client; observer, operator and driver are independent sets of grants, not special agent privileges.

## License

Sophia uses the [BSD 3-Clause License](LICENSE).

# Sophia

Sophia is an experimental display server and compositor with a native X11 frontend. Its Engine controls physical input and display output, committing window geometry and pixels together. Separate components handle application protocols, window layout, shell UI, and transfers between isolated applications.

The design follows the Unix tradition of programs with specific jobs. Window managers and shells run as separate processes and communicate over versioned protocols. You can write either in the language of your choice without rebuilding the compositor.

The Engine's transaction interface is protocol-neutral. The X11 frontend owns X11 resources and delivery rules; the Engine works with surfaces, buffers, and layout transactions. This keeps application protocol details out of the rendering core.

Visit [sophia.gg](https://sophia.gg) for the project overview and development posts. To build a window manager, shell, or desktop environment, start with [Building on Sophia](docs/building-on-sophia.md).

## Architecture

Each component has a defined authority:

- **Sophia Engine** owns physical input, the scene graph, frame scheduling, visual commits, rendering, and display output.
- **Sophia X Server Frontend** accepts X11 connections, manages resources, enforces protocol rules, and translates client state into Engine transactions. It is written in Rust in `sophia-x-authority`.
- **Sophia WM** proposes layout and focus through `sophia_wm_v1`. It handles workspaces and keybindings using opaque layout nodes and surface handles. [Hagia](https://github.com/sophia-org/hagia) is the reference window manager.
- **Sophia Shell** supplies panels, launchers, and switchers through `sophia_shell_v1` and requests space along screen edges. Shells receive sanitized metadata through the broker. They can submit UI descriptors or, with permission, their own rendered content; the Engine controls placement, presentation, and input. [Narthex](https://github.com/sophia-org/narthex) is the descriptor shell reference.
- **Sophia Portals** broker transfers between namespaces, such as clipboard sharing, drag-and-drop, and screen capture. Each transfer requires authorization for a specific recipient.

```text
================================================================================
                         HARDWARE AND KERNEL
================================================================================
 [ physical input devices ]                                  [ display output ]
            │                                                        ▲
            │ raw input via libinput                                 │ DRM/KMS
            ▼                                                        │

================================================================================
                    SOPHIA ENGINE: COMPOSITOR AUTHORITY
================================================================================
 ┌────────────────────────────────────────────────────────────────────────────┐
 │ Scene graph | spatial hit-testing | damage tracking | frame scheduling     │
 │ Atomic visual commits | rendering | scanout                                │
 └───────────────┬───────────────────┬────────────────────┬───────────────────┘
          ▲      │                   │                    │      ▲
          │      │ opaque snapshots  │ portal events      │      │ descriptors & chrome
          │      ▼                   ▼                    ▼      │
 ┌───────────────┐        ┌────────────────┐       ┌─────────────────────────┐
 │  SOPHIA WM    │        │ SOPHIA PORTALS │       │      SOPHIA SHELL       │
 │ blind policy  │        │ allow/deny     │       │ panels & switchers      │
 │ layout/focus  │        │ handoff/revoke │       │ work area reservations  │
 └───────┬───────┘        └────────┬───────┘       └────────────┬────────────┘
         │                         │                            ▲
         │ layout proposals        │ portal commands            │ sanitized metadata
         │ [sophia_wm_v1]          │ [sophia_portal_v1]         │ & UI descriptors
         ▼                         ▼                            │ [sophia_shell_v1]

================================================================================
                         PROTOCOL AUTHORITY LAYER
================================================================================
 ┌────────────────────────────────────────────────────────────────────────────┐
 │ Sophia X Server Frontend: X11 resources, selections, grabs, protocol checks │
 └────────────────────────────────┬───────────────────────────────────────────┘
                                  │
                                  │ namespace-checked surface transactions
                                  │ routed input / configure / lifecycle
                                  ▲

================================================================================
                         SANDBOXED CLIENT NAMESPACES
================================================================================
 ┌────────────────────────────────────┐     ┌─────────────────────────────────┐
 │ Namespace A: trusted               │     │ Namespace B: untrusted          │
 │ X terminal | trusted local tools   │  X  │ X browser | untrusted X app     │
 └────────────────────────────────────┘     └─────────────────────────────────┘
```

## Design

### Geometry and pixels together

A resize is a visual transaction: new geometry and matching pixels commit together. If a client stalls, the Engine keeps the last committed visual state until a replacement is ready.

### Window management without client metadata

The window manager receives opaque layout nodes and returns policy proposals. It has no access to XIDs, window titles, process IDs, namespace identities, or clipboard contents. It runs outside the rendering loop, so layout policy can change without taking over presentation or physical input.

### Namespace isolation

Trusted applications can share a namespace and use the traditional X11 object model. Applications that need isolation run in separate namespaces, where cross-namespace resource lookups fail closed. A portal authorizes a specific data transfer without opening either namespace to general access.

## Documentation

[Building on Sophia](docs/building-on-sophia.md) describes the component interfaces and how to use them. The [Strategic Roadmap](docs/strategic-roadmap.md) outlines the migration from monorepo incubation to autonomous satellite repositories under `sophia-org`. The [documentation index](docs/README.md) links to architecture, protocol, configuration, and security specifications.

Investigations and design decisions live in the [development notebook](docs/notes/README.md). Current work is tracked in [todo.md](todo.md); contributors should follow the [work-tracking guide](docs/work-tracking.md).

## Status

Sophia is a research prototype. X11 is the current application protocol, and the Rust frontend implements a subset of it. Application compatibility and end-to-end validation remain in progress.

The accepted [9P2000.L direction](docs/sophia-9p-control-bus.md) targets common public interfaces for WMs, shells and administration, plus a 9P application frontend alongside X11. It preserves namespace isolation and separate authorities. The file API and migration remain design work; the existing native protocols remain supported. Other frontend candidates have not been promoted by this decision.

## License

Sophia uses the [BSD 3-Clause License](LICENSE).

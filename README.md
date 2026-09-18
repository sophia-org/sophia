# Sophia

Sophia is a modern, transaction-driven X11 display server and compositor. It keeps X11’s flexible application model and adds explicit authority boundaries and synchronized visual commits.

The design is a constitutional cathedral with bazaar edges. The core is a single, coherent system in which no component has unchecked power. The Engine controls pixels and physical input, the protocol frontend applies X11 rules, and the window manager sets layout policy. Well-defined interfaces limit each role.

You can replace or develop everything around this core—window managers, shells, and portal policies—independently. The core enforces safety and presentation; the edges decide how the desktop behaves. If you want to build one of these edges—a tiling window manager, a shell, or a full desktop environment—see [Building on Sophia](docs/building-on-sophia.md).

Sophia targets native X11. It preserves a classic shared-X profile for trusted applications and supports isolated namespaces for clients that should not share authority.

## Architecture

Sophia divides its systems based on what each part is permitted to control, rather than what is easiest to write.

- **Sophia Engine:** The absolute visual authority. It manages physical input, visual state, frame scheduling, transaction commits, rendering, and display output.
- **Sophia X Server Frontend:** A clean, modern X11 frontend. It presents the established X11 API, translates protocol state into Sophia surface transactions, and performs X11 delivery rules. It does not control layout or scanout.
- **Sophia WM (Window Manager):** A dedicated policy process handling layout, focus, keybindings, workspaces, and launch decisions. It operates entirely on opaque layout nodes and `SurfaceId` handles. [Hagia](https://github.com/sophia-org/hagia) is the reference implementation — an independent Nim client with no Sophia build dependency, and the repository to copy from if you're writing your own.
- **Sophia Portals:** Mechanisms for deliberate cross-namespace transfers, such as clipboard sharing, drag-and-drop, and screen capture.
- **Metadata Broker and Chrome:** Translates protocol metadata into redacted compositor UI without exposing namespaces to the window manager.
- **Sophia Shell:** A separately confined client that specifies shell UI, such as panels and switchers, and requests work-area reservations. The Engine renders and presents that UI. [Narthex](https://github.com/sophia-org/narthex) is the reference implementation, deliberately small.

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
          │      │ opaque snapshots  │ portal events      │      │ chrome data
          │      ▼                   ▼                    ▼      │
 ┌───────────────┐        ┌────────────────┐       ┌─────────────────────────┐
 │  SOPHIA WM    │        │ SOPHIA PORTALS │       │ METADATA BROKER/CHROME  │
 │ blind policy  │        │ allow/deny     │       │ redacted UI only        │
 │ layout/focus  │        │ handoff/revoke │       │ labels/icons/badges     │
 └───────┬───────┘        └────────┬───────┘       └────────────┬────────────┘
         │                         │                            ▲
         │ layout proposals        │ portal commands            │ sanitized
         ▼                         ▼                            │ metadata

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

## Core Principles

### Visual Authority

The Sophia Engine holds absolute visual authority. It commits window geometry and matching pixels together. If a client hangs during a resize, Sophia retains the last committed visual state until a replacement is ready.

### Opaque Window Management

Layout policy lives in an external process. Because the window manager operates outside the rendering hot path, it can crash, restart, or be rewritten without bringing down the session. To preserve security, the window manager remains blind to client identity: it receives opaque layout nodes and never sees an XID, a window title, a namespace, or a clipboard payload.

### Secure Namespaces

Sophia uses namespaces to separate applications that should not share authority. The classic shared-X profile runs trusted applications in a single namespace, preserving the traditional X11 object model. Cross-namespace lookups fail closed. Portals authorize specific, user-approved transfers, such as clipboard sharing, without granting general access to another namespace.

## Documentation

[Building on Sophia](docs/building-on-sophia.md) is the map for third-party developers: which component owns what, which protocol each piece speaks, and how a window manager, a shell, or a full desktop environment composes from them.

For design specifications, architectural guides, and security policies, see
[docs/README.md](docs/README.md). Maintain investigations, milestone history, and
architectural decisions through the [linked development notebook](docs/notes/README.md)
with `zk`; keep the active execution queue in [todo.md](todo.md), using the
[todo.txt tracking workflow](docs/work-tracking.md). `zk queue` shows the critical
path; `zk tasks` manages tasks and updates the notebook index.

## Status

Sophia is a research prototype. The native Sophia X Server Frontend is the designated product path, and no other application protocol is currently supported. However, the transaction boundary is protocol-neutral, allowing future translation layers or native interfaces—such as Wayland—to be added without importing another protocol's desktop architecture.

## License

Sophia is licensed under the BSD 3-Clause License. See `LICENSE`.

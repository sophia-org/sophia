# Sophia

Sophia is an experimental display server and compositor shaped by Plan 9: a shared file protocol, a namespace composed for each identity, and explicit grants between namespaces. Engine controls physical input and display output, committing window geometry and pixels together. Separate components handle application protocols, window layout, shell UI, and transfers between isolated applications.

Window managers and shells run as separate processes and use role files on a shared 9P2000.L core. Each export keeps its own endpoint and authority. You can write clients in the language of your choice without rebuilding the compositor.

X11 remains a first-class frontend, with a native authority written from scratch in Rust. It owns X11 resources and delivery rules; Engine works with surfaces, buffers, and layout transactions. Application protocol details stay outside the rendering core.

Visit [sophia.gg](https://sophia.gg) for the project overview and development posts. To build a window manager, shell, or desktop environment, start with [Building on Sophia](docs/building-on-sophia.md).

## The Shape

Each component has a defined authority:

- **Sophia Engine** owns physical input, the scene graph, frame scheduling, visual commits, rendering, and display output.
- **Sophia X Server Frontend** accepts X11 connections, manages resources, enforces protocol rules, and translates client state into Engine transactions. It is written in Rust in `sophia-x-authority`.
- **Sophia WM** proposes layout and focus through `sophia_wm_v1`. It handles workspaces and keybindings using opaque layout nodes and surface handles. [Hagia](https://github.com/sophia-org/hagia) is the reference window manager.
- **Sophia Shell** supplies panels, launchers, and switchers through `sophia_shell_v1` and requests space along screen edges. Shells receive sanitized metadata through the broker. They can submit UI descriptors or, with permission, their own rendered content; the Engine controls placement, presentation, and input. [Narthex](https://github.com/sophia-org/narthex) is the descriptor shell reference.
- **Sophia Portals** broker transfers between namespaces, such as clipboard sharing, drag-and-drop, and screen capture. Each transfer requires authorization for a specific recipient.

The [accepted one-core decision](docs/notes/decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
uses one shared 9P implementation with separately authorized role exports.
Per-identity endpoint directories and role trees are derived from admission;
grants remain checked on retained handles. t317 implements
authenticated attach for supervised WM, lock, output and shell roles;
[qualification and trust boundaries](docs/factotum-launch-authentication.md)
remain explicit. t318 implements admission-derived namespace recipes,
automatic application selection and an authorized developer launcher; see the
[namespace client guide](docs/namespace-client-guide.md). Renderer capture and
an admitted output-capture CLI are implemented; physical and performance
qualification remain open. The [delivery plan](docs/notes/plans/jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order)
starts with a bounded observer interface; the native application frontend
and input driving are separate deliverables.

```text
  X11 clients              9P clients: WM · shells · output
        │                  lock · factotum · inspection · portals
        ▼                              │
  ┌────────────────┐          ┌────────────────────┐
  │ X AUTHORITY    │          │ SHARED 9P CORE     │
  │ Native Rust    │          │ 9P2000.L           │
  │ Built from     │          │ Separate exports   │
  │ scratch        │          │ and endpoints      │
  └───────┬────────┘          └────────┬───────────┘
          │     admitted transactions, │
          │     proposals and commands │
          └──────────────┬─────────────┘
                         ▼
                  SESSION OWNERS
              admission · routing · policy
                         │
                         ▼
                   SOPHIA ENGINE
        input · scene · commits · rendering · scanout
                         │
                         ▼
                     DRM / KMS
```

Administration and the metadata broker still await migration from the legacy
socket envelope. The 9P application authority and the common protocol-authority
contract remain planned; the diagram does not show them as completed paths.

## Design

### File interfaces and composed namespaces

Session derives each identity's socket directory and discovery file from trusted
policy. Each endpoint serves its own role tree. Authenticated attach fixes
identity for the admitted launch paths, and the server checks authority on
retained handles as well as new operations. X11 keeps its own admission checks.
Kernel mounts do not confer authority.

The roles share file operations, not one record format. Small controls and
discovery can use text; WM and shell traffic keeps compact binary records.
Sophia borrows Plan 9's namespaces and file interfaces without adopting its
draw protocol. Engine continues to own composition.

Portal policy decides on bounded facts; the recipient's frontend executes the
transfer. A 9P recipient receives a grant-bound object in its tree, with retained
handles becoming stale on revocation. An X recipient uses X semantics. Capture
is the first output bind: a frame record and bounded screenshot bytes for an
authorized output. Descriptors belong on a separate channel, never in the 9P
stream. Capture grants imply no input-injection permission.

### Geometry and pixels together

A resize is a visual transaction: new geometry and matching pixels commit together. If a client stalls, the Engine keeps the last committed visual state until a replacement is ready.

### Window management without client metadata

The window manager receives opaque layout nodes and returns policy proposals. It has no access to XIDs, window titles, process IDs, namespace identities, or clipboard contents. It runs outside the rendering loop, so layout policy can change without taking over presentation or physical input.

### Namespace isolation

Trusted applications can share a namespace and use the traditional X11 object model. Applications that need isolation run in separate namespaces, where cross-namespace resource lookups fail closed. A portal authorizes a specific data transfer without opening either namespace to general access.

## Documentation

[Building on Sophia](docs/building-on-sophia.md) describes the component interfaces and how to use them. The [Strategic Roadmap](docs/strategic-roadmap.md) outlines the migration from monorepo incubation to autonomous satellite repositories under `sophia-org`. The [documentation index](docs/README.md) links to architecture, protocol, configuration, and security specifications.

Investigations and design decisions live in the [development notebook](docs/notes/README.md). Current work is tracked in [todo.md](todo.md); contributors should follow the [work-tracking guide](docs/work-tracking.md).

Agents can retrieve bounded task context with `zk context task t319` (or `cargo-slot sophia cargo xtask context task t319`).
The [context guide](docs/agent-context.md) covers section reads and session handoffs.

## Status

Sophia is a research prototype. X11 is the current application protocol, and the Rust frontend implements a subset of it. Application compatibility and end-to-end validation remain in progress.

WM, shell, output, lock, factotum, inspection and portal file exports use the
shared core. The portal decision files are wired into Session's X clipboard
coordinator; their physical acceptance remains separate from implementation.
Authenticated launch attachment and namespace composition have landed.
Capture and its CLI are implemented, but live capture currently times out;
[t319](docs/notes/investigations/hok3zvgt-deliver-the-admitted-observer-output-status-and-capture-path.md#current-resumption)
tracks the repair and remaining physical and performance qualification.

An explicit protected output process receives the output role; without one,
Session owns profile configuration without an output listener.
[Read-only WM inspection](docs/sophia-wm-inspection.md) has a separate host-domain
permission and endpoint. Administration, the metadata broker, the common
protocol-authority contract and the 9P application authority remain unfinished.
The [delivery plan](docs/notes/plans/jsschoen-converge-public-roles-on-one-9p-core.md)
separates their implementation and acceptance gates.

Wayland is possible but off the roadmap. A third party could build a compatibility
layer as a client of the planned 9P application authority, carrying its clients
as separate identities. That path depends on the application authority landing.
Agents use the same admitted interfaces as other clients; observer, operator
and driver name independent grant recipes, not special agent privileges.

## License

Sophia uses the [BSD 3-Clause License](LICENSE).

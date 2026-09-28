---
id: kcfh2hdg
date: 2026-09-28
kind: investigation
status: investigating
tags: [architecture, security]
---
# Adopting the Plan 9 namespace model in Sophia

## Question and scope

How should Sophia adopt the Plan 9 namespace model so each process or admitted
client group receives a composable view of files and services?

niltempus requested this investigation on September 28 while reviewing the
README's target architecture, then clarified that the subject is adopting the
namespace model itself. Merely using 9P for role messages or hiding sockets in
Linux mount namespaces does not answer that request.

The investigation should define which Plan 9 namespace semantics Sophia adopts,
how clients use them, and how they map onto a Linux implementation. It should
cover bind, mount, unmount, union ordering, inheritance, and private versus shared
namespace state. No particular implementation mechanism is selected yet.

## Questions to resolve

1. **Naming and composition.** Define a client's namespace root, service naming,
   bind and mount operations, replacement and union lookup rules, and where
   creation lands in a union. Decide which operations clients can perform
   themselves and which require Session admission.
2. **Identity and authority.** Map a filesystem namespace to Sophia's existing
   `NamespaceId`, immutable admission contexts and role grants. Identify when
   two clients share resources even if their path views differ. Path visibility,
   service identity and permission to perform an operation need separate rules.
3. **Process lifecycle.** Define inheritance at launch, private copies versus
   shared views, subsequent changes, service restart, revocation and cleanup.
   Specify what happens to existing fids, file descriptors and in-flight work
   when a name is rebound or a service is removed.
4. **Desktop composition.** Work through applications, WM, shells and
   administration seeing different service views. Allow both X11 and future 9P
   applications in a trust domain; protocol choice must not define trust.
   Keep the Engine's rendering and transaction interface protocol-neutral.
5. **Transfers.** Define how an authorized clipboard, drag-and-drop or capture
   transfer exposes a specific object or service without joining whole
   namespaces. Relate this to the broker/portal decision tracked by t273.
6. **Linux feasibility.** Compare kernel mounts, userspace filesystem mounts,
   and client-side namespace resolution for the chosen semantics. Re-check
   privilege requirements and compatibility with ordinary applications. Cover
   same-UID escape paths, inherited descriptors, runtime sockets and credentials;
   a naming view alone is not an isolation proof.
7. **Contributor interface.** Propose a small declarative namespace recipe and
   readable inspection interface. Keep named desktop policies in their own
   repositories; Sophia should provide generic composition and admission.

## Evaluated design: two-layer hybrid namespace architecture

To provide per-process namespace trees across Linux and BSDs without introducing
bloated userspace VFS layers or requiring unprivileged kernel mount permissions,
the evaluated model splits namespace resolution into two distinct layers:

```text
 ┌────────────────────────────────────────────────────────────────────────┐
 │                      DECLARATIVE NAMESPACE RECIPE                      │
 │    (KDL: mounts, service grants, union orderings, portal exports)      │
 └───────────────────────────────────┬────────────────────────────────────┘
                                     │
          ┌──────────────────────────┴──────────────────────────┐
          ▼                                                     ▼
 ┌─────────────────────────────────┐   ┌─────────────────────────────────┐
 │ LAYER 1: VIRTUAL 9P NAMESPACES  │   │ LAYER 2: PHYSICAL OS SANDBOXING │
 │ (Native 9P apps, WM, Shell)     │   │ (Legacy POSIX, X11 apps)        │
 │ • Pure userspace inside session │   │ • Pluggable sandbox drivers     │
 │ • Zero FUSE / kernel dependency │   │ • Socket-directories on tmpfs   │
 │ • Portable across Linux & BSDs  │   │ • bwrap / Jails / unveil        │
 └─────────────────────────────────┘   └─────────────────────────────────┘
```

### Layer 1: In-protocol virtual namespaces (native 9P and desktop roles)

For native 9P applications, window managers, shells, and administrative tools,
namespace composition is virtualized entirely inside `sophia-session`'s userspace
9P server core:

- **Virtual attach roots (`Tattach`):** Each connection's root is synthesized
  from its admission context. The WM receives only the `/wm` hierarchy; an
  admitted 9P application receives `/dev/{draw,events}` and authorized service
  stems under `/srv`.
- **Zero-overhead path routing:** Path traversal (`Twalk`) evaluates against
  an immutable array of granted string slices. There are no virtual inodes,
  dentries, or page caches; lookup overhead is single-digit microsecond slice
  matching.
- **Static union and bind rules:** Replaces runtime kernel `bind(2)` with
  declarative union tables. File lookups probe declared layers in order; creation
  lands in the designated writable tier.
- **Portability:** Because interaction occurs over standard `AF_UNIX` streams via
  9P messages, this layer is 100% portable across Linux, FreeBSD, OpenBSD, and
  NetBSD with zero root privileges and no kernel VFS interaction.

### Layer 2: Physical socket directories (POSIX and X11 containment)

Standard POSIX and X11 applications do not speak 9P and require physical filesystem
paths. Sophia extends the standardized socket-directory model (`ooy00zjd`):

- **Isolated runtime roots:** The supervisor provisions per-namespace directories
  on `tmpfs` under `$XDG_RUNTIME_DIR/sophia/<namespace-id>/` (e.g. holding `X0`
  and authorized portal FIFOs).
- **Pluggable host containment drivers:**
  - **Linux:** Invokes `bwrap` or direct `unshare(CLONE_NEWNS)` to bind-mount the
    allocated socket directory onto `/tmp/.X11-unix/`. The confined process is
    physically excluded from host and other-namespace sockets.
  - **FreeBSD:** Maps the socket directory into a lightweight unprivileged Jail
    or `nullfs` mount, paired with `cap_enter(2)` (Capsicum) capability mode.
  - **OpenBSD:** Invokes the client under `unveil(2)` restricted to the allocated
    socket directory, locked with `pledge(2)`.
  - **Unsandboxed fallback:** Direct environment pointer (`$NAMESPACE` or
    `$XDG_RUNTIME_DIR`), preventing accidental path collision.

### Anti-bloat guardrails

To prevent architectural bloat and preserve low-latency execution:

1. **No FUSE for core desktop roles:** Desktop roles must connect directly to
   9P endpoints over userspace sockets. FUSE adds four kernel context switches
   per operation and must not be used on the compositor control path.
2. **Static launch recipes over dynamic client mutation:** Namespaces are
   declaratively defined at launch in configuration (`desktop.kdl`) and sealed at
   attach. Clients cannot issue arbitrary dynamic `bind(2)` modifications to
   their running environment.
3. **Retained synchronous event loop:** Multiple namespace socket listeners
   are multiplexed within the existing non-blocking accept loop in `sophia-session`.
   No asynchronous runtime (Tokio) is introduced.
4. **Immediate handle revocation:** Because `sophia-9p` validates all operations
   at the export boundary, revoking a portal or service immediately returns
   `EBADF` or `ESTALE` on existing open fids without tearing down physical mounts.

## Evidence to collect

Read the original Plan 9 namespace documentation and cite the specific semantics
being adopted. Audit current Sophia namespace allocation, admission, service
exports and process launch code with file references. Distinguish current
behavior, earlier proposals and the new target model.

Use a bounded, device-free prototype to resolve uncertain Linux mechanisms if
needed. Exercise two private clients with overlapping names, an explicit shared
service, a rebound service, and a revoked transfer. Record expected visibility,
operation permission and retained-handle behavior separately. No live-session
changes are required for this investigation.

## Investigation exit

Produce a proposed ADR naming the adopted semantics, ownership and threat model;
a worked application namespace recipe; and a migration plan with concrete tests
for composition, inheritance, isolation and revocation. Identify implementation
gaps and any semantics that cannot be supported on the chosen Linux mechanism.
The investigation can finish with that reviewed design; implementation and
acceptance require their own tasks and evidence.

## Connections

- [Namespaces and portals](../../namespaces-and-portals.md) owns Sophia's
  existing resource isolation and admission contract. Reconcile it with the
  proposed naming model rather than treating identical terminology as identical
  semantics.
- [Earlier Plan 9 and mount investigation](5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
  records the role-transport discussion and a host-specific v9fs limitation.
  This investigation broadens the question to namespace composition.
- [Accepted 9P public-interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
  governs the transport direction; it does not by itself settle namespace APIs.
- [Socket-directory and frontend plan](../plans/ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md)
  supplies related isolation work and its review. A directory of sockets is one
  possible ingredient, not the full namespace model.
- [IPC removal inventory](1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
  tracks t273's broker/portal transport decision. Coordinate the designs without
  conflating that narrower migration with this investigation.

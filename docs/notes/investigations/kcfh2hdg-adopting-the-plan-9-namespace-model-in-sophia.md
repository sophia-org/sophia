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

That paragraph records the original September 28 question. The October 9
one-core ADR subsequently selected per-identity endpoint directories and
separate role trees; the October 10 draft below works within that decision.

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

This is the September 28 proposal, retained for provenance. Its single
forwarding-root, draw-file and unmeasured performance/portability claims are
not the accepted target. The later one-core ADR and the October 10 boundary
below replace those parts; no prototype established the quoted costs.

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

## Proposed recipe boundary (2026-10-10)

The accepted [one-core ADR](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
settles the architecture. This draft specifies t275's recipe and t318's test
boundary. It follows the [admission design](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md),
not the earlier pidfd-as-ancestry proposal. It is a design for review, with no
parser, authentication, endpoint exposure or capture implementation in this slice.

### Three independent values

| Value | Owner and rule |
| --- | --- |
| Visibility | A sealed recipe maps local names to role endpoint references. Names affect discovery and reach, not permission. |
| Service identity | Session's service instance and generation identify the actual export. Aliases of one export retain that identity; a restarted or rebound service has another generation. |
| Permission | Immutable admission plus current role/object grants. The export checks it on every operation, including retained and blocked fids. |

A recipe is compiled from supervisor-owned configuration against an admission
and a catalog of admissible services. It cannot mint a namespace or add a grant.
Its input includes a recipe revision/digest, namespace context, admitted service
references and independent operation grants. Its output is a bounded endpoint
table and discovery snapshot. It is not a new 9P server forwarding calls between
owners. Each connection still enters exactly one role export on the shared core.

### Five-operation language

Use a versioned, line-oriented file, `namespace-recipe 1`, followed by these
operations. This is Sophia's proposed subset, not a claim that Plan 9 has only
five operations: [namespace(6)](https://9p.io/magic/man2html/6/namespace) also
defines `import` and `clear`, and spells inclusion `.`. Sophia names it `include`.

| Operation | Proposed meaning |
| --- | --- |
| `mount SERVICE PATH` | Resolve a supervisor-approved service reference and add its role endpoint at PATH. No kernel mount, authentication bypass, network address or arbitrary host socket path. |
| `bind SOURCE PATH` | Alias the already-resolved endpoint or endpoint directory at SOURCE. Resolve the source once at compilation; do not follow later rebinding. Grants stay those of this admission. |
| `unmount PATH` | Remove this recipe's mapping at PATH. It is a construction operation, not proof that an already-open stream lost permission. |
| `cd PATH` | Change the recipe compiler's base for relative target names. It does not change the launched process's working directory. |
| `include FILE` | Expand another supervisor-owned, pinned recipe fragment at this point. Preserve order and detect cycles; it cannot read client-supplied paths. |

Spaces and tabs delimit tokens; blank lines and whole-line comments are ignored.
Version 1 has no shell expansion, environment substitution, quoting, executable
hooks or runtime client commands. Service references are typed catalog names,
not credentials. Paths normalize within the recipe root; NUL, escape above
root, invalid components and reserved discovery-name collisions refuse the
whole candidate. Include files resolve under the trusted configuration root,
with no symlink escape or mutable unpinned input admitted during compilation.

Directory `mount` and `bind` may carry `-b` or `-a` to prepend or append endpoint
entries, borrowing ordered composition from
[bind(1)](https://9p.io/magic/man2html/1/bind). This does not union the contents
of different role servers. Conflicting leaf names naming different services or
grant sets refuse; duplicate aliases of the same reference are deduplicated.
Without a flag, replacement must be explicit at the target. No `-c` creation
or `-C` data-cache flag is supported: role contracts own object creation and
freshness, and the current .L core does not implement generic create. Permission
failure never falls through to a lower union member.

Proposed parser limits, to pin in t318 tests before implementation: 64 KiB of
aggregate source, 1,024 expanded operations, include depth 8, 32 include files,
64 exposed role endpoints, path length 1,024 bytes and component length 255
bytes. Reject excess before publishing anything; these are chosen design
bounds, not measurements or current supported limits. A failed compile leaves
the previous admitted view intact. Compilation performs no GPU or input I/O.

### Worked observer and application views

The following is design notation, not accepted `desktop.kdl` syntax:

```text
namespace-recipe 1
cd /roles
mount inspection inspection
mount portal portal
bind inspection status
```

Here `inspection` and `status` name the same export, not two implementations.
The observer's separately approved grants allow bounded status/inspection and
requests for capture of named outputs. They do not allow portal approval,
administration, clipboard transfer or injection. A denied capture request
creates no object. An allowed request materializes its object inside the portal
owner's tree; it does not rewrite this base recipe. t257 owns the availability
record and t319 the capture frame grammar. The example does not assert these
files already exist.

The discovery file lists only this identity's endpoints, versions, opaque
service generations and availability, under bounded records. Exact pathname
and serialization are part of t318's versioned discovery contract. The output
availability slice distinguishes available, waiting and recovering; it cannot
report a remembered topology as currently presented. A client still authenticates
at the selected endpoint and reads that role's version/capability contract.

A confined X application can use a smaller recipe:

```text
namespace-recipe 1
cd /roles
mount x-group x11
mount portal portal
```

`x-group` denotes the group's admitted X listener, not a 9P export. The discovery
entry identifies its protocol. t142's containment driver exposes that group's
X socket at the conventional client path and excludes the trusted and other
group paths. Shared and confined profiles can use the same recipe text with
different Session-supplied namespace contexts and grants. Two identical path
views therefore need not share resources. Conversely, two aliases or explicitly
shared views do not create duplicate service identities. WM policy remains blind
to all of these identities.

### Launch, changes and retained handles

At launch, Session chooses the recipe revision and namespace. A child receives
the selected endpoint directory; for a new connection it authenticates and
obtains its own admission. Sharing a NamespaceId is explicit Session policy,
not a consequence of inheriting a pathname. An inherited authenticated stream
shares the existing admission, with the descriptor-transfer limit documented in
t133; it does not prove the child's identity or supervisor custody.

Recipes are sealed for an admission. A policy change constructs a new view and
revokes the old admission when authority must shrink. Unmounting or rebinding
a visible path alone cannot revoke an open stream. An old fid stays pinned to
its original owner, node and epochs; it may continue only while that old grant
remains valid, and never silently resolves to the replacement. Service shutdown,
grant revocation and output replacement invalidate the applicable generation
and answer ESTALE for further protected access. Pending reads recheck at
delivery. Already delivered bytes cannot be recalled.

For capture, output generation is separate from role-connection and namespace
identity. A returned monitor cannot inherit the old screenshot grant even if
its connector name and numeric OutputId match. A new grant has its own identity.
Session lock refuses new capture and cancels unfinished delivery through the
portal/capture owners. The compiler has no lock state and cannot enforce this
by hiding names alone.

### Implementation tests handed to t318

| Control | Required separate observations |
| --- | --- |
| Two private clients, same names | Different admitted trees; foreign open refused; no shared resource identity inferred from equal paths. |
| Explicit shared service, two aliases | One service identity, independent admissions/grants, no authority added by aliasing. |
| Rebind and unmount | New walks observe the declared new mapping; retained handles never retarget; revoked old handles fail. |
| Portal revoke, expiry, lock and disconnect | Pending and retained reads stop; unrelated grants survive; each resource is released once. |
| Same-name output replacement | Old generation-bound grant fails; new grant is independent even at equal numeric output identity. |
| Union collision/denied member | Deterministic directory order and refusal; no fallback around an access denial. |
| Include cycle, escape, changed input and limits | Whole recipe refused before publication; old view unchanged; no client file or socket opened by compilation. |
| Parent/child and forwarded stream | Separate connections get independently checked admission; a shared stream is explicitly the same identity. |
| Host reach | t142 proves trusted/other-group paths and inherited descriptors are excluded; tree visibility alone is insufficient. |
| Observer permissions | Inspection/capture grants grant neither ctl writes nor XTEST/9P injection; recipe names grant nothing. |

Retain compiled controls removing grant intersection, generation pinning,
retained-fid revalidation and final delivery cancellation. Pure compiler tests
are device-free; independent SDK/socket tests exercise the actual export.
Renderer pixel evidence belongs to t319, and physical acceptance follows a
matched release. None of these tests ran in this documentation slice.

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

The accepted one-core ADR supplies the architectural decision. Complete this
investigation with review of the worked recipe, ownership and threat boundary,
and concrete tests for composition, inheritance, isolation and revocation.
Identify implementation gaps and unsupported semantics. The October 10 draft
supplies that review material; implementation and acceptance remain separate
tasks and evidence, and writing the draft does not close this investigation.

## Connections

- [Namespace prerequisites for capture](id869143-x11-drawable-readback-and-an-operator-capture-path.md#namespace-prerequisites-for-capture-2026-10-09)
  map t133/t142 admission and confinement to t046's proposed capture path;
  the full namespace composition investigation is a separate design effort.
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

---
id: ernn0bkv
date: 2026-10-09
kind: concept
status: draft
tags: [concept, architecture, security, session, portability, namespaces, portals]
---
# Plan 9 integration points for Sophia

Plan 9's value to Sophia is one pair of ideas: a tiny uniform interface, walk,
open, read, write and clunk, and a per-identity namespace that composes it.
Sophia adopts that pair at two boundaries, the public role boundary and the
admission boundary, and nowhere else. Sophia Engine keeps its rendering, scene
and transaction ownership. The X authority, the WM and shell 9P2000.L role
contracts, and the planned 9P application authority keep their protocols and
their owners. Every integration recorded here is additive work in a known
place. The limit of the insight is that file contents remain a protocol: each
file needs a grammar, a version and a bounded parser. Small text controls are
useful where they fit; binary role records retain their existing contracts.

niltempus set this scope on 2026-10-09 during a brainstorm over a personal
design note, "Plan 9 Inspired Native Display Server Architecture", and the X11
readback investigation [id869143](../investigations/id869143-x11-drawable-readback-and-an-operator-capture-path.md).
The personal design note is not in the repository. The resulting
[one-core ADR](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
was accepted after review on 2026-10-09. This concept remains explanatory;
its cost estimates and unimplemented examples are not qualification evidence.

## What stays fixed

[Engine](../../architecture.md) owns physical input, the scene graph, visual
commits, rendering and scanout, and its transaction interface stays
protocol-neutral. The X authority remains the sole active application
authority. The WM and shell roles keep their binary runtime records and their
9P2000.L file contracts. The
[public-interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
stands: plain 9P2000.L, no Sophia message types, and desktop meaning in each
role's file namespace. The
[portal contract](../../namespaces-and-portals.md#portal-contract) stays a
reducer over bounded facts whose request carries no payload and no descriptor.
Descriptors never travel on the 9P stream. The
[lock image plan](../plans/r8z5a3sk-negotiated-dma-buf-lock-images-with-bounded-capture-and-release.md)
states that rule, and the DRI3 import path already follows it: plane
descriptors are read at the socket while pure dispatch never sees them.

## Three rules for namespaces, identity and the operating system

| Rule | Present in the code | What changes | Departure |
| --- | --- | --- | --- |
| A namespace is the tree a client sees at attach | `Export::attach` in `crates/sophia-9p/src/export.rs` receives an `AttachContext`. Six production exports implement it: factotum, lock files, inspection, output files, shell files and WM policy. | A composition layer, a declarative recipe, decides which exports an admitted identity sees and under which names. | Additive. One module about the size of one export. No export is rewritten. |
| Identity is proven at attach, not read from the socket | `AttachContext` carries peer credentials, uname and aname. The core refuses any afid with EINVAL at `crates/sophia-9p/src/connection.rs:489`. Factotum is ported, is itself an export, and serves unlock only. | Accept an afid, run the factotum conversation, and derive admission from that identity plus the supervisor's launch record. Peer credentials become a platform-optional second check. | Moderate and local: the attach path, the admission policy and launch records. X11 admission is unchanged. |
| Only device code touches the operating system | `libc` appears in three crates: `sophia-sysv-shm`, `sophia-linux-peer` and `sophia-factotum-pam`. Everything else uses rustix. The live backend is feature-gated behind libdrm, udev, libinput and libseat. | An audit under t256, not a restructure. | Small per item. |

The three rules sit at three boundaries. Everything between them keeps its
current owner.

```text
                  clients: X11 apps, 9P apps, WM, shell, CLI, agents
                                      |
 ADMISSION BOUNDARY ----------------- | ------  rule 2: identity proven at attach
                                      |          afid + factotum + launch custody;
                                      v          peer credentials become optional
 +-------------------------------------------------------------------+
 | sophia-session                                                    |
|   namespace recipe  -> endpoint directory per admitted identity  |  rule 1
 |   portal policy     -> reducer over bounded facts   (unchanged)   |
|   recipient executor -> bind in its role tree / X translation    |
 +-------------------------------------------------------------------+
                                      |
 PUBLIC ROLE BOUNDARY --------------- | ------  one 9P core, N exports
                                      |
   +--------+ +------+ +----------+ +------+ +-----+ +------+ +-----------+
   |factotum| |lock  | |inspection| |output| |shell| |WM    | |app        |
   |export  | |files | |          | |files | |files| |policy| |authority* |
   +--------+ +------+ +----------+ +------+ +-----+ +------+ +-----------+
                                                     * becomes the seventh export;
                                                       its own codec and FUSE go
 +------------------+       +--------------------------------------------+
 | X authority      |------>| SOPHIA ENGINE (unchanged)                  |
 | (unchanged)      |       | scene, commits, rendering, scanout         |
 +------------------+       +--------------------------------------------+
                                                     |
 OS BOUNDARY ---------------------------------------- | ------  rule 3: only device
                                                      v         code touches the OS
   backend-live: DRM/KMS, libinput, udev, libseat   |  renderer: EGL/GBM
   narrow adapters: sysv-shm, linux-peer, drm-clock, out-fence, xshmfence, pam
   per-OS containment driver: bwrap | jail + capsicum | unveil + pledge
```

### Namespaces and portals under rule one

Each export already builds its root from the attach context. The missing work
includes composition, authenticated admission and recipient executors. A 9P
recipient receives a grant-bound object in that role's tree; an X recipient
uses the X authority's protocol translation. Revocation, expiry, disconnect
or a session lock makes the capture fid return ESTALE. The policy reducer,
the seven `PortalTransferKind` values and the request lifecycle keep their
definitions; each kind still needs an executor and ownership proof.
Confinement divides into two halves that
the current bubblewrap policy mixes. Reach within Sophia is enforced by the
composed tree on every platform with no operating-system help. Reach into the
host stays a per-platform driver fed the same backend-neutral policy that
`crates/sophia-runtime/src/supervisor/protection.rs` already turns into
bubblewrap arguments.

Mount topology is a convenience for clients that need paths. It is never
authorization. The
[root-readback leak](../investigations/8xgoow54-a-root-readback-showed-one-namespace-anothers-windows.md)
was repaired in the authority, not with mounts, and the
[public-interface design](../../sophia-9p-control-bus.md#namespaces-complement-the-protocol)
says that hiding a path is not proof an open handle has lost authority. The
server checks attach, walk, open and every operation on a retained handle.

The first new bind is capture, t319, carved from t046. One grant materializes a small
directory for the requester holding a frame record, with dimensions, format,
the output or opaque window identity and the presentation generation, and a
frame file whose read returns bounded bytes. That is the small direct client
the [transport investigation](../investigations/5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
wanted after reading wmii, it answers the question in `id869143` of what an
operator or agent reads to verify a GUI, and it is the revoked-transfer proof
the [namespace investigation](../investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md)
asks for. The lock gate recorded under
[t046](../plans/queue-16-portals-and-confined-applications.md#t046) applies: a
capture is refused while the session is locked and none in flight completes
after the lock is applied. The dma-buf form of the same frame belongs to the
recording kind on a separate descriptor channel, as the lock image plan
proposes. An output is the first target; a window by opaque identity follows.

```text
requester's portal-role tree                grant lifecycle
 /
 ├── status      role status, one line        request  Pending -> Allowed
 └── capture/<grant>/                        grant    Active  -> bind into tree
     ├── frame.record   dimensions, format, target,
     │                  presentation generation
     └── frame          bounded bytes read over 9P; dma-buf only on
                        the recording kind's descriptor channel
                                              grant    Completed | Revoked | Expired | locked
                                                       -> unbind; the open fid answers ESTALE
```

Two X-side findings from `id869143` stay separate from the portal. Core and
MIT-SHM GetImage read CPU backing through `read_drawable_image` and composite
within the caller's namespace, so accelerated presentation does not imply
pixels for GetImage. Serving that case needs the same primitive the capture
executor needs: an immutable renderer-owned snapshot copied to bounded bytes at
the [renderer import boundary](../../renderer-import-boundary.md). Name that
primitive once and let both callers use it. The missing XkbBell request, t313,
is independent of capture and is not closed by accepting unknown minors.

### Identity under rule two

The accepted target authenticates the first attach and fixes one principal
and namespace for that connection. Every later attach must prove the same
identity and stay within its admitted grants; a different principal needs a
separate connection. Existing fids never change identity. A mount sharing one
connection therefore shares its admission, rather than treating each process
using the mount as a new authenticated client. The supervisor's launch record
proves custody only for a child it actually spawned. Lock-file
admission today hard-requires `SO_PEERPIDFD` in
`crates/sophia-runtime/src/lock_files/transport.rs`; under this rule that
becomes an optional check where the platform offers it. X11 clients have no
attach to authenticate and keep socket directories, peer UID and
namespace-keyed resource checks.

### Portability under rule three

Using rustix does not prove platform neutrality; syscalls and dependencies
still need the t256 audit. The live session names a libinput poller entry point
directly at `crates/sophia-session/src/live_session.rs:270`, where the profile
should select the backend. 9P2000.L's `Rlerror` carries Linux errno numbers by
dialect definition, so the error vocabulary must be Sophia's own enum mapped
per dialect and never an unchecked host errno. Plain 9P2000 requires explicit
adapters for the current .L open, stat, directory and error operations, plus
independent interoperability evidence. Its cost is not established. A future
BSD containment driver should take the same policy input as bubblewrap;
backend and dependency availability must be audited before selecting a port.
PAM is isolated in `sophia-factotum-pam`, the seam for any alternative adapter.

## Findings against the personal design note

The design note proposes a per-window directory of ctl, event, kbd, mouse and
buffer files, mount-namespace isolation, and a dma-buf descriptor delivered
inside the 9P `Ropen` reply over `SCM_RIGHTS`. Four points conflict with
accepted Sophia decisions and are recorded so they are not proposed again.

The descriptor must not ride inside the reply. The 9P stream carries typed
records; a separate socket capability carries descriptors. The reply-descriptor
design also makes any mounted client impossible, since no descriptor crosses a
9pfuse or v9fs mount.

Absence from a namespace is not authorization. The server check stays, for the
reasons given under rule one.

The WM must not see the tree. The design note hands the window manager every
application's directory. Sophia's WM is metadata-blind over opaque nodes and
never receives pixels, per the
[freeze surface](../../wm-v1-freeze-surface.md#brokers-and-portals). The
component that may see every output is the capture executor under an operator
grant.

The claim of simplicity relative to Wayland omits allocation ownership, format
and modifier negotiation, double buffering, acquire and release fences, damage
and a per-frame commit, and it treats keyboard and pointer byte streams as an
input contract without XKB state, repeat or text input. The
[application frontend design](../../sophia-9p-authority.md) lists these as
open; the design note does not answer them.

Two parts of the design note are worth keeping. Its reason to reject FUSE, that
a FUSE handle can never be the GPU buffer, is stronger than the context-switch
argument already recorded. Its memory row, dma-buf, LinuxKPI dma-buf, POSIX
shared memory and VMO handles, is the list of backing kinds a platform profile
should advertise, with CPU bytes first and dma-buf for the recording kind.

## Seven integration points beyond namespaces and portals

### 1. Control and status files for administration

Plan 9 has no ioctl. Control is a line written to a ctl file, state is a line
read from a status file, and both have a documented grammar. Sophia's
administrative command migration and the per-role status files under t257 are
this idea. It applies to the operator and agent surface: outputs, session
state, lock state and admitted clients. The sophia CLI then becomes cat and
echo over a mounted or forwarded tree, and an agent drives the desktop without
a special client. Frame-rate roles stay binary, as decided.

### 2. The plumber for URI open, file handoff and notification

Plan 9's plumber routes typed messages between programs by rules in a text
file, and each destination reads its port as a file. That covers three of the
seven portal kinds and supplies a rule language. Plumbing rules are a reducer
over bounded message attributes, so portal policy keeps its shape, and the
executor becomes a write to a port file that only the admitted recipient can
read. No prompt UI product is required for user-editable dispatch.

### 3. Application services delegated by bind

Acme exposes its buffers and commands as files, and other programs script it.
The application frontend design already allows an application to export its
own tree. Delegation needs no new mechanism: granting another client access to
an application service is the same bind into a namespace that portals use. One
mechanism covers transfers and service delegation, and it is how an agent
would drive a native application.

### 4. Recursive re-export

Rio serves each window the same device files the real devices have, so a
program cannot tell whether it talks to the kernel or to a window. The planned
application authority's per-window files should be designed so an intermediary
can re-serve them to its own children. The condition is that the primary
contract is bytes and records only, with descriptors as an optional socket
capability. That is already the rule, and this is the strongest reason to keep
it.

### 5. Factotum as the single authentication agent

Plan 9 used one agent for every authentication so keys never lived in clients.
Sophia's factotum port is already an export and serves unlock only. Extending
it to 9P attach, and later to SDK clients, is the identity change under rule
two and is a reuse rather than a new component.

### 6. Network transparency for inspection and administration only

Plan 9 mounts a remote machine's tree as naturally as a local one. For pixels
at frame rate that is a trap, and the
[performance section](../../sophia-9p-control-bus.md#performance-is-an-acceptance-question)
of the public-interface design says so. For the ctl and status tree it is
free: forward the socket over ssh and an agent on another machine reads the
desktop's state files. Identity still comes from the attach, not from the
transport, which is why rule two precedes this.

### 7. Discovery by the tree

Plan 9 needs no registry because the namespace root is the catalogue, and each
service has a version or ctl file at its root. This answers the first open
question in the public-interface design. A client lists its root to see which
roles it may attach to and reads one file per role for version and
capabilities. The recipe format can follow the Plan 9 namespace file, which
has five verbs, bind, mount, unmount, cd and include, and has not needed more.

Two Plan 9 parts are not imported. The draw protocol would only wrap a command
stream around composition that Engine already owns. The Plan 9 process model
is already matched by Rust threads and the owner loop.

## Protocol authorities and the primary native path

The 9P core is a transport. What hooks into Engine is a protocol authority,
and there should be exactly one way to be one. The X authority never imports
Engine; it emits the value vocabulary in `sophia-protocol`, and the 9P
application scaffold already targets the same vocabulary. What is missing is
the name: the session's run loop takes ten X-named types from the X authority,
observed transaction batches and CPU buffer updates on the way in, allocation
preferences and metadata candidates as proposals, routed input origin and
output update outcomes on the way out, pointer grab anchors and responses,
dma-buf import formats, service commands and the injection policy. None is
X-specific in meaning. Naming them once as the protocol authority contract,
with the X authority as first implementor and no behavior change, turns the X
wiring into one instance of a seam rather than the seam itself.

```text
  X11 clients          9P clients: WM · shells · output · lock ·        third-party protocol
                       admin · broker · portal · applications · agents   clients (any protocol)
       │                               │                                        │
       ▼                               ▼                                        ▼
  X authority                 ONE 9P CORE: one export per role           translator process:
  in-process crate            9P2000.L and plain 9P2000                  a 9P client of the
                              application authority = an export          application authority
       │                               │                                        │
       └───────────────────────────────┴────────────────────────────────────────┘
                                       │
                 PROTOCOL AUTHORITY CONTRACT (records in sophia-protocol)
                 ingress: admitted surface transactions, proposals, metadata candidates
                 egress:  routed input with origin, outcomes, topology, revocation
                                       ▼
                   session owners: admission · routing · policy  ──▶  ENGINE  ──▶  DRM/KMS

   identity proven at attach · one socket directory per identity with a discovery file
   binds land in the granting role's tree · descriptors on a side channel per role
   OS touched only by backend, renderer, adapters and the containment driver
```

Composition across roles happens where Plan 9 did it, in the client's
namespace, not in a server. Six role servers on six endpoints stay as they
are; the recipe decides which endpoints an identity's socket directory holds
and writes a discovery file; binds land in the granting role's tree. A
composing root that forwards into other owners' exports would collapse the
per-role reach boundary and is deferred until measured.

Three ways to hook in follow. Sophia's own authorities, X and the 9P
application authority, implement the contract in process, hosted like the
role exports with an owner, a thread, bounded queues and a wake. Third
parties hook in out of process as a 9P client of the application authority,
the model in which a translator is itself a client, which is why that
authority's files must carry a translator's many clients as sub-identities
with their own namespace contexts. An in-process trait over the contract is a
later option only if a measured need appears. niltempus recorded on
2026-10-09 that 9P on this core is the primary way to build native window
managers, shells, services and applications for Engine, that a Wayland
authority is not planned, and that other developers remain free to add a
protocol authority of their choice through the translator path.

## Guardrails against sprawl

There is one 9P core and everything else is an export on it. The application
authority scaffold in `crates/sophia-9p-authority` currently carries its own
decode and encode modules beside `crates/sophia-9p/src/wire.rs`, and its crate
header promises service over FUSE mounts, contrary to the no-FUSE rule for
core roles. Both go before any application API work, and the authority becomes
the seventh export. Each file has one grammar, a version and a parser that
refuses partial input. No new 9P message types are added. Descriptors stay off
the stream. Policy reduces and executors bind; no executor carries policy.
Operating-system code lives only in the backend, the renderer and the narrow
adapter crates, and the session selects a backend by profile rather than by
name. Every new mechanism absorbs an existing path rather than adding a
parallel one, consistent with the
[IPC removal inventory](../investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md).

## Agents as admitted clients

The end-state diagram places agents beside the WM, the shells and the CLI as
9P clients. That placement is deliberate. An agent is not a new kind of
client. It is an admitted connection with an identity proven at attach and a
tree composed for that identity. It reads status files, writes one-line ctl
commands, blocks on event files and requests portal grants, which is how a
shell script drove wmii and is where the file idiom pays off: an agent needs
no SDK, no protocol of its own and no feature built for it.

An agent's authority is its tree and nothing more. There is no ambient
permission, so agent power is defined as recipes rather than code paths.

| Recipe | Explicit grants | Absent without another grant |
| --- | --- | --- |
| Observer | Status and inspection files, capture grants on named outputs | Input injection, administration, WM policy |
| Operator | Selected administrative ctl files and clipboard grants | Capture, input injection, WM policy |
| Driver | Input injection on one named namespace | Capture, administration, other namespaces |

Input injection is the line that matters. Capture never implies it, XTEST
admission is per namespace and separately granted, and the invariants forbid
synthetic input as a side effect of anything. These recipes do not inherit
one another. The default observe-and-act workflow uses two identities on
separate connections; an operator may explicitly compose grants for one
identity. Neither an identity's name nor its recipe label grants authority.

Every agent action is attributable and revocable. It happens on a fid tied to
an attach, the 9P journal records it, and revocation makes the fid answer
ESTALE in the middle of a task. That is stronger than a CLI running as the
user with ambient rights, and it bounds prompt injection: a hostile window
title or clipboard payload the agent reads cannot grant it anything the recipe
did not. The agent is itself confined. It runs arbitrary code, so it lives in
its own namespace with host reach cut by the containment driver and touches
Sophia only through its composed tree. Because identity comes from the attach
rather than the socket's peer, the agent may sit on another machine over a
forwarded socket.

Sophia builds nothing agent-specific. The shell and WM independence rule
applies: no agent harness, no named-agent client, no feature for one agent.
Sophia ships the generic files, the small CLI and the conformance peers, and an
agent is simply the most demanding generic client, which makes it a useful
test of whether the contract is generic.

The development loop changes with it. The readback investigation that began
this note arose because an agent could not verify a GUI without screenshot
workarounds. With inspection, status files and the capture bind, an agent
verifies a desktop change by reading a frame record, comparing known pixels
and checking a presentation generation, and the headless VKMS candidate gives
CI the same path. niltempus agreed on 2026-10-09 that Sophia ships the
observer recipe, that operator and driver recipes are composed by the
operator, and that the driver tier stays out of the installed daily session
unless a task names it.

### What the observe and act split replaces in X11

The agent model is a better XTEST only in the sense that it separates what X11
lumps together. XTEST is one of three X mechanisms, and the tiers map onto all
three.

| X11 today | What it gives | Sophia end state | What changes |
| --- | --- | --- | --- |
| XTEST | Injects pointer and key events. Ambient: any connected client may. No audit, no revocation, no target binding; events land wherever focus is. | Driver tier | Injection is a per-namespace admission granted to one identity, journaled, revocable in the middle of a task, and routed by Engine against presented state like physical input. Sophia already gates XTEST this way with `--admit-xtest`. |
| GetImage, xwd, scrot | Sophia's current readback uses CPU backing and does not establish accelerated or composed-output pixels. | Capture bind | One grant yields a frame record with a presentation generation and bounded pixels from the composed output, refused while locked, revoked by ESTALE. |
| XRecord, xprop, xwininfo | Watches protocol traffic and reads window state with full metadata. | Inspection and status files | Sanitized, bounded, read-only records per role, with no metadata reaching a blind WM and no acknowledgement-floor cost on it. |

The target puts observation and input behind explicit grants and attributable
operations, with separate identities in the default workflow. Permission to
connect to a display is insufficient authority to inject input. Authentication
does not prevent an explicitly authorized client from misusing its grants.

Three limits keep this from being oversold. XTEST itself remains the X-side
surface for clients such as xdotool, gated by namespace admission; the driver
tier is XTEST admission for X plus an equivalent for 9P applications. That 9P
equivalent is not yet designed: the Plan 9 answer is to write events into an
admitted window's input file, served by the application authority and routed
by Engine against presented targets, and t320 in the
[convergence plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t320)
owns that design on the critical path. And injection stays out of the
installed daily session unless a task names it; an agent on the live desktop
observes and does not act.

## Where the cost lands

Composition and authentication act at attach; a portal bind acts per grant.
Bounded screenshot bytes do cross 9P, while GPU descriptors use their separate
channel. X clients keep DRI3 and shared memory, and WM and shell roles keep
binary records. Engine ownership is unchanged, but capture can require GPU
waits, readback and copies that compete with rendering. There is no guarantee
of unchanged frame latency. The costs to bound and measure include these.

| Where | Cost | Cadence |
| --- | --- | --- |
| Attach with factotum | One authentication conversation | Per connection |
| Walk against the composed tree | Slice matching with no inodes or caches | Per path lookup, never per frame |
| Capture frame read | Snapshot completion, bounded readback, copies and delivery | Per grant, with deadlines and byte limits |
| Ctl and status files | Parsing one line | Per operator command |
| Mounted clients through 9pfuse | Extra kernel context switches | The reason core roles connect directly |

The repository's existing rule is unchanged by this note: no 9P migration
retires an old interface until its latency distribution, CPU, allocations and
copied bytes are measured against the current owner for the same workload, as
the [performance section](../../sophia-9p-control-bus.md#performance-is-an-acceptance-question)
of the public-interface design requires.

## What is Plan 9 here and what is not

The namespace is assembled per identity at attach, which is rfork and bind.
Identity comes from an authenticated attach through factotum, which is Tauth.
Everything above the device layer speaks one file protocol, and only the
device layer knows the operating system, which is how the Plan 9 kernel was
divided. Above that sit ctl and status files, the plumber, Acme-style
delegation, Rio-style re-export and discovery by the tree.

The departures are deliberate. There is no draw protocol, because Engine's
transaction model is the part that survives GPUs and server-side drawing is
the part that did not. There are no kernel mounts as authority, because the
server check is the authority. Screenshot pixels are bounded file reads;
GPU-buffer transfers use handles on a separate descriptor channel. The result
borrows Plan 9's structure with a modern
transactional compositor in the middle.

## Sequence

The immediate monitor-continuity application is recorded in the
[t310 plan](../plans/cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md#persistent-services-and-replaceable-display-attachments)
and [physical-return investigation](../investigations/ig4obtxu-copies-and-rasterization-dominate-after-kms-mapping-retention.md#second-bare-metal-return-sleeping-gpu-and-incomplete-validation-2026-10-09).
Session and application identities outlive unplugged displays; native owners,
input routing and presentation grants are tied to their physical generation.
Admitted slow probing and complete-plane validation shipped in release 222,
which passed the attended cable and KVM returns, including while locked.
This borrows the service-lifetime idea without replacing Engine or pretending
that a disconnected display presented a frame. Status/discovery and revoked
capture fids remain scoped work under t257/t318/t319, separate from this release.

The [delivery plan](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#first-deliverable-and-dependency-order)
now owns the dependency sequence: finish t310's output publication obligations,
prove admission and confined groups, compose the observer view, then deliver
one authorized output screenshot. The portal foundation does not wait for
clipboard execution through a future 9P application frontend. Native
applications, input driving, other portal kinds and portability are separate
deliverables. Task lanes remain in todo.md; t307 is parked until a physical
failure or a new VM-testing goal warrants resuming it.

## Limits

Whether the owner loop uses poll or epoll beneath `sophia-wake` was not
verified. BSD library availability is general knowledge, not a build. The
departure estimate, low thousands of lines added, a few hundred changed and
nothing deleted in the first step, is an estimate from the export sizes.

## Connections

- [Serve every public role from one 9P core](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
  is the accepted decision this reasoning led to.
- [Adopt 9P2000.L as the public interface](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
  governs the transport direction this note builds on.
- [Keep broker and portal file authority and custody separate](../decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
  owns the portal authority boundary the bind model preserves.
- [Public interfaces over 9P2000.L](../../sophia-9p-control-bus.md) and the
  [9P2000.L profile](../../sophia-9p-profile.md) own the dialect and operation
  subset.
- [Namespaces and portals](../../namespaces-and-portals.md) owns the existing
  isolation, admission and portal contracts.
- [Sophia 9P application frontend](../../sophia-9p-authority.md) and
  [protocol frontend candidates](../../protocol-frontend-candidates.md) own the
  planned application authority.
- [Plan 9 in the session control plane](../investigations/5kqzwmi5-plan-9-belongs-in-the-session-control-plane-not-the-engine.md)
  and [adopting the Plan 9 namespace model](../investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md)
  are the earlier investigations this concept consolidates.
- [Portals and confined applications](../plans/queue-16-portals-and-confined-applications.md),
  [authority and lifecycle hardening](../plans/queue-13-authority-and-lifecycle-hardening.md)
  and [desktop role migration](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  hold the tasks named in the sequence.
- [Headless validation with VKMS writeback](../investigations/jweorh0z-headless-sophia-validation-and-capture-with-vkms-writeback.md)
  can supply a virtual-device proof for the capture bind without being a
  prerequisite for it.
- Task state lives in [todo.md](../../../todo.md).

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
file needs a grammar, a version and a bounded parser, and Plan 9's own
simplicity came from keeping those grammars to one line each.

niltempus set this scope on 2026-10-09 during a brainstorm over a personal
design note, "Plan 9 Inspired Native Display Server Architecture", and the X11
readback investigation `id869143`. That investigation sits on the t310 and t306
lane branches at the time of writing, so its findings are summarized here
rather than linked. The design note is not in the repository.

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
 |   namespace recipe  -> composes one tree per admitted identity    |  rule 1
 |   portal policy     -> reducer over bounded facts   (unchanged)   |
 |   executor          -> bind / unbind one object into that tree    |
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

Each export already builds its root from the attach context, so composition is
the only missing layer. With it, every portal kind has one execution model. An
allowed grant binds one object into the requester's tree for the grant's
lifetime. Revocation, expiry, disconnect or a session lock makes the open fid
return ESTALE. The policy reducer, the seven `PortalTransferKind` values and
the request lifecycle keep their current definitions; the executor is bind and
unbind plus one object type per kind. Confinement divides into two halves that
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

The first bind should be capture, t046. One grant materializes a small
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
 requester's tree (composed at attach)        grant lifecycle
 /
 ├── status      role status, one line        request  Pending -> Allowed
 ├── ctl         one-line commands            grant    Active  -> bind into tree
 └── portal/
     └── capture/<grant>/                     read the record, read the bytes
         ├── frame.record   dimensions, format, target,
         │                  presentation generation
         └── frame          bounded bytes; dma-buf only on the
                            recording kind's descriptor channel
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

Plan 9 authenticated every attach because one server served local and remote
clients over the same protocol. Sophia has the same need in a different form.
A mounted client may share one socket's peer credentials among several
processes, which the public-interface design records as an unresolved
contract requirement. An afid conversation settles identity per attach
regardless of transport. The supervisor's launch record, the process
descriptor it holds for a child it spawned, settles custody. Lock-file
admission today hard-requires `SO_PEERPIDFD` in
`crates/sophia-runtime/src/lock_files/transport.rs`; under this rule that
becomes an optional check where the platform offers it. X11 clients have no
attach to authenticate and keep socket directories, peer UID and
namespace-keyed resource checks.

### Portability under rule three

The compositor core is already platform-neutral through rustix. The remaining
items are the t256 audit. The live session names a libinput poller entry point
directly at `crates/sophia-session/src/live_session.rs:270`, where the profile
should select the backend. 9P2000.L's `Rlerror` carries Linux errno numbers by
dialect definition, so the error vocabulary must be Sophia's own enum mapped
per dialect and never the host's errno. Plain 9P2000 beside 9P2000.L is cheap
because role contracts already restrict themselves to the common operation set.
A BSD containment driver takes the same policy input as bubblewrap. FreeBSD is
the first target because libinput, udev and libseat exist there; OpenBSD also
needs a wscons input backend. PAM is isolated in `sophia-factotum-pam` with the
libpam binding in its own module, which is the seam a bsd_auth module would use.

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

| Tier | The recipe binds | The agent cannot |
| --- | --- | --- |
| Observer | Status and inspection files, capture grants on named outputs | Inject input, read another namespace, propose layout |
| Operator | Observer plus administrative ctl files and clipboard grants | Become the WM, see pixels without a grant |
| Driver | Operator plus XTEST admission on one named namespace | Reach the trusted namespace, act after revocation |

Input injection is the line that matters. Capture never implies it, XTEST
admission is per namespace and separately granted, and the invariants forbid
synthetic input as a side effect of anything. An agent that can see and an
agent that can act are two identities.

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
| GetImage, xwd, scrot | Reads a drawable's CPU backing. Misses accelerated content and the composed desktop. | Capture bind | One grant yields a frame record with a presentation generation and bounded pixels from the composed output, refused while locked, revoked by ESTALE. |
| XRecord, xprop, xwininfo | Watches protocol traffic and reads window state with full metadata. | Inspection and status files | Sanitized, bounded, read-only records per role, with no metadata reaching a blind WM and no acknowledgement-floor cost on it. |

The honest description is XTEST, GetImage and XRecord placed behind one
identity, one grant model and one audit trail, with observing and acting held
by different identities. XTEST's flaw is not that it injects input; it is that
anyone who can open the display may do so silently, with no way to tell
afterwards.

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

The three rules act at connect time and at operator cadence, not per frame.
Composition and authentication happen once at attach. A portal bind happens
once per grant. Pixels never cross 9P: X clients keep DRI3 and shared memory,
the recording kind uses a dma-buf on its own descriptor channel, and the WM and
shell roles keep binary records. Engine's scene, commits, rendering and scanout
are untouched, so the design cannot make rendering slower. The costs it does
add are these.

| Where | Cost | Cadence |
| --- | --- | --- |
| Attach with factotum | One authentication conversation | Per connection |
| Walk against the composed tree | Slice matching with no inodes or caches | Per path lookup, never per frame |
| Capture frame read | One bounded copy from a retained snapshot | Per grant |
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
server check is the authority. Pixels move as GPU handles on a descriptor
channel, not as file reads. The result is Plan 9's structure with a modern
transactional compositor in the middle.

## Sequence

This note creates no task and promotes none. The order follows the open tasks
that already exist. Identity comes first: reconcile the
[admission investigation](../investigations/1pv291te-namespace-and-client-admission-security-gaps.md)
and the [pidfd proposal](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md)
under t133 with production admission, alongside the t256 audit. Composition
follows under t142, the
[several-listener frontend](../plans/ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md),
and t275, the namespace recipe. The first bind is the output-only capture slice
of t046, then the confined daily group under t045. The t257 status files and
the t313 XkbBell fix proceed independently. Monitor recovery remains the active
implementation priority.

## Limits

Whether the owner loop uses poll or epoll beneath `sophia-wake` was not
verified. BSD library availability is general knowledge, not a build. The
departure estimate, low thousands of lines added, a few hundred changed and
nothing deleted in the first step, is an estimate from the export sizes.

## Connections

- [Serve every public role from one 9P core](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
  is the proposed decision this reasoning led to.
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

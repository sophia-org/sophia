# Scripting Sophia

**Role:** normative architecture and CLI contract.
**Status:** experimental session control service and `sophia msg` implemented
for registered policy actions and confirmed WM restart. Control is disabled by
default; `session { control "host-admin"; }` enables it at session startup.
Profile reload, shell commands, and delegated access remain future work.

Sophia supports the architectural goal of a scriptable display server with
replaceable window-manager and shell clients. Scripts request deliberate
changes through a session-owned interface. Each authority interprets its own
operations, and Engine retains validation, scene state, input, rendering,
presentation, and scanout.

This contract applies to any conforming WM or shell. It requires no particular
client executable, implementation language, or private command vocabulary.
[Architecture](architecture.md) owns the authority boundaries;
[Namespaces and Portals](namespaces-and-portals.md) owns resource isolation
and transfers; the [native protocol family](sophia-policy-ipc.md) owns shared
wire and lifecycle rules. This document owns scripting-specific requirements
within those contracts. The [control schema](../protocol/sophia-control-v1.kdl)
and companion wire specification define the first independent-client surface.

## Current And Proposed Support

| Capability | Current state | Extension boundary |
| --- | --- | --- |
| Named WM actions | The WM registers bounded action names and ordinals; existing policy requests carry argument-free actions | Implemented discovery and invocation through the session endpoint |
| WM restart | Scripted restart completes after the intended replacement commits a usable policy | Profile reload remains unadvertised pending transactional repair |
| Shell behavior | Negotiated descriptor, candidate, presentation, and activation exchanges | A generic shell command catalog needs a separate protocol extension |
| Parameterized commands | The WM action path has no arbitrary argument payload | Specify a bounded argument contract before adding setters |
| State queries and event subscriptions | Role snapshots serve their admitted recipients | A scripting disclosure and subscription contract is future work |
| Public control endpoint | Explicit host-admin opt-in, bounded worker, typed owner settlement | Delegated callers need a separate authorization contract |

The existence of a session operation or role message does not make it a public
scripting API. In particular, a shell activation acknowledgement is not a
general command-injection mechanism.

## Ownership And Routing

```mermaid
flowchart LR
    Script[Script or CLI] --> Admission[Session control admission]
    Admission --> Owner[Responsible authority]
    Owner --> Validation[Validation and settlement]
    Validation --> Result[Correlated result]
    Result --> Script
```

| Owner | Responsibility |
| --- | --- |
| Session runtime | Endpoint creation, caller admission, command authorization, routing, supervision, revocation, and bounded transport I/O |
| WM client | Registered spatial-command semantics, private model changes, and layout/focus proposals |
| Shell client | Shell presentation policy and permitted interactions within its negotiated role |
| Engine | Authoritative input, scene validation, atomic visual commit, rendering, presentation, and scanout |
| Input, output, session, and portal services | Their existing device, topology, process, capture, and transfer operations under the corresponding authority contracts |

The runtime routes a spatial command without learning the WM's layout
semantics. The WM reduces the admitted action against its private state and
returns a proposal through its existing role connection. The same validation
and settlement rules apply regardless of whether a request originated from a
physical binding or an authorized script. Script provenance must not be
represented as a fabricated physical keypress or trusted user gesture.

No policy process may own a client-facing endpoint. WMs and shells connect to
the session; they do not listen for scripts. The public control endpoint must
be separate from the supervised role endpoints. An admitted scripting client
does not become a WM, shell, or broker peer, and cannot forward their protocol
messages or capabilities as commands.

Scripting also creates no direct WM-to-shell channel. Role-to-role effects
continue through their existing authority paths, including Engine commits and
broker-mediated activation. Session reload, restart, and launch retain their
session ownership even when a WM exposes bindings that request them.

## CLI

```sh
sophia msg commands
sophia msg policy 'registered action name'
sophia msg session restart-wm
sophia msg session reload-profile
sophia msg session logout
sophia msg --json commands
```

An explicit `--socket /absolute/path` overrides `SOPHIA_CONTROL_SOCKET`.
The session exports that variable to its host application launches. There is
no guessed endpoint. Human-readable output is the default; `--json` uses the
independent reference client's catalog/outcome shape. Exit status is 0 for
successful owner settlement, 1 for a server-reported failure, and 2 for local,
transport, or uncertain-reply failure. The client never retries a mutation.
`reload-profile` is reserved on the wire but is not advertised or dispatched.

`commands` discovers the operations available to the authorized caller in the
current session. Policy names come from the active WM's admitted catalog;
Sophia must not maintain a second list of WM-specific verbs. Public callers
select names, rather than supplying internal action ordinals, session tokens,
or target handles. Discovery is bounded and does not disclose application
metadata or the WM's private model.

The first policy-command scope is existing argument-free actions. A WM can
provide steps, preset cycles, or bounded configured slots through that catalog.
Absolute setters and other parameterized commands require a separately
specified argument contract: transport bounds belong to Sophia, while spatial
meaning and value validation belong to the WM. Arbitrary payload forwarding
must not stand in for that contract.

Shell discovery and invocation are future extensions. They require negotiated
commands and outcomes appropriate to the shell role. Script access must not
turn a shell into a placement authority or authorize it to select an arbitrary
application by identity. Targeted activation retains its existing broker,
recipient, generation, and presentation requirements.

Invocation, state queries, and event subscriptions are separate permissions.
Permission to change a workspace does not imply permission to enumerate
window titles, read clipboard contents, capture pixels, or subscribe to
application activity. General process execution and synthetic input are also
outside the first command scope.

## Namespace And Caller Security

Three boundaries apply independently:

| Boundary | What it enforces |
| --- | --- |
| Sophia resource namespace | Access to X resources, properties, selections, events, and cross-namespace transfers |
| OS process protection domain | Access to host files, sockets, inherited descriptors, debugging, and other ambient channels |
| Scripting authorization | Which control operations a particular admitted caller may request |

A Sophia namespace is not an OS sandbox. Selecting the confined namespace
profile alone does not prove a process cannot reach a host control socket.
The current confined launcher allocates a namespace for a client group; it
does not automatically isolate every application into a separate namespace.
Classic-shared deliberately permits shared-X access within its group.

X admission, matching UID, possession of a namespace ID, role membership, or
discovery of a socket path does not implicitly grant scripting authority.
Scripts need not be X clients. Where a command needs a namespace context, the
session must derive it from verified admission and authorization; a namespace
field supplied by the caller is never proof. A new control connection must
not inherit an unrelated X connection's permissions merely because their
credentials appear similar.

Two useful authorization scopes must remain distinguishable:

- **Namespace-scoped automation:** acts within the caller's admitted resource
  scope. Cross-namespace access requires the appropriate explicit authority
  or portal flow. A global policy action that cannot honor this scope must be
  refused, rather than silently dispatched as desktop administration.
- **Desktop administration:** deliberately grants control over global desktop
  behavior, such as arranging windows across outputs or changing workspaces.
  These operations may affect windows from several namespaces. They do not
  grant access to those applications' data.

A host-user access mode can deliberately authorize reachable, unconfined
processes running as the session user. That trusts all such processes, not
only a terminal or a selected executable. Owner-only filesystem permissions
exclude other users; they do not provide a same-user application allowlist.
This mode must be named explicitly and must not be advertised as
namespace-restricted scripting. Control v1 selects this mode only through an
explicit host-administration opt-in; the default is disabled. On Linux the
service pins the socket's peer with `SO_PEERPIDFD`, checks the session UID and
current credentials, and compares pinned user, mount, and PID namespaces.
Sophia's protected roles occupy different namespaces and receive neither the
socket mount nor inherited control descriptors. A supplied socket path does
not bypass admission. Admission is checked again before command dispatch.

These checks establish the supported Sophia/Linux boundary. They do not attest
arbitrary third-party confinement: seccomp, Landlock, cgroups, or another
sandbox sharing all three namespaces do not by themselves exclude a caller.
An authorized host process can proxy its authority. The mode trusts that host
user domain; it supplies neither per-executable grants nor per-message sender
identity. Missing kernel/proc prerequisites disable the endpoint while the
desktop continues. See the [wire admission contract](sophia-control-v1.md#scope-and-authority).

Selective grants require an enforceable caller boundary. They cannot establish
isolation from an unconfined process that can replace authorized tools, read
their credentials, or modify the session's configuration. Confined callers
need explicit delegation and verified protection against reaching or
inheriting broader host authority. Grant issuance and delivery remain
prerequisites for a future delegated-access extension, outside control v1.

The session keeps caller credentials, namespace identity, and authorization
decisions outside the WM. The WM receives only the permitted opaque action
and role facts. Shell command handling retains its negotiated disclosure
limits. Existing clipboard or capture permissions do not authorize desktop
control, and desktop control does not authorize those transfers.

## Completion, Recovery, And Cost

An admitted mutation must retain a correlation identity through dispatch and
the responsible owner's terminal outcome. Requests bind to the applicable
session, recipient, catalog, and authority generations. Admission order is
preserved for ordered commands; relative changes must not be coalesced as if
they were replaceable state updates.

Accepted means admitted for processing. Committed means the responsible
authority settled the change. Presented means the relevant visual state
reached its presentation boundary. These are different outcomes. A CLI must
not report completion merely because it wrote bytes, queued an action, or
observed a replacement process start. No-op success must be explicit where
the owner supports it.

Rejected, stale, revoked, unavailable, overloaded, and timed-out operations
need machine-readable outcomes with bounded diagnostics. A disconnect after
dispatch may leave the caller unsure whether a mutation completed; report
that uncertainty rather than claiming the command had no effect. Automatic
reconnect must not replay an ambiguous mutation. Cancellation must not be
reported as undoing work that already committed.

Role replacement invalidates old recipient/catalog identities. Security
transitions and session teardown revoke affected admission and pending work
under their owning lifecycle rules. Unauthorized or malformed requests must
have no side effects. Failures preserve the last coherent state according to
the owner's recovery contract.

Before exposing reload through this interface, verify its
owner's validation, checkpoint, rollback, and completion behavior. Structural
profile validation alone cannot certify settings whose semantics belong to
the WM. A rejected candidate must preserve or restore the working state;
starting a replacement process is not proof that it restored a usable policy.

Transport must bound message sizes, concurrent connections, outstanding
requests, subscriptions if later admitted, and diagnostic volume. Parsing and
blocking socket I/O stay outside input and rendering work. Session servicing
must enforce fair work budgets so a slow reader or command flood cannot
starve physical input, frame service, security transitions, or recovery.
The hot path must not spawn a process per request or acquire unbounded queues.
The [control v1 bounds](sophia-control-v1.md#resource-and-scheduling-contract)
fix transport ceilings. Measurable aggregate service budgets still require
implementation acceptance evidence.

## Guidance For Independent Clients

WM authors keep their action catalog and semantics inside the policy client.
They use the existing action/projection path and preserve commit-aware private
state; they need no scripting listener or metadata access. Adding a command
name does not automatically make every caller eligible to invoke it.

Shell authors use only negotiated shell facts, candidates, and interactions.
A future command extension must state its permitted effects, disclosure,
settlement, and recovery before it can be advertised. Unsupported command
families must fail explicitly rather than fall back to another role's socket.

Scripting-client authors discover available names and support, handle terminal
outcomes, and treat replacement or reconnect as a fresh authority context.
The interface must be implementable from its published contract and
schema using ordinary Unix IPC, without a required SDK or a reference WM or
shell executable. The CLI is a convenience client of that interface.

## Verification And Remaining Work

`tools/check_control_protocol.sh` checks generated vectors, Rust and independent
Python clients, the real endpoint, config opt-in, sequencing, cancellation,
deadlines, and queue pressure. `--live-owner` also runs the namespace denial
proof and a supervised policy fixture through the live session's action and
restart settlement paths. These tests need Linux Unix sockets and bubblewrap;
they do not install software or launch a graphical session.

A short optional installed-session smoke discovers commands, invokes one safe
registered action, and requests restart while checking the desktop remains
usable. Physical input/frame fairness under sustained traffic remains an
operator observation, not a claim established by codec tests. This feature
adds no 36-row acceptance gate. Reload rollback, delegated grants, shell
commands, parameters, queries, and subscriptions need their own implementation
and evidence before publication. Existing WM and shell wire capabilities are
unchanged.

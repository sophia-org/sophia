# Sophia Control v1

**Role:** normative authority, catalog and settlement contract for scripted
administration.
**Status:** experimental; Linux session endpoint, `sophia msg`, policy
settlement, confirmed WM restart, `reload-profile` and `logout` are
implemented. Control remains disabled by default.

[Scripting Sophia](scripting.md) owns the generic authority contract.
[Administrative file records](sophia-admin-files.md) own the wire: record
grammar, file lifecycle, limits and transport errors. This document owns the
meaning of admission, catalogs and outcomes. The `sophia_control_v1` socket
envelope, its schema, generated tables, vectors and Python client are retired
(t255, 2026-11-19); Sophia neither serves nor speaks that wire.

## Scope And Authority

The session runtime owns a separate Unix `SOCK_STREAM` control endpoint.
Scripts are not role peers. No WM or shell listens for commands; callers
cannot send role messages, internal action ordinals, Engine transactions,
tokens, or application handles through this endpoint. Engine still validates
and commits visual state; the WM still interprets spatial policy.

The endpoint exposes discovery and invocation of argument-free registered WM
actions and the session operations `restart-wm`, `reload-profile` and `logout`,
with the settlement rules below. The owner is `policy` or `session`. Shell
commands, parameters, process execution,
synthetic input, metadata queries, event subscriptions, FD transfer, and
delegated grants are absent. No generic extension is reserved for them.
Future support requires a separately specified contract.

Control is **disabled by default**. The desktop profile's session owner accepts
`control "disabled"` or `control "host-admin"`; this is startup-only access
policy. CLI/environment options cannot enable the listener. A profile reload
cannot change the running session's control admission mode.

Linux admission requires the kernel peer UID to match the session user and a
socket-derived `SO_PEERPIDFD`. The service checks that pidfd before and after
opening the peer's proc directory, retains both descriptors, and compares the
peer's user, mount, and PID namespaces against pinned session namespace
handles. It checks all four current UIDs and pidfd liveness again before
dispatch and excludes the supervised WM identity. Sophia's protected role
launches use distinct namespaces, explicit mounts, cleared environments, and
FD allowlists; they cannot acquire this endpoint through their role grants.
Missing pidfd/namespace/proc prerequisites leave control disabled without
preventing desktop startup. Dead or unverifiable peers are denied.

This is a concrete Sophia/Linux protection boundary, not attestation of every
OS sandbox. A third-party sandbox sharing the session's user, mount, and PID
namespaces can satisfy admission even if it applies seccomp, Landlock, or
cgroup restrictions. Owner-only permissions and UID alone are insufficient;
caller-supplied PID, namespace IDs, executable names, and paths are not proof.

This mode trusts **all reachable processes of the session user in the admitted host domain**,
including a browser running in that domain. A Sophia resource namespace alone
does not establish OS confinement. Excluding protected processes also needs
enforced socket/descriptor isolation: authenticating the original connector
cannot prevent an authorized process from forwarding its open stream.
Do not claim per-message sender authentication from connection credentials.
Delegation to confined callers needs a later contract, not weaker admission.

The runtime filters discovery and rechecks authorization at admission and
immediately before dispatch. An observed admission loss denies or closes the affected request and prevents
undispatched work from running. Credentials authenticate the connector; they
cannot atomically attest arbitrary process transitions after inspection. It cannot undo committed work.
Caller identities never cross into WM policy; only the admitted opaque action
uses the existing WM request/projection path. Desktop control grants no clipboard, capture, title, or other
application-data access.

## Discovery

A client uses an explicit `--socket PATH` or `SOPHIA_CONTROL_SOCKET`; an explicit
path wins. The value must be an absolute filesystem path. No X property,
role endpoint, guessed socket, abstract socket, or TCP fallback is allowed.
The session creates a fresh random directory (0700) under the existing private
`XDG_RUNTIME_DIR`, binds `admin.9p` (0600), and exports its absolute path to
host application launches only when control is enabled. Socket and peer
descriptors are close-on-exec. Session teardown removes its own endpoint. Possession of the path conveys no authority.
Clients should check server peer UID against their expected session user;
that check cannot defend against another already trusted host-user process.

## Catalog And Invocation

The catalog is one complete snapshot with a nonzero generation; its
[record form](sophia-admin-files.md#discovery-records) and size bound belong to
the file contract. Empty catalogs are valid; truncating a larger catalog
without reporting an admission failure is not. An owner catalog exceeding
capacity must be refused before publication. Discovery must not wait for WM work.

Names use the existing WM action grammar: 1–128 ASCII bytes drawn from letters,
digits, space, `-`, `_`, and `.`, with no leading or trailing space. Names
are case-sensitive and never interpreted as shell text. Entries are unique by
`(owner, name)` and sorted by owner then ASCII name bytes. Policy entries
complete at policy commit; session entries complete at session settlement and
name `logout`, `reload-profile` or `restart-wm` when the session offers them.
An invocation absent from the catalog is `rejected`. Session-operation binding slots and launch
bindings must not be advertised as policy commands to bypass owner routing.

The generation identifies an immutable authorized catalog and its private
recipient/action mapping. Allocate nonzero generations monotonically within
the session; never reuse one for a different mapping or authority context.
WM replacement, profile/action mapping changes, and authorization changes
invalidate affected catalogs even when the visible names are unchanged.
Connections may share a generation only when their authorized mappings match.
Catalog IDs reveal no WM-private model or application identity. No generation
survives reconnect as a client cache: rediscover on every new connection.

An invocation names an owner and command from an exact previously returned catalog
generation. No discovery, a stale generation, or a changed recipient before
dispatch yields `stale`; no command runs. A valid current generation with a
name absent from its catalog yields `rejected`. Authorization loss before a
request is accepted refuses the file operation with `EACCES` and no request
exists. Loss after acceptance yields `denied` if a reply can still be
delivered, otherwise the connection closes.
Unknown owners are malformed, not extensible command families.
The runtime resolves the private action mapping once, pins it through
dispatch/settlement, and must never retarget the action to a replacement WM.

Only per-connection order is promised to clients. The session gives admitted
ordered mutations a serial dispatch order consistent with its owner queues;
two independent connections do not establish a caller-visible total order.
Relative actions are never coalesced or automatically replayed. A busy owner
may cause a bounded wait or `overloaded`, not unbounded parallel work.

## Settlement And Failure

An outcome echoes the **requested** catalog generation, including for
failure. Its optional detail is 0–256 bytes of valid UTF-8 with
no NUL or control characters. Diagnostics must exclude application metadata,
private handles, credentials, and raw owner output. Clients branch on enums,
never diagnostic prose. CLI JSON renders these fields by name; JSON is not the
server wire.

| Outcome | Required meaning |
| --- | --- |
| `committed` | Policy action's correlated projection passed Engine validation and committed, including an accepted no-op projection |
| `completed` | Session operation reached its owner settlement boundary |
| `unchanged` | Reload validated an equivalent candidate and retained the usable active profile |
| `rejected` | Semantic validation failed; the owner's coherent state was preserved/restored |
| `stale` | Catalog or recipient changed before dispatch; no action dispatched |
| `denied` | Authorization refused before dispatch; no action dispatched |
| `unavailable` | Owner unavailable before dispatch; no action dispatched |
| `overloaded` | Capacity refused before dispatch; no action dispatched |
| `timed-out` | Deadline expired and the owner proves no mutation took effect or can take effect later |
| `indeterminate` | Dispatch occurred but definitive settlement/effect cannot be established |

A policy `committed` is not physical display retirement. A session reload
`completed` means the candidate was validated by the relevant owners, activated,
and reached a usable policy cycle. Structural parser acceptance alone is
insufficient. Rejection must preserve or restore the previous working profile
and coherent policy state. Restart `completed` means replacement admission,
configuration/checkpoint restoration as applicable, and a usable committed
policy cycle, not merely process creation. `unchanged` is valid only for reload.

`reload-profile` and `logout` settle through the session's existing owners, which
also serve the key bindings. `reload-profile` raises the same reload request as
the binding. It is `unchanged` when the equivalent profile is retained,
`rejected` when the reload owner declines the candidate or a required WM
replacement rolls it back, and `completed` when the candidate is activated
directly or its replacement commits. One control reload is dispatched at a time;
another is `overloaded`. `logout` raises the same logout request as the binding
and is `completed` once the session has admitted the logout and begun draining
toward its own end. It has no rollback. The endpoint closes with the session, so
the reply may be the connection's last or may be lost to end of stream,
which a client reports as an unknown outcome.

The intentional WM replacement inside `restart-wm` or a reload is tracked by
the session-owned operation across that expected transition. Its original
catalog generation remains the reply identity. It is not rejected merely
because its own operation invalidated the catalog. Concurrent undispatched
old WM actions become stale; already dispatched ones require a proven result
or `indeterminate`. An unrelated WM replacement must not be mistaken for
the requested restart's successful completion.

Each invocation must resolve by the advertised command deadline measured from
receipt of its complete record, including queue time. If rollback or settlement
cannot be proven by then, return `indeterminate`; do not claim timeout means
no effect. No success may mean merely queued, parsed, or handed to an owner.
Deadline expiry and disconnect are not cancellation or undo. Recovery retains
the existing owner's bounded lifecycle and coherent-state obligations.

No at-most-once promise spans connections. Within a connection a well-formed
request is dispatched at most once. A replayed or decreasing request identity
is refused even if a previous reply was lost. If a connection drops after sending
an invocation, the client reports an unknown outcome; it does not reconnect
and replay a relative mutation. There is no replay cache or result retrieval
API. An indeterminate operation may still settle; a later invocation cannot
assume that operation was rolled back.

## Resource And Scheduling Contract

The service admits at most 32 connected peers, including peers still
authenticating, and 16 invocations in total (queued plus dispatched), also
constrained by the responsible owner's existing capacity. Each peer has one
pending administrative invocation; that is separate from 9P pipelining, which
allows up to eight wire requests per connection. Disconnect must release transport state immediately;
already dispatched work retains only its bounded owner correlation record
until terminal recovery. The [file contract](sophia-admin-files.md#runtime-limits-and-admission)
states the message, fid and timeout limits and the errors that report them.

Socket work is nonblocking and separately budgeted from input/render work.
Input, frame, security-transition, and recovery deadlines take priority.
The implementation must establish and measure an aggregate service-time
budget under command floods and slow readers before enablement. These
bounds alone do not prove latency or confinement. Saturated or malformed
control peers must not terminate the session or invalidate unrelated roles.

## Conformance

```sh
tools/check_control_protocol.sh
```

The check runs the real endpoint with the Rust and pinned Go clients, config
admission, sequencing, overload and command deadlines. Add
`--live-owner` for bubblewrap namespace denial and supervised policy owner
settlement, including withholding the commit after replacement to prove that
restart admission alone cannot report success. It launches no desktop.

Installed-session smoke, after enabling host-admin and starting a new session:
run `sophia msg commands`, invoke one safe advertised action with
`sophia msg policy 'NAME'`, then `sophia msg session restart-wm`. Check continued
input and rendering. This optional smoke is separate from the automated proof;
no installation or graphical session launch is performed by the check script.

The acceptance obligations and remaining limits are:

| Scenario | Required evidence from real service/owners |
| --- | --- |
| Admission | Session UID, socket-derived pidfd and matching user/mount/PID namespaces; Sophia protected roles denied; broader sandbox attestation excluded |
| Sequencing | Replayed, zero or decreasing request identities and a second request while one is pending are refused with no invalid dispatch |
| Authorization/catalog | Filtered discovery, dispatch-time recheck, stale mapping and forged scope denial; no role/private metadata leaks |
| WM settlement | Action correlation through projection commit, rejection and accepted no-op; no queue-only success |
| Replacement | Pending old requests never retarget; requested restart survives its own generation change and settles on the intended usable replacement |
| Reload | Equivalent, declined and activated candidates settle through the reload owner; a failed replacement restores the working profile |
| Disconnect/deadline | Before/after dispatch, partial replies and uncertain effects; no replay, bounded abandoned work and recovery |
| Resource pressure | Connection/queue overload, partial-frame trickle, slow readers, bounded buffers and fair input/frame progress under floods |

## Design Sources

[i3 IPC](https://i3wm.org/docs/ipc.html) demonstrates a small Unix framing
contract and documents reply loss around restart. Sophia keeps simple framing
but makes ambiguous outcomes explicit and exposes no general tree query.
[Sway IPC](https://github.com/swaywm/sway/blob/master/sway/sway-ipc.7.scd)
shows the value of reusing an established framing family.

[Niri IPC](https://github.com/niri-wm/niri/blob/main/docs/wiki/IPC.md)
separates machine-readable output from human formatting; its
[action handler](https://github.com/niri-wm/niri/blob/main/src/ipc/server.rs)
waits for action processing. Sophia similarly requires a meaningful completion
boundary, specifically owner settlement, and keeps JSON in the convenience CLI.

[QMP](https://www.qemu.org/docs/master/interop/qmp-spec.html) informs explicit
negotiation, request correlation, structured errors, and validation before
effects. Sophia makes correlation mandatory and allows one pending invocation
per connection.
[D-Bus](https://dbus.freedesktop.org/doc/dbus-specification.html) illustrates
separate authentication, typed interfaces, and correlated replies. Sophia
adopts that separation without adding a general message bus or treating
kernel credentials as a complete authorization decision.

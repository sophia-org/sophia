# Sophia Control v1

**Role:** normative scripting wire and lifecycle specification.
**Status:** experimental major 1, revision 2 (revision 1 remains negotiable); Linux session endpoint, Rust and
independent Python clients, policy settlement, and confirmed WM restart are
implemented. Control remains disabled by default. Revision 2 dispatches
`reload-profile` through the session's reload owner, which now restores the
prior profile when a replacement fails (t037), and adds `logout`.

[Scripting Sophia](scripting.md) owns the generic authority contract;
[the native protocol family](sophia-policy-ipc.md) owns the common envelope.
[`protocol/sophia-control-v1.kdl`](../protocol/sophia-control-v1.kdl) owns
field order, widths, kinds, and enum values. This document owns their meaning,
validation, and lifecycle. [Generated tables](generated/sophia-control-v1-wire.md)
and [wire vectors](../protocol/golden/sophia-control-v1.frames) are derivative
checks. Disagreement is a specification defect, never implementation choice.

## Scope And Authority

The session runtime owns a separate Unix `SOCK_STREAM` control endpoint.
Scripts are not role peers. No WM or shell listens for commands; callers
cannot send role messages, internal action ordinals, Engine transactions,
tokens, or application handles through this endpoint. Engine still validates
and commits visual state; the WM still interprets spatial policy.

Revision 1 exposes discovery and invocation of argument-free registered WM
actions and the session operation `restart-wm`; it reserves `reload-profile`
without advertising or dispatching it. Revision 2 (2026-09-26) adds the session
operations `reload-profile` and `logout`, with the settlement rules below.
Nothing else changes: a revision-1 connection behaves exactly as before. The owner selector
is `policy = 1` or `session = 2`. Shell commands, parameters, process execution,
synthetic input, metadata queries, event subscriptions, FD transfer, and
delegated grants are absent. No generic extension blob is reserved for them.
Future support requires a separately specified, negotiated revision.

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
uses the existing WM request/projection path. Features describe wire support,
not permission. Desktop control grants no clipboard, capture, title, or other
application-data access.

## Discovery And Transport

A client uses an explicit `--socket PATH` or `SOPHIA_CONTROL_SOCKET`; an explicit
path wins. The value must be an absolute filesystem path. No X property,
role endpoint, guessed socket, abstract socket, or TCP fallback is allowed.
The session creates a fresh random directory (0700) under the existing private
`XDG_RUNTIME_DIR`, binds `control.sock` (0600), and exports its absolute path to
host application launches only when control is enabled. Socket and peer
descriptors are close-on-exec. Session teardown removes its own endpoint. Possession of the path conveys no authority.
Clients should check server peer UID against their expected session user;
that check cannot defend against another already trusted host-user process.

The common 24-byte envelope is little endian, with no native padding:

| Offset | Type | Meaning |
| --- | --- | --- |
| 0 | u32 | Magic `0x48504f53` (bytes `SOPH`) |
| 4 | u16 | Frame version 1 |
| 6 | u16 | Message kind, 128–134 reserved for this interface |
| 8 | u64 | Connection-scoped request correlation ID |
| 16 | u32 | Payload byte count, at most 65,536 |
| 20 | u32 | Reserved, zero |

Here the family's `transaction` field is **not an Engine transaction or an
authorization token**. Its domain is this connection alone. Byte order, kind,
length, and reserved header must be checked before allocating a payload.
Reads/writes may fragment at any byte. EOF in a frame is truncation. Trailing
payload bytes, unknown kinds/enums, nonzero reserved fields, nonzero string
padding, excessive counts, invalid strings, and wrong-direction messages fail
closed. No descriptors may be sent or accepted; unexpected ancillary data is
a protocol violation and received FDs must be closed immediately.

## Negotiation And Sequencing

The admitted client sends `ClientHello` (128), ID zero, specifying inclusive
minimum/maximum revisions and required feature bits. Both revisions must be
nonzero and ordered. Revisions 1 and 2 define **no feature bits**. A server selects
the highest supported revision in the intersection; absent overlap returns
`ProtocolError(revision)`. Any unsupported required feature returns
`ProtocolError(features)`. The frame version and major are not negotiated;
this endpoint speaks major 1 only.

`ServerWelcome` (129), ID zero, states the selected revision and features zero. Its
128-bit session ID (two LE u64 words) is a fresh nonzero identifier for each
session-runtime incarnation; its nonzero connection ID is never reused within
that session. Neither is a secret or permission. Counters must not wrap.
The welcome advertises effective bounds:

| Field | Allowed value |
| --- | --- |
| `max_payload` | 65,536 (fixed in revision 1) |
| `max_commands` | 256 WM actions plus the revision's session operations: 258 in revision 1 (two, one reserved), 259 in revision 2 (three) |
| `max_name_bytes` | 128 |
| `command_timeout_ms` | 1–10,000 |
| `frame_timeout_ms` | 1–2,000 |
| `idle_timeout_ms` | 1–60,000 |

These three capacity fields are fixed so a complete valid catalog always fits.
Timeouts may be reduced by the server. Before welcome, the handshake deadline
is 2,000 ms from accept, including admission and receipt of the complete hello.

After welcome, each request has a nonzero ID strictly greater than its
predecessor. IDs may skip, but may not wrap or be reused. The response echoes
the ID. Exactly one request may be outstanding per connection; the client
waits for its complete terminal reply before sending another request. A server
must not queue an additional request received while it owes a reply; it closes
the peer for a sequence violation. Bytes already buffered beyond the current
request must not silently become a later request after settlement. Normal
stream fragmentation is not a second request.

`CommandsRequest` (130) receives `CommandsReply` (131). `Invoke` (132)
receives `CommandOutcome` (133). There are no acceptance replies or unsolicited
events. A `ProtocolError` (134) is terminal and followed by close. Its ID is
the offending request ID when a valid header identifies it, otherwise zero;
the client must tolerate this terminal exception to ordinary reply matching.
Malformed/sequence errors are codes 1/2; revision/features errors are 3/4.
An implementation may close immediately instead of sending an error when
admission, buffer limits, or the transport prevent a safe reply. Unauthenticated
callers receive no catalog or negotiation details.

## Catalog And Invocation

`CommandsRequest` has an empty payload. Its reply is one complete snapshot:
nonzero `catalog_generation`, entry count, reserved zero, then 136-byte entries.
Each entry contains owner, completion kind, name length, reserved zero, and
128 name bytes. At 258 entries the payload is 35,100 bytes. Empty catalogs are
valid; truncating a larger catalog without reporting an admission failure is
not. An owner catalog exceeding capacity must be refused before publication.
Discovery must produce its reply within `frame_timeout_ms` of receipt of the
complete request, or close the connection; it must not wait for WM work.

Names use the existing WM action grammar: 1–128 ASCII bytes drawn from letters,
digits, space, `-`, `_`, and `.`, with no leading or trailing space. Their
declared length excludes zero padding; all unused bytes must be zero. Names
are case-sensitive and never interpreted as shell text. Entries are unique by
`(owner, name)` and sorted by owner then ASCII name bytes. Policy entries have
completion `policy-commit = 1`; session entries have `session-settlement = 2`
and a session operation of the connection's revision. A revision-1 connection
is shown only `restart-wm`; a revision-2 connection is shown `logout`,
`reload-profile` and `restart-wm` when the session offers them. Both views of
one generation carry identical entries apart from those session operations,
and an invocation absent from the connection's view is `rejected`. Session-operation binding slots and launch
bindings must not be advertised as policy commands to bypass owner routing.

The generation identifies an immutable authorized catalog and its private
recipient/action mapping. Allocate nonzero generations monotonically within
the session; never reuse one for a different mapping or authority context.
WM replacement, profile/action mapping changes, and authorization changes
invalidate affected catalogs even when the visible names are unchanged.
Connections may share a generation only when their authorized mappings match.
Catalog IDs reveal no WM-private model or application identity. No generation
survives reconnect as a client cache: rediscover on every new connection.

`Invoke` names an owner and command from an exact previously returned catalog
generation. No discovery, a stale generation, or a changed recipient before
dispatch yields `stale`; no command runs. A valid current generation with a
name absent from its catalog yields `rejected`. Authorization loss yields
`denied` if a reply can still be delivered, otherwise the connection closes.
Unknown owner enum values are malformed, not extensible command families.
The runtime resolves the private action mapping once, pins it through
dispatch/settlement, and must never retarget the action to a replacement WM.

Only per-connection order is promised to clients. The session gives admitted
ordered mutations a serial dispatch order consistent with its owner queues;
two independent connections do not establish a caller-visible total order.
Relative actions are never coalesced or automatically replayed. A busy owner
may cause a bounded wait or `overloaded`, not unbounded parallel work.

## Settlement And Failure

`CommandOutcome` echoes the **requested** catalog generation, including for
failure. Its optional detail is 0–256 bytes of valid UTF-8, zero padded, with
no NUL or control characters. Diagnostics must exclude application metadata,
private handles, credentials, and raw owner output. Clients branch on enums,
never diagnostic prose. CLI JSON may render these fields with symbolic enum
names; JSON is not the server wire.

| Outcome | Required meaning |
| --- | --- |
| `committed` (1) | Policy action's correlated projection passed Engine validation and committed, including an accepted no-op projection |
| `completed` (2) | Session operation reached its owner settlement boundary |
| `unchanged` (3) | Reload validated an equivalent candidate and retained the usable active profile |
| `rejected` (4) | Semantic validation failed; the owner's coherent state was preserved/restored |
| `stale` (5) | Catalog or recipient changed before dispatch; no action dispatched |
| `denied` (6) | Authorization refused before dispatch; no action dispatched |
| `unavailable` (7) | Owner unavailable before dispatch; no action dispatched |
| `overloaded` (8) | Capacity refused before dispatch; no action dispatched |
| `timed-out` (9) | Deadline expired and the owner proves no mutation took effect or can take effect later |
| `indeterminate` (10) | Dispatch occurred but definitive settlement/effect cannot be established |

A policy `committed` is not physical display retirement. A session reload
`completed` means the candidate was validated by the relevant owners, activated,
and reached a usable policy cycle. Structural parser acceptance alone is
insufficient. Rejection must preserve or restore the previous working profile
and coherent policy state. Restart `completed` means replacement admission,
configuration/checkpoint restoration as applicable, and a usable committed
policy cycle, not merely process creation. `unchanged` is valid only for reload.

Revision 2 settles its operations through the session's existing owners, which
also serve the key bindings. `reload-profile` raises the same reload request as
the binding. It is `unchanged` when the equivalent profile is retained,
`rejected` when the reload owner declines the candidate or a required WM
replacement rolls it back, and `completed` when the candidate is activated
directly or its replacement commits. One control reload is dispatched at a time;
another is `overloaded`. `logout` raises the same logout request as the binding
and is `completed` once the session has admitted the logout and begun draining
toward its own end. It has no rollback. The endpoint closes with the session, so
the reply may be the connection's last frame or may be lost to end of stream,
which a client reports as an unknown outcome.

The intentional WM replacement inside `restart-wm` or a reload is tracked by
the session-owned operation across that expected transition. Its original
catalog generation remains the reply identity. It is not rejected merely
because its own operation invalidated the catalog. Concurrent undispatched
old WM actions become stale; already dispatched ones require a proven result
or `indeterminate`. An unrelated WM replacement must not be mistaken for
the requested restart's successful completion.

Each invocation must resolve by the advertised command deadline measured from
receipt of its complete frame, including queue time. If rollback or settlement
cannot be proven by then, return `indeterminate`; do not claim timeout means
no effect. No success may mean merely queued, parsed, or handed to an owner.
Deadline expiry and disconnect are not cancellation or undo. Recovery retains
the existing owner's bounded lifecycle and coherent-state obligations.

No at-most-once promise spans connections. Within a connection a well-formed
request is dispatched at most once. Duplicate/decreasing IDs are protocol
errors even if a previous reply was lost. If a connection drops after sending
an invocation, the client reports an unknown outcome; it does not reconnect
and replay a relative mutation. There is no replay cache or result retrieval
API. An indeterminate operation may still settle; a later invocation cannot
assume that operation was rolled back.

## Resource And Scheduling Contract

Revision 1 imposes these server ceilings; implementations may choose less:

- 32 connected peers total, including peers still authenticating.
- One incoming frame and one outgoing frame per connection, each at most
  65,560 bytes including header; no unbounded decoded copies or output list.
- 16 admitted invocations total (queued plus dispatched), also constrained by
  the responsible owner's existing capacity. Overflow replies `overloaded`.
- One outstanding request per peer. Pending result state is bounded by the
  global invocation limit; disconnect must release transport state immediately.
  Already dispatched work retains only its bounded owner correlation record
  until terminal recovery, not an indefinitely retained result for the client.
- An incomplete frame expires after `frame_timeout_ms` from its first byte,
  without extending the timer for trickle traffic. A queued reply must drain
  within that same deadline from reply creation. An idle ready connection
  expires after `idle_timeout_ms`. Command settlement uses its separate timer.

Socket work is nonblocking and separately budgeted from input/render work.
Use fair round-robin servicing: at most one complete request and 16 KiB of
read/write work per peer per service pass, carrying partial frames forward.
This is a ceiling, not permission to delay physical input by a full pass;
input, frame, security-transition, and recovery deadlines take priority.
The implementation must establish and measure an aggregate service-time
budget under command floods and slow readers before enablement. These wire
bounds alone do not prove latency or confinement. Saturated or malformed
control peers must not terminate the session or invalidate unrelated roles.

## Example Exchange And Conformance

On one authenticated stream (numbers are request IDs):

```text
C -> ClientHello(0, revisions=1..1, required_features=0)
S -> ServerWelcome(0, revision=1, session_id=S, connection_id=C, bounds=...)
C -> CommandsRequest(1)
S -> CommandsReply(1, generation=G, [(policy, policy-commit, "focus-next")])
C -> Invoke(2, generation=G, owner=policy, name="focus-next")
S -> CommandOutcome(2, generation=G, outcome=committed)
C -> Invoke(3, generation=G, owner=policy, name="focus-next")
S -> CommandOutcome(3, generation=G, outcome=stale)  // WM replaced before dispatch
C -> CommandsRequest(4)
S -> CommandsReply(4, generation=G2, ...)
```

The independent [Python client](../bindings/python/sophia_control_v1.py)
uses only the standard library and this wire contract, with no Sophia codec
or WM dependency. It interoperates with the implemented session endpoint and
defaults to JSON output; the installed `sophia msg` defaults to human output:

```sh
python3 bindings/python/sophia_control_v1.py --socket /absolute/test.sock commands
python3 bindings/python/sophia_control_v1.py --socket /absolute/test.sock policy focus-next
tools/check_control_protocol.sh
```

The check validates generated artifacts, both codecs, real endpoint clients,
config admission, sequencing, overload and command/frame deadlines. Add
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
| Negotiation/sequencing | Unsupported revision/features, pipelining, zero/duplicate/decreasing IDs close only the offender with no invalid dispatch |
| Authorization/catalog | Filtered discovery, dispatch-time recheck, stale mapping and forged scope denial; no role/private metadata leaks |
| WM settlement | Action correlation through projection commit, rejection and accepted no-op; no queue-only success |
| Replacement | Pending old requests never retarget; requested restart survives its own generation change and settles on the intended usable replacement |
| Reload | Deferred: reload remains unadvertised until invalid WM settings preserve/restore the working profile |
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
effects. Sophia makes correlation mandatory and avoids concurrent outstanding
requests in revision 1.
[D-Bus](https://dbus.freedesktop.org/doc/dbus-specification.html) illustrates
separate authentication, typed interfaces, and correlated replies. Sophia
adopts that separation without adding a general message bus or treating
kernel credentials as a complete authorization decision.

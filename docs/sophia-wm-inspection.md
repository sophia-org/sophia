# Read-only WM inspection

**Role:** normative disclosure and lifecycle contract for the optional
`sophia_wm_inspection_v1` host service. The observer serves both WM transports.
It does not attach another client to the [WM role](sophia-wm-files.md).

## Permission and ownership

The startup desktop profile must explicitly contain:

```kdl
session {
    inspection "host-admin"
}
```

Omission and `inspection "disabled"` disable the endpoint. This permission is
independent of `control "host-admin"`. Changing either permission requires a
new Session; a profile reload cannot widen the audience. No command-line flag
or environment variable enables inspection.

The audience is the trusted host domain, not individual applications. Admission
uses the same pinned process and namespace checks as host control: a
socket-derived pidfd, a pinned proc directory, all UID identities, user/mount/PID
namespace identities, liveness, and exclusion of the protected WM peer. A UID,
pathname, attach name, fid or qid alone grants nothing. An authorized host
process can copy what it has read; this is not a per-application delegation
scheme. Missing admission prerequisites leave inspection disabled while the
desktop continues.

Session creates a separate private runtime directory and socket (0700/0600).
Host application launches receive `SOPHIA_WM_INSPECT_SOCKET` only from that
Session's enabled service; inherited values are removed. Protected role launch
environments and grants do not receive it. The environment variable is
discovery, never admission authority.

The service owns its export, qids, sequences, ring, readers and snapshot pins.
It never uses the WM writer's attach, snapshot pin, candidate buffer, journal,
ACK floor or continuing Qid allocator. Session's existing owners publish copied
observations. There is no observer reducer, input path, command file or mutation
operation.

## Disclosure

The server constructs and validates an allowlisted record before it becomes
visible. The v1 snapshot contains Session and WM generations, selected WM wire
and capabilities, readiness, opaque output/surface identities, output geometry
and work areas, focus, and surface geometry. It describes the last complete
Session scene supplied to spatial policy, not an attestation of current scanout
or application pixels.

Events summarize owner transitions: connection/configuration changes,
configuration refusal, projection commit/refusal/timeout, reported presentation
changes and session-operation acceptance/refusal. A queued outcome is not proof
that the WM consumed it. Accepted session intent does not prove application
execution. Presentation summaries do not add a physical-completion claim.
These are coalesced notifications, not a transition ledger: repeated reports
within one owner turn collapse, and mixed kinds become `snapshot_changed`.
Session normally publishes at the start of the next turn, before early-return
paths; its idle authority wait is capped at 25 ms. Startup and authority fences
publish immediately. This bounds encoding to one snapshot per normal turn,
without adding a timer or observer-driven policy work.

The schema omits titles, classes, PIDs, XIDs, namespace identities, raw input,
pixels, paths, profile content/digests, action catalogs and activation serials,
operation tokens/slots, launch tokens/classifications and actionable presentation
correlation identities. Unknown fields are refused. A full WM snapshot's Debug
renderer is not a live disclosure interface.

## Files and records

| Path | Access | Meaning |
| --- | --- | --- |
| `/` | 0500 | Fixed `api`, `status`, `snapshot`, `events` enumeration |
| `api` | 0400 | ASCII family, schema, bounds and interpretation |
| `status` | 0400 | Availability, generations, selected wire/capabilities, event floor/tail and loss generation |
| `snapshot` | 0400 | Immutable object pinned at open, with its correlated sequence and next event byte offset |
| `events` | 0400 | Retained NDJSON owner reports, addressed by observer byte offset |

JSON has schema 1, stable field/enum names and escaped strings. Every u64 is a
canonical decimal string, including capability masks, so clients do not lose
identity precision. Ordinary smaller integers remain JSON numbers. The public
`sophia_protocol::inspection` records and strict codecs define the full layout.

Snapshot publication, its observer sequence and its event cursor change
together. Opened object metadata describes that immutable object. Directory
metadata is advisory and does not pin a snapshot. `events` reads are repeatable
within the retained byte range; a read at the tail waits, one beyond it refuses,
and one below the floor returns `ESTALE`. A snapshot's cursor begins immediately
after its correlated publication event.

Opening `events` requires a coherent snapshot opened on the same attachment.
A watch that encounters a retention gap remains stale; jumping its offset to
the current tail cannot resume it. Open a fresh snapshot and a new watch to
resynchronize. Clunking the snapshot releases its pin without invalidating that
resynchronization cursor.

The service permits four admitted connections, sixteen fids and eight pending
requests per connection, 64 KiB messages and 128 KiB queued output per
connection. Each reader can pin one snapshot of at most 1 MiB. The shared ring
holds at most 64 events and 1 MiB. Readers never pin event retention. Slow readers
lose history rather than delaying the WM.

Publication uses a bounded nonblocking operation. Contention or invalid/oversized
publication records explicit observation loss; it cannot fail WM settlement or
wait for a reader. The next successful publication supplies a fresh coherent
snapshot. An old watch fails on the changed loss generation, even if the data
itself is unchanged. There is no silent cursor advance, auto-reconnect or
observer-generated policy Dirty/cycle.
An invalid record is latched until its owner facts change; repeated notifications
cannot make the same rejected scene re-encode on every turn.

Authorization is rechecked on every fid operation and pending-read retry, with
bounded maintenance for quiet peers. WM disconnection/replacement fences old
attachments, handles and pins before new state is published. Session stop and
failed peer revalidation revoke them too. Revocation discards pending work and
unsent output at the worker boundary; it cannot recall bytes already delivered
or make an already concurrent authorized read retroactively unauthorized.

## Client

```text
sophia inspect wm [--socket ABSOLUTE_PATH] [--json] ls
sophia inspect wm [--socket ABSOLUTE_PATH] [--json] stat PATH
sophia inspect wm [--socket ABSOLUTE_PATH] [--json] status
sophia inspect wm [--socket ABSOLUTE_PATH] [--json] snapshot
sophia inspect wm [--socket ABSOLUTE_PATH] [--json] watch
```

An explicit socket wins over the environment. The client does not guess
endpoints, sniff another protocol or enable the service. It validates the API
identity and complete typed objects before rendering. `watch` emits an initial
snapshot, then contiguous events from that snapshot's cursor. Gaps, loss or
replacement end it with an error; the operator explicitly opens a new view.
Human output is the default; `--json` preserves the versioned machine records.

The client uses read-only 9P operations, bounded connection/RPC deadlines and
flushable idle event reads. It exposes no write/open-for-write API. This is
direct socket inspection, not mounted filesystem acceptance.

The offline `wm_file_inspect` remains separate. It reads caller-supplied
captures under a supplied validation context and may render the fuller captured
protocol. It does not authenticate those files or define this live audience.

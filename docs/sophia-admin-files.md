# Administrative file records

Status: t254 invocation, discovery and status codecs. No administrative 9P listener, client migration
or default change is implemented by this document. The existing control service
remains active. The shared-core export must preserve its HostDomain admission,
command catalog and execution owners before these records can be used.

## Version 1 line grammar

This version names the file syntax, independently of legacy control-wire
revision 2. It does not downgrade the current command catalog or add commands.

The future `ctl` file accepts one invocation record. Its corresponding `status`
record describes readiness, a pending request or an outcome; accepting bytes is
not a successful command result.
These are separate files with one versioned grammar each, not the old socket
envelope inside a 9P write.

```text
v1 invoke CONNECTION REQUEST GENERATION OWNER LENGTH:NAME\n
v1 ready CONNECTION\n
v1 pending CONNECTION REQUEST GENERATION\n
v1 outcome CONNECTION REQUEST GENERATION OUTCOME LENGTH:DETAIL\n
```

Here `\n` means one LF byte. Fields before the counted text have exactly one
ASCII space between them. CONNECTION, REQUEST and GENERATION are nonzero u64
values written in canonical unsigned decimal: no sign or leading zeroes.
LENGTH uses the same decimal spelling but allows zero and counts UTF-8 bytes,
not characters. It must equal the complete remaining text before LF. There is
no escape processing, shell expansion or trailing padding.

Every record is at most 512 bytes including LF. NAME is 1–128 bytes, using the
existing control-name rules: ASCII letters, digits, spaces, hyphens, underscores
and dots, with no leading or trailing whitespace. OWNER is `policy` or
`session`. Syntactically valid names still require catalog membership and an
admitted grant; parsing a new name never creates an operation. Existing session
operations and registered policy actions retain their existing owners.

DETAIL is at most 256 UTF-8 bytes and may be empty. Control characters are
forbidden, including CR, embedded LF, tab and NUL. OUTCOME is exactly one of
`committed`, `completed`, `unchanged`, `rejected`, `stale`, `denied`,
`unavailable`, `overloaded`, `timed-out` or `indeterminate`. Preserve the existing
distinction between policy commit, Session completion and uncertain effects.
The codec does not decide which outcomes an operation may legally produce.
Ready means this connection has not accepted a request. Pending identifies an
accepted request without claiming dispatch or success. An outcome preserves the
request's identity and catalog generation, including when its effects are uncertain.

Examples, each followed by LF:

```text
v1 invoke 7 9 12 policy 8:next tab
v1 invoke 7 10 12 session 10:restart-wm
v1 outcome 7 9 12 committed 0:
v1 outcome 7 10 12 indeterminate 19:completion not seen
```

## Discovery records

`info` describes the admitted connection and its service session:

```text
v1 info SESSION_LOW SESSION_HIGH CONNECTION COMMAND_MS FRAME_MS IDLE_MS\n
```

The session identity consists of two u64 words in canonical unsigned decimal.
Either word may be zero; the pair must not be all zeroes. CONNECTION is nonzero.
The three timeout fields are nonzero u32 milliseconds. They describe the command
deadline, incomplete transport/reply deadline and idle deadline. The initial
service values remain 10000, 2000 and 60000 ms. These fields do not extend a
command's deadline when its status is read. The complete record is at most 512
bytes. A new service session is a new identity; clients must not replay an
uncertain invocation after reconnecting.

`commands` is one immutable catalog snapshot, with a header and exactly COUNT
rows. Each row ends in LF:

```text
v1 catalog GENERATION COUNT\n
v1 command OWNER LENGTH:NAME\n
```

GENERATION is a nonzero canonical u64. COUNT is canonical decimal, from 0 to
259. NAME follows the invocation rules. Rows are strictly ordered by owner
(`policy` before `session`), then by ASCII name; duplicates are forbidden.
There may be at most 256 policy actions. Session names are limited to `logout`,
`reload-profile` and `restart-wm`. This preserves the current catalog; it adds
no executable selection or output invocation. An empty catalog is valid.

The complete snapshot is at most 39,432 bytes: a 64-byte header allowance plus
259 rows of at most 152 bytes. The decoder checks the total bound and count
before allocating rows. Missing rows, surplus rows, malformed names, unknown
owners and unsupported Session names are refused. The encoder validates the
same catalog before formatting. A catalog supplies names, not permission to
invoke them.

## Proposed file lifecycle

These rules guide the next export slice. They are not implemented by the codecs.
The [bounded lifecycle model](../validation/specula/admin-file-lifecycle/README.md)
checks cancellation, replay, status retention and unsettled execution custody.
Runtime integration, admission and restart evidence remain required.

| File | Open mode | Read or write behavior |
| --- | --- | --- |
| `info` | Read-only | Connection identity and limits, pinned at open. |
| `commands` | Read-only | Current complete catalog, pinned at open. |
| `status` | Read-only | Current ready, pending or outcome record, pinned at open. Reopen to observe a change. |
| `ctl` | Write-only | One complete invocation per offset-zero write, at most 512 bytes. |

Reads accept byte offsets and short counts within their pinned snapshot, with
EOF at its end. No read consumes an outcome. Writes to read-only files, reads
from `ctl`, nonzero write offsets, partial lines and multiple lines fail before
dispatch. There is no cross-write staging. The negotiated message size must
accommodate a complete 512-byte control write plus its 9P framing.

Keep one pending request and one current outcome per admitted connection.
Reserve the ticket and outcome capacity before accepting an invocation. A
strictly newer accepted request acknowledges the previous outcome; no separate
acknowledgement command is needed. Malformed, replayed or capacity-refused
writes cannot erase that result or advance the request watermark. Once accepted,
stale-generation and unknown-command refusals produce attributed outcomes and
consume their request identity. Counter exhaustion refuses further requests.
An indeterminate result does not release an unsettled execution ticket.

Opening `commands` establishes that connection's discovered generation. The
client must read and validate the full snapshot before invoking; the server
still checks the current generation and command at dispatch. Limit retained
catalogs to one per connection: concurrent opens share it until its last fid
closes. Bound all remaining fids and pinned status records through the shared
core's declared limits. New opens and operations on retained fids must recheck
admission. Old snapshots cannot authorize a stale command.

Flushing a status read or closing its fid only drops that read or snapshot.
Clunking `ctl` does not cancel an accepted command. Connection loss cancels a
queued ticket through the existing owner; it cannot undo a dispatched effect.
A cancelled write reply is not evidence that the command did not execute.
The client must report uncertainty whenever it cannot establish a terminal
outcome, and must not reconnect and replay. Service restart invalidates old
connections and does not reconstruct their results.

## Export obligations still to implement

The connection value must match the admitted attach, not select another caller.
The owner must verify catalog generation and action identity immediately before
dispatch, preserve monotonically scoped request identities and refuse replay.
Current code keeps at most sixteen pending administrative tickets; migration
must retain bounded custody rather than introducing an unbounded line queue.

Model and implement the proposed lifecycle with the shared core and existing
ticket owner, including its capacity and cancellation boundaries. A codec
consumes exactly one complete record; it is not a stream accumulator.
No 9P flush, clunk or disconnected reader may undo a committed effect
or authorize a retry after an indeterminate result. Admission must be rechecked
at the existing claim boundary. Read-only inspection grants no command access.

Use the existing `ControlCommand`, `ControlCatalog` and `ControlOutcome` values
in `sophia-protocol::control_records`. The legacy codec re-exports those same
types for compatibility; no duplicate command executor belongs in the file adapter.
The deferred output-process invocation extension is excluded.

## Validation

`sophia-protocol/tests/admin_files.rs` checks independent literal records,
non-ASCII byte counts, command spaces, exact bounds, every outcome, zero and
overflow identities, noncanonical numbers, unknown versions/owners/outcomes,
missing or duplicate line endings and embedded control characters. These are
codec controls, not proof of admission, replay prevention or live execution.
`admin_discovery.rs` adds literal discovery/status records, full-size catalogs,
strict sorting and command membership, malformed counts, identities and deadlines.
The existing legacy golden-frame controls remain unchanged.

The [t254 investigation](notes/investigations/t75i0gx8-define-bounded-administrative-file-records-before-migrating-control.md)
records implementation progress and remaining gates.

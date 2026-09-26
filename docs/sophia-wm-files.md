# WM files over 9P2000.L

Status: implementation contract under review for t249 and Hagia h006. The
existing WM IPC remains the default. This document specifies the WM role only;
output control still uses its separately admitted existing IPC connection.
The first checkpoint is a direct Unix socket, not a kernel mount.

## Ownership and negotiation

Session creates the endpoint for one supervised, protected WM launch. The
existing admission owner binds the accepted peer and connection epoch before
the export becomes accessible. There is one WM writer. The endpoint cannot be
used alongside a current-IPC WM writer or acquire authority from `uname`,
`aname`, a numeric UID, a fid or a qid. An unauthorized second attach refuses;
cloning the admitted root does not create another role or epoch.

9P negotiates the base version `9P2000.L` and its message-size limit. WM API
negotiation is separate: the file family is `sophia_wm_fs_v1`, API version 1.
The server publishes its admitted epoch, API range, capability ceiling and
object limits. A client submits its required and optional capabilities before
profile handoff. Selection is required union supported optional, within the
Session ceiling. An unknown required bit refuses. Native presentation
capabilities remain absent without native retirement; files supply no software
completion owner. Negotiation does not admit configuration or an application.

Revocation invalidates every operation through retained fids immediately.
Clunk and disconnect still release their local resources. Reconnect creates a
fresh admitted epoch and new qids; no retained fid, event offset, submission ID
or presentation authority crosses that boundary.

## Files

The root has this fixed vocabulary; discovery needs no directory enumeration
in the first direct-client checkpoint. Unsupported filesystem mutations refuse.

| Path | Access | Meaning |
| --- | --- | --- |
| `api` | read | Small immutable ASCII family/version, with `output_transport=current_ipc` |
| `limits` | read | Immutable binary epoch, capabilities and bounds |
| `snapshot` | read | Latest complete binary scene; open pins that exact immutable object |
| `events` | read | Ordered binary records, read by byte offset and retained until explicit acknowledgement |
| `transaction` | read/write | One bounded candidate buffer owned by its open fid |
| `submit` | write | Explicit submission of that buffer's epoch, submission ID and exact length |
| `ack` | write | Acknowledgement of a complete event sequence number |

Opening `snapshot` with no complete snapshot available returns `EAGAIN`.
At most one snapshot fid and one candidate buffer may be pinned per attach.
Repeated opens of other files share the same attach-owned bounds. `getattr`
reports the pinned snapshot length and qid, not a later scene's length. Reads
beyond its end return EOF. Closing a snapshot releases only that pin.

An event announces the snapshot's epoch, scene generation and object identity.
The adapter retains that scene until the matching cycle settles, so a reader
cannot accidentally open a later scene while handling the earlier request.
If an old snapshot fid remains open, it continues to expose its old bytes and
identity. A new snapshot open returns `EBUSY` until that fid is clunked; it
never aliases the old pin to the new scene. At most the current scene and one
older pinned scene coexist, each bounded to 1 MiB. The client checks the opened
object identity against the event before consuming it.
Opening/reading files never creates windows, scenes or transactions.

## Candidate assembly and submission

There is one unsubmitted buffer per attach, limited to 1 MiB including its
header. Its first nonempty write starts a 12-second assembly deadline; later
progress and retries do not extend it. This does not extend any shorter
existing profile/driver deadline. Empty writes return zero and allocate nothing.

Writes must append at the current end or exactly repeat bytes entirely within
the assembled prefix. Gaps, conflicting overlap, overlap-and-append, overflow,
or bytes beyond the declared record length refuse without changing the prefix.
The complete record is not delivered to Session on `Rwrite` or clunk. Clunk,
expiry or disconnect discards unsubmitted data. Flushing an append before it
executes cancels only that append and preserves the earlier acknowledged prefix.
Expiry releases the buffer and makes the old fid stale.

`submit` takes one complete fixed-size record at offset zero. It names the
admitted epoch, a nonzero attach-local submission ID and the exact candidate
length. Submission IDs increase within an attach and are distinct from domain
transaction IDs, request IDs, scene generations, 9P tags and fids. Domain IDs
retain their existing correlation and reuse rules.

Before submission, the adapter validates the complete binary shape, bounds,
capabilities and epoch, and obtains admission from the single existing driver
phase owner. The export has no mirrored phase machine. Only complete submit
begins proposal delivery; partial file writes do not mark `ProjectionPending`.
Dirty notifications before submission therefore remain governed by the driver's
current semantic phase. It reserves the existing bounded
semantic queue slot and its acknowledgement capacity before transferring
custody. Queue refusal leaves the exact candidate retryable. The adapter does
not perform a second policy validation or commit: configuration, projection,
session-operation and presentation decisions still belong to their existing
Session/Engine owners.

After custody transfers, an ordered `Submitted` event records the submission
ID. This means only that the existing driver accepted a complete value. It is
not a configuration, scene or presentation outcome. The existing correlated
semantic outcome follows from its owner. `Rwrite` on `submit` remains a byte
count. A submitted candidate is immutable; clunk or flush cannot undo it.

The last accepted candidate and its `Submitted` record stay available until
that record is acknowledged. An identical submit before acknowledgement
returns success without re-enqueueing. A conflicting repeat refuses. A later
ID cannot replace unacknowledged candidate custody. After acknowledgement the
bytes may be released, but the last submission-ID watermark remains; replay
at or below it returns `EALREADY`. There is no exactly-once claim across
disconnect. Recovery uses the fresh epoch, current committed snapshot and
existing checkpoint/profile rollback rules.

## Events, reads and cancellation

Events have strictly increasing nonzero sequence numbers within the admitted
epoch and monotonically increasing byte offsets. A record is appended whole;
9P reads may split it arbitrarily. Re-reading retained bytes returns identical
bytes. Reads at the current end block; reads beyond it return `EINVAL`; reads
below the acknowledged retention floor return `ESTALE`.

`ack` is one fixed-size epoch/sequence record at offset zero. It acknowledges
all records through that sequence. Exact repeats succeed. An older sequence,
a future sequence or a wrong epoch refuses. It releases transport retention
only: it cannot create a receipt, commit a proposal or release source/native
custody. At most 64 records and 1 MiB of event bytes are retained. The existing driver may dequeue one command before calling the adapter. That
single command remains in-flight custody throughout borrowed `send`; the
adapter reserves the whole journal record before appending it or releasing
that custody. It adds no command queue or peek owner.
An adapter send has a four-second monotonic deadline from its first attempt,
matching the current IPC write bound. Waiting for acknowledgement capacity does
not extend that deadline. On expiry it returns a bounded send failure through
the existing worker failure/disconnect path. Stop wakes the wait immediately.
No core/export lock is held while waiting. The driver therefore never joins an
unbounded producer on Drop. There is no additional retry owner.

A pending read consumes nothing. `Tflush` cancels that request and its wait,
without advancing the event offset or acknowledgement floor. Once an ordinary
write or submit has executed, flush does not undo its effect. The 9P core
reserves reply space before calling exports; an output-full condition cannot
consume an event and silently discard its reply. A tag identifies request
custody, never a semantic transaction.

Input revocation, actual presented-frame observation and swallowed-release
debt remain local Session transitions. They do not wait for an event read,
acknowledgement or free 9P queue slot. A slow or disconnected WM loses live
authority under the existing rules even when its final receipt cannot be read.

## Binary envelope and payload ownership

All integers are little endian. Binary runtime files contain complete records,
not `sophia_wm_v1` frame headers or Begin/Chunk/End messages. The common record
header is 32 bytes:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 4 | total bytes including header, at most 1 MiB |
| 4 | 2 | WM file API version, exactly 1 |
| 6 | 2 | record kind |
| 8 | 8 | admitted connection epoch |
| 16 | 8 | attach-local submission ID for a candidate; zero otherwise |
| 24 | 8 | event sequence for events; zero for candidates and snapshots |

The envelope decoder requires an explicit object, event or candidate class; a
record of another class refuses. The file owner must additionally match the
exact permitted kind (for example Limits versus Snapshot). The typed kind
specifies body shape and allowed phase. Counts,
lengths, enum values, reserved zeros and UTF-8 are validated before exposing a
semantic value. Integer conversions are checked. Unknown kinds and unnegotiated
sections refuse; no partial semantic value escapes an assembly failure.

Payloads represent negotiation, exact profile prepare/activate/rollback and
completions, configuration/catalog and outcome, a complete scene and request,
a complete projection and outcome, dirty notification, session operation and
outcome, and presentation receipts. They preserve every existing semantic
identity and capability, including launch classifications/origins, tab and
translation groups, output actions, generic presentation and exact presented
action identities. Large arrays are sections of one complete object; 9P
fragments file bytes and never defines array or commit boundaries.

The bounded envelope and complete-array bodies are specified in
[`sophia-wm-files-v1.kdl`](../protocol/sophia-wm-files-v1.kdl). It includes the
kind table, 32-byte header, 16-byte section header, 24-byte submit and 16-byte
ack. Sections have unique ascending nonzero kinds, nonzero row count and byte
length, and zero reserved fields. Context-specific row sizes and aggregate
bounds remain in the shared neutral record codec, which checks them before
row allocation. The envelope exposes borrowed raw bodies and sections. The
typed array entry points use the shared neutral codecs for Snapshot,
Projection and Configuration; they do not construct old IPC transfer frames.

Their body prefixes are respectively 32, 40 and 48 bytes after the common
header, followed by complete sections. The schema pins every field offset.
Snapshot includes the domain transaction, scene generation and active output.
Projection includes the domain transaction, request, base generation and
active output. Configuration includes the domain transaction, policy generation
and chrome styles. Chrome colours use `0x00RRGGBB`; the legacy scalar frame's
`0xff` alpha byte is not part of this file representation. Snapshot and
Projection require an output section, and a snapshot's active output must
occur in it. Session still validates complete output coverage and scene truth.

The file path refuses unnegotiated sections even where the legacy path did not
enforce those capability bits. Snapshot encoding omits unselected extensions;
WM candidates must omit them themselves, or submission refuses. Hagia must
honour the selected set, not merely the capabilities it offered. Capability
requirements within individual rows and final authority checks remain with
Session. A transport Submitted event names the submission ID; policy settlement
names the original domain transaction/request identities. Neither substitutes
for the other.

Staging belongs to the per-attach file owner. It enforces the 1 MiB total and
requires submit length to equal the actual complete staged length. If the
generic listener's sixteen-connection limit were used with one staging attach
per connection, this would permit at most sixteen MiB of candidate staging;
the WM endpoint instead admits only its one supervised writer. Snapshot/event
retention and server output queues have their separate stated bounds.

The remaining codec checkpoint fixes scalar body layouts and publishes
cross-language valid/malformed binary corpora. Record-array layouts already
have a neutral codec owner shared by both transports. The new adapter must not build old IPC
frames or feed files through the old transport. Hagia implements the published
layouts independently in Nim; Sophia source is not a Hagia dependency.

Text inspection, when added, is a derived view with no mutation authority. It
does not replace the binary runtime path or expose client metadata to the WM.

## Required evidence

The transport checkpoint proves the base protocol independently of these role
semantics. The role checkpoint additionally proves fragmented reads/writes,
discarded staging, exact duplicate submit, stale/relabelled epochs, cancellation
on both sides of submission, missing and duplicate writers, bounded journal
pressure, shutdown, and no extra commit or receipt from an acknowledgement.

The paired executable checkpoint joins real Hagia with existing Session
prepare/commit and backend receipt owners. It covers the current capability
matrix, restart/profile rollback and all-output/topology behavior. Output IPC
must remain labelled in evidence. Simulated completion, direct sockets and
physical acceptance are distinct. Compare identical old/new workloads before
claiming a performance gain or proposing retirement of current IPC.

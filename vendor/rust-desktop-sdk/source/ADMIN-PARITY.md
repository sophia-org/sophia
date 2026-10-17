# Administrative SDK parity

Revision 15, 2026-10-16. Both SDKs now have developing administrative sessions
and caller-connected socket facades. Qualification remains t333 work.

The pinned file contract is copied unchanged from Sophia
`baedfd0c9`, `docs/sophia-admin-files.md`; SHA-256 is
`3cc2db4cce9d2154bf7765f074a7f78bd041c20bdc26903167d489f539fcba52`.
Its historical qualification paragraphs are not evidence for these SDKs.
The current runtime reference is `control/admin_client.rs`; passive codecs
are `sophia-protocol/src/admin_files{,/discovery}.rs` and
`control_records.rs`. The administrative owner and executor stay in Sophia.

| Required operation or behavior | Implementation/coverage needed in both SDKs |
| --- | --- |
| Connect and discover | HostDomain admission, standard 9P, immutable info snapshot and EOF, service/connection identity, validated advertised deadlines. No launch-role impersonation or protocol fallback. |
| Catalog | Complete bounded snapshot through EOF; 259 rows maximum, strict ordering, names with spaces and only the three defined Session operations. Retain exact generation. |
| Invocation encoding | Canonical nonzero identities and counted UTF-8 text, at most 512 bytes. One complete offset-zero ctl write; no cross-write staging or partial-write replay. |
| Admission | One request at a time, monotonic request IDs, validated catalog membership. A complete write refusal establishes non-acceptance; short or lost replies leave uncertainty. |
| Status | Reopen immutable ready/pending/outcome snapshots. Check connection, request and catalog generation and the owner's legal success outcomes. Reads never acknowledge an outcome. |
| Settlement | Preserve all ten outcomes, including timed-out and indeterminate. A newer accepted invocation acknowledges the prior outcome. No durable exactly-once claim. |
| External loop | Descriptor/readiness, bounded byte/syscall work, caller-monotonic deadlines and next wake. Poll status only while a command is pending; never poll while idle. |
| Cancellation and loss | Closing or flushing does not undo dispatched effects. Preserve uncertainty and forbid automatic reconnect/replay. Recovery requires a new explicit client and command. |
| Limits and lifetime | Bounded catalogs/status buffers, one immutable invocation, owned/borrowed lifetimes, request exhaustion and absolute deadlines that progress cannot extend. |
| Independent evidence | Literal malformed/boundary codecs, scripted partial/reordered traffic, same production owner/ticket scenarios, revocation, process loss and a foreign-language C binding smoke. |

Use the runtime's one-millisecond pending-status interval as a compatibility
reference, exposed through caller-driven wakeups. Do not copy its blocking
sleep loop into either SDK's external-loop API. Catalog ownership should
transfer or remain borrowed rather than cloning on every query. These are
design requirements; no client implementation or speedup is claimed here.

Both SDKs now decode the complete info record without heap allocation. Rust
returns a Copy value; C fills caller storage only after validating the entire
record. Independent tests cover literal values, maximum identities/timeouts,
zero and overflow refusals, every truncated prefix, noncanonical numbers,
extra fields, control bytes and malformed UTF-8. Parsing does not establish
EOF, admission or a deadline policy; those remain client responsibilities.
Both also decode invocation and ready/pending/outcome records without allocation.
Command names and outcome details borrow the input; keep that storage alive
and unchanged while using the result. All ten outcomes retain their distinct
values. UTF-8 byte counts and Unicode control-character rejection are checked.
Invocation encoding uses fixed 512-byte scratch storage, then copies the
complete record into caller storage. Invalid values or insufficient capacity
leave output unchanged. C also permits the source name to overlap destination
storage. Neither language turns a syntactically valid name into catalog authority.

Independent tests cover literal wire records, maximum IDs and name/detail
lengths, all outcomes, borrowed pointers, short output buffers, every truncated
prefix, malformed spelling and invalid text. Six Rust tests and the three C
administrative test binaries pass, with strict Rust Clippy.

Complete catalog decoding and binary-search membership lookup are now available
in both SDKs. The catalog contains at most 259 inline rows whose names borrow
the immutable input; neither parsing nor lookup allocates. Validation rejects
zero/noncanonical generations, excessive counts, missing or surplus rows,
duplicates, bad owner/name ordering, more than 256 policy actions and unsupported
Session names. C leaves the output catalog unchanged on failure; Rust returns
no catalog. Neither exposes a partially validated catalog.

Nine Rust administrative tests and all four C administrative test binaries pass.
Catalog tests cover all 259 commands, every truncated prefix of a literal
two-row snapshot, the 256-policy boundary, 128-byte names, malformed snapshots,
borrowed names and lookup. The external-loop clients and paired production
gates remain; no performance measurement or client parity is claimed.

The provisional snapshot components now produce Walk/Open/Read/Clunk operations
for info, commands and status on an existing connection. Reads respect msize,
iounit and file bounds, including a one-byte EOF probe at the bound. Results
remain unavailable until EOF, syntax validation and clunk succeed. Deadlines
are absolute caller-monotonic times; clock regression changes no state. Remote
refusal, invalid replies and expiry require the connection driver to close,
without automatic retry or reconnect.

Rust accepts reusable Vec storage, reserves the file bound once and returns
that storage without copying. C uses caller storage of at least the file bound.
Raw scratch bytes are not validated results. Three Rust component tests and
C's independent snapshot test cover fragmented reads of all three files,
storage identity, EOF/clunk barriers, malformed EOF, overflow, oversized replies,
clunk refusal, EAGAIN refusal, deadline and clock regression. Strict Clippy
passes. These tests supply matched replies; connection/tag ownership, readiness,
bounded dispatch, startup, invocation custody and production gates remain.

Both SDKs now have provisional startup drivers on an exclusively owned,
already-versioned pipeline with no fids. They attach once, fetch info through
EOF/clunk, enforce advertised timeout ceilings (10000/2000/60000 ms), and open
ctl for writing with an iounit sufficient for a complete 512-byte command.
They hand off the same pipeline, root and ctl; no reconnect or fallback occurs.
The caller selects the admitted HostDomain endpoint. Raw-socket connection and
version negotiation facades remain separate work.

Independent socket tests cover fragmented info, one-byte I/O budgets, exact
file order, retained fids, no idle dispatch I/O, directory refusal, excessive
timeouts, insufficient ctl iounit, clunk refusal and expiry before sending.
Clock regression leaves state unchanged. Five Rust client tests and C's startup
test pass; strict Clippy passes. Rust shares one byte/syscall budget across at
most 32 rounds. C calls its bounded pipeline service once per dispatch, then
queues the next request and exposes write readiness. Neither sleeps.

Rust owns and drops the failed pipeline; C marks failure and requires the
caller to close its borrowed descriptor. C reports transport failures separately
from snapshot/protocol failures and retains the raw transport result. Startup
alone is not a usable command client: catalog retention/refresh, invocation
custody, status scheduling, final facade and production gates remain.

The provisional command tracker now validates catalog membership and owns one
immutable encoded invocation. It distinguishes queued, issued, accepted,
settled, explicitly refused, dropped-unsent and uncertain custody. A complete
write error proves refusal; short replies, loss or expiry after issue preserve
uncertainty. Close retains confirmed outcomes and refusals. No automatic replay
is permitted. The session still owns monotonic request IDs and single-command
admission.

Status snapshots must match connection/request/catalog generation and the
owner's legal success outcome. Pending schedules the next snapshot one
millisecond later through caller wakeups. Each snapshot has a frame deadline;
partial progress and repeated pending results cannot extend the original
command deadline. All ten outcomes are retained, with catalog invalidation
matching Session operations and stale/denied/indeterminate results. Up to 256
detail bytes are copied once into retained result storage so later snapshot
reuse cannot overwrite them. The tracker allocates no heap storage.

Eight Rust client tests and C's independent command test pass, including all
owner/outcome combinations, identity refusal, immutable bytes, custody on close,
explicit/short write replies, frame/overall expiry, clock regression and pending
cadence. Strict Clippy passes. These component tests do not establish the full
command session: pipeline integration, retained catalog ownership, request
sequencing, external-loop facade and paired production gates remain.

The integrated sessions now retain one catalog, admit one command at a time,
assign increasing request IDs and drive matched snapshot/write replies on the
same pipeline. Rust retains one snapshot Vec plus fixed name offsets; C retains
names in its caller-owned catalog buffer. Neither clones a catalog per command.
Rust reuses status storage; C uses fixed storage. C session state must not move
after initialization because its borrowed views point inside it.

Both expose catalog refresh, command admission, latest custody, outcome,
write readiness, next wake and explicit close. Catalog refresh is explicit after
an invalidating result. Idle sessions schedule no status reads. Request-ID
exhaustion refuses admission. Terminal errors preserve custody and prevent
further mutation; no reconnect or replay occurs. Rust drops its owned pipeline;
C requires its caller to close the borrowed fd. C errors identify the failing
component and preserve that component's error value.

Ten Rust client tests plus nine protocol tests pass, with strict Clippy. C's
independent integrated socket test passes. Each peer exercises three sequential
commands on one attach, a pending snapshot before each outcome, reuse of the
first catalog, refresh after a Session result, unadvertised/busy refusal,
explicit write refusal, loss after write and mismatched outcome generation.
Both retain confirmed outcomes on close. These are scripted peers, not production
export evidence. Raw-socket/version facades, further integrated failure cases,
ownership/recovery guidance and paired production qualification remain.

Both now provide nonblocking caller-connected socket startup through version,
file opening and catalog completion. One absolute deadline spans all phases;
one phase spends I/O budget per dispatch. A ready session transfers once.
The complete scripted exchange now uses these facades. Paired stalled-version
tests cover zero/one-byte budgets, clock regression and expiry with no extra
bytes; unsupported versions terminate without attach or fallback. Twelve Rust
client tests and C's session/connecting tests pass, with strict Rust Clippy.
[Ownership and recovery](ADMIN-CLIENT.md) documents both APIs and their limits.
Further integrated failure cases and paired production qualification remain.

Both independently built SDK peers now pass Sophia's production administrative
export gate `admin_external_sdk`. Each completes a policy command through a
claimed ControlTicket, receives Stale after the parent changes catalog generation,
refreshes and completes a new command on the same connection. Only requests 1
and 3 reach the owner; the stale request is settled by the export.

A second run revokes the child's admission after its command ticket is claimed.
Each SDK terminates with uncertain custody and refuses another invocation.
The claimed ticket remains dispatched; loss of admission does not undo its
effects. The fixture supplies policy completion and does not execute a real WM
action. This proves the file-service/SDK boundary, not service-process loss,
installed-session behavior or release-wide qualification. Strict Rust Clippy
and the independent C peer build pass. Service-loss/recovery and remaining
integrated failure coverage still need qualification.

Both peers also pass production service shutdown before and after ticket claim.
Dropping the service stops and joins its worker and closes the sockets. The SDK
reports uncertain custody and refuses another command in both schedules. The
owner observes cancellation only for the unclaimed ticket; a claimed ticket
remains dispatched and can finish after the client exits. This is orderly
service teardown, not an abrupt process crash. Explicit recovery on a replacement
service and remaining process-loss/negative-case coverage are still open.

Both peers now pass explicit recovery on a replacement production service.
They retain the old uncertain command, wait for an explicit new endpoint,
discover a different service identity and invoke a newly advertised probe
command. The replacement deliberately reuses the old connection/request numbers.
The old ticket's late Committed result does not settle the probe; only the new
owner's Rejected result does. The old command remains uncertain throughout.
No uncertain mutation is replayed. All three production SDK tests pass for
each peer. Abrupt process death and remaining integrated negative schedules
remain separate qualification work.

Both peers also pass abrupt death of a test-owned production service process
before and after command claim. The parent waits for the accepted/claimed
fixture marker, kills that exact child and waits for its non-success exit.
Each SDK retains uncertain custody and refuses another invocation. The effect
marker exists only in the claimed schedule. A replacement service gets a new
endpoint; the old endpoint refuses connection. All four external SDK gates
pass for each peer. The shared fixture's existing runtime-client crash gate
also passes. Integrated malformed/partial traffic, namespace denial and
release-wide parity remain separate audit items.

Independent integrated session tests now also reject a short write reply, a
status detail with the wrong byte count, and Completed for a Policy command.
Each leaves custody uncertain, refuses further invocation and sends no replay.
Together with the existing refusal, disconnect and wrong-generation cases,
all six failure schedules issue exactly one command and fetch one catalog.
Command dispatch uses a one-byte budget, including the successful three-command
sequence, which still fetches only two catalogs (the second after explicit
refresh). Both tests and strict Rust Clippy pass. No production code changed;
these checks establish bounded-progress correctness, not a measured speedup.
Namespace denial and the remaining release-wide audit were still open at that checkpoint.

Both peers now pass production HostDomain denial from a foreign user/PID/network
namespace. Bubblewrap keeps the socket path reachable; the peer requires a
successful connect followed by transport closure during negotiation, never a
timeout or a ready session. No command reaches the owner. Missing isolation
prerequisites fail the opt-in gate rather than silently skipping it. All five
production administrative gates pass independently for C and Rust; strict Rust
Clippy passes. This tests administrative HostDomain admission, not Factotum
authentication for other roles. Release-wide qualification remains open.

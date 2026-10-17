# C and Rust SDK API parity

## Queued shell submit (B2), 2026-10-17

Review follow-up: both SDKs retain a full successful submit reply as authoritative
custody on disconnect or terminal failure, including while the record reply or
journal event remains missing. This also aligns the ordinary AwaitSubmitted
path. Live progress still waits for the journal; the reply does not advance the
covering-ACK barrier. Malformed journal data fails validation but cannot erase
independently established custody.

C preserves its existing stale-session classification for ESTALE on the record
write, retaining the record errno while draining the paired submit. Rust continues
to report a record-write refusal as a protocol error, as before B2. Both stop the
connection and preserve the same custody; their terminal error categories differ.
The B2 C client layout already changed, before B3: rebuild every C consumer and
foreign-language binding with the matching header and library.

Both SDKs may issue submit behind the final record fragment before its reply.
They retain both outcomes, preserve observed custody and stop after a failed
record without retrying the incomplete assembly. Normal submit retry remains
unchanged after a successful record write. Caller APIs and buffer ownership
are unchanged; C's client storage layout grows, so consumers and bindings
must rebuild against matching headers and library.

Rust can queue covering ACK, record and submit together on a reusable writer.
C keeps its confirmed-ACK barrier, then queues record and submit together.
This is a scheduling difference, not different custody semantics. Independent
scripted tests establish ordering and failure handling. Independent startup
peers pass exports `116075541` and `feaf3cbc7`; broader export failures,
vendor adoption and native performance remain separate gates.

## Cumulative shell ACK scheduling, 2026-10-17

Rust's automatic mode now gathers consumed records until the bounded drive
would yield, or until the next transaction needs its covering ACK. Manual mode,
object holds and I/O budgets are unchanged; there is no delay timer. C already
coalesces consumed records in its explicit `sophia_ss_ack` call. Its caller keeps
choosing that boundary; no automatic ACK is introduced into a manual API.
Independent tests in both SDKs consume three records and send one cumulative
ACK. Existing snapshot-hold and custody-barrier controls pass. This is an
internal automatic-policy optimization in Rust, with the equivalent explicit
operation tested in C; it does not establish a native request-count reduction.

## Reusable shell transaction writer, 2026-10-17

Review follow-ups explicitly test that C delays discovery until Negotiated has
arrived even when Submitted was acknowledged earlier. Rust's scripted peer
withholds the covering ACK reply and observes the next record write outstanding
at the same time. C still waits for the confirmed ACK before the next record;
this scheduling difference preserves custody but may cost an extra service pass.
These tests establish ordering, not measured native performance.

The optional `transaction-reuse` node follows signed Sophia `ee22309ba`.
Both SDKs probe after negotiation and its covering ACK; only lookup ENOENT
selects legacy transactions. Successful writers survive submissions; covering
ACKs permit reuse, and definitive refusals close the writer before replacement.
C retains explicit caller-driven ACK scheduling and startup readiness; admission
waits for discovery and the writer barrier. Rust retains its automatic and manual
ACK modes. C consumers must rebuild against the changed client storage layout.
Independent scripted tests cover repeated reuse, refusal replacement and lookup
errors. Both independent startup peers pass the real export. Snapshot adoption and
native performance qualification remain pending; request reductions are not measured allocation savings.

The [2026-10-17 release audit](RELEASE-QUALIFICATION.md) records verified
C/Rust capability parity, exact tested revisions and remaining limits.

## Announced snapshot scheduling, 2026-10-17

Rust `set_automatic_snapshot_fetch(false)` lets a caller defer new snapshot
fetches. `pending_snapshot` exposes the current kind/generation/Qid;
`request_snapshot` admits that exact request without I/O. A stale identity
changes nothing. Dispatch performs the bounded fetch and verification before
delivering the ordinary typed snapshot. Re-enabling automatic mode resumes
waiting work; already started fetches and their bounded retries continue.
C already requires `sophia_ss_object` to start each fetch.

Scripted Rust controls prove no object walk before caller permission, wrong
identity rejection, duplicate request handling, automatic resumption and close.
C's descriptor tests independently assert no object walks before explicit
requests. Both peers pass Sophia's real metadata/content export scenario with
caller-requested descriptor fetching and explicit journal ACKs. The Rust shell
suite and strict Clippy pass. No new allocation or background task is added to
the scheduling decision; this is not a measured performance claim.

Rust now retains the latest pending identity in fixed storage for each feed.
`pending_snapshots` lets callers select any pending feed; `prefetch_snapshot`
also accepts an unannounced identity for a disclosed feed. One fetch runs at
a time. A verified identity releases only its exact matching publication;
an earlier unverified feed still holds cumulative ACKs. Scripted tests in both
SDKs cover prefetch before publication and a different-Qid refusal to release
the hold. Both independent peers pass the real metadata/content export while
fetching Tabs before the earlier Descriptors publication.

Prefetch before publication has scripted coverage only.
[Shell sizing](SHELL-SIZING.md) maps allocation-free candidate queries and their tests.

## Retained journal delivery, 2026-10-17

Rust `set_automatic_event_delivery(false)` retains one validated application
event. `journal_event` borrows its sequence and typed `JournalRecord` without
cloning. Repeated dispatches expose the same head; `consume_journal_event`
schedules its consumption without I/O or a duplicate receive-queue copy.
Dispatch applies its effects before later events and advances ACK eligibility.
Startup and Submitted custody remain SDK-owned, as in C. Existing automatic
delivery remains the default; switching modes cannot retract prior delivery
or queued ACKs. A retained head must be consumed before automatic delivery
resumes. Local close leaves the head readable but refuses further consumption.

C's `sophia_ss_event/consume` already provides caller-consumed delivery.
Independent scripted controls keep the head and ACK limit stable across
dispatches. Rust additionally checks ordinary event ordering, close retention,
automatic resumption and prefetch-before-publication-consumption. Both real
metadata/content peers hold each publication across dispatches before consuming
it, then recover a wrong-generation fetch and fetch Tabs before Descriptors.

Snapshot results still use Rust's existing owned receive APIs; they are not
journal heads. In manual fetch mode the caller controls replacement and can
take ownership of a result before requesting another. Automatic snapshot
coalescing and the documented failed-decode eviction behavior are unchanged.

## Explicit snapshot fetch failures, 2026-10-17

Rust explicit requests now retain a `SnapshotFetchFailure` with the requested
identity and error. Inspect `snapshot_failure`, then `take_snapshot_failure`
before requesting another fetch. Failed objects never release journal holds.
Automatic fetches keep their existing terminal-error policy; callers needing
recovery disable automatic fetching and choose requests explicitly, as in C.
Transport failures and local allocation failures still terminate the connection.
Rust retries EAGAIN/ESTALE a bounded number of times before retaining failure;
C returns the fetch error to the caller for its retry decision.

Rust scripted controls cover wrong Qid, generation, epoch, trailing bytes,
object size overflow, EIO and exhausted EAGAIN/ESTALE retries, followed by a
successful explicit retry. C's independent descriptor controls cover wrong Qid,
generation, epoch and trailing bytes without clearing ACK holds. Both peers
pass the real metadata/content export after requesting a wrong Tabs generation,
observing failure and fetching the correct identity on the same connection.
Malformed-object recovery is scripted evidence, not a production-server fault
injection. A failed Rust decode can discard an older undelivered snapshot;
the retained journal API above does not turn snapshot results into journal heads.

## Journal ACK scheduling, 2026-10-17

Rust's `ShellConnection::set_automatic_journal_ack(false)` defers future ACKs;
`acknowledge_journal` queues one through the current safe limit without I/O.
Automatic ACKs remain the default. C already provides explicit `sophia_ss_ack`.
Neither call bypasses unverified snapshot holds or retracts an already queued
ACK. Delaying progress can stall submissions or trigger server revocation.

Rust `journal_progress` separates handled, safe-limit, queued and confirmed
watermarks, with pending ACK/object flags. Handled means retained by the SDK,
not processed by the application. C's `obligations.consumed` follows explicit
event consumption instead; this new Rust control does not claim that the two
delivery models are interchangeable. Input/action ACKs remain separate.

Rust's manual-ACK tests cover caller deferral, no I/O on queueing, held replies,
return to automatic mode, close and the unverified-object barrier. C's
`selected_feeds_and_holds` independently checks no ACK before caller request
and no confirmation before dispatch. Rust's full shell suite and strict Clippy
pass. Both independent `shell_startup_peer` executables pass Sophia's real
export in metadata/content modes: fetch a descriptor snapshot, defer its ACK,
explicitly queue it, then wait for confirmation. Admission is supplied; no
protected launch, presentation or performance claim follows from this check.
Manual object scheduling and retained-head delivery are covered above;
[allocation-free sizing](SHELL-SIZING.md) has separate codec and allocation checks.

[Shell startup](SHELL-STARTUP.md) maps caller-driven connection, timers,
bounded progress and handoff in both SDKs, with independent export evidence.

The [transport and namespace map](TRANSPORT-PARITY.md) covers generic wire
operations, directory/attribute access, discovery and their ownership rules.

The [launch operation map](LAUNCH-PARITY.md) covers caller-driven negotiation,
timers, bounded I/O, relay reuse and the remaining ownership differences.

The [broker operation map](BROKER-PARITY.md) records the current broker API,
paired coverage, ownership and release limits. Checkpoints below retain their
original source dates; an older missing-implementation statement is not a
current inventory.

## Broker retry readiness, 2026-10-17

Rust `Transaction::retry_ready` / `Session::retry_ready` match C
`sophia_bt_retry_ready` / `sophia_br_retry_ready`. These pure queries distinguish
received EAGAIN from locally queued but unissued work. They return false after
retry issuance or terminal failure. EAGAIN retains Queued custody, not Refused;
applications must schedule explicit retries from readiness, not Refused custody.
Paired socket tests check initial queuing, EAGAIN, immutable retry and close.

## Broker submit-handle reuse, 2026-10-17

Both integrated broker sessions keep the opened `submit` fid until connection
close. Negotiation opens it once; later responses reuse it. Each candidate
still gets a fresh transaction assembly, and its clunk still waits for the
Submitted receipt acknowledgement. Standalone transaction drivers retain their
original ownership and clunk both handles. No wire record, retry identity,
deadline or public session operation changes.

Independent C and Rust socket peers require one submit walk across negotiation
and two responses, one assembly clunk per candidate, and no clunk before its
receipt ack. Existing fragmented writes, immutable EAGAIN retry, malformed
receipts and connection-loss scenarios pass. This removes three serial file
operations per later response; it is not a measured latency or CPU improvement.
The C transaction storage layout changes; rebuild consumers and bindings with
matching headers. Sophia pin adoption and the release cost comparison remain
separate steps.

Both revised peers pass Sophia's `external_sdk_broker_lifecycle` and
`external_sdk_protected_broker_lifecycle` against source `2af4195ab` with
Bubblewrap 0.13.0. These export gates cover negotiation and single-/two-row
responses; the independent socket peers provide the two-response reuse check.

## Current publication candidate, 2026-10-16

Both native peers now pass `external_sdk_protected_broker_lifecycle` in Sophia's
`broker_sdk` harness. Each launches through ProcessSupervisor/Bubblewrap with
MetadataBroker evidence and exact kernel peer admission, then completes native
negotiation, single-row retirement and two-row disclosure exchanges, receipt
acknowledgement and cleanup. The original socketpair gate also passes. These are
generic peers; the production reducer and live Session migration remain open.
Bubblewrap is 0.13.0. The peer explicitly uses a supplied socket path when
`SOPHIA_BROKER_9P_SOCKET` is set, otherwise supplied stdin; a failed path never
falls back. Strict peer/harness Clippy and C compilation pass.

Current peer SHA-256: C
`5bafac353d5f1d64642412627a3b02f737d7edd0e04384164146ec19cc3e60c9`;
Rust `b10603e16a7a8885bc30b7167c538a8b5a8b1e2a0e45ba8d469b88307fb5dd58`.
The peer changes and Sophia harness remain uncommitted.

Portal recipient capture/frame integration, portal requester sessions and broker
sessions are ready for signed source publication. Both complete SDK test suites,
the Nim requester binding smoke test, Rust all-target strict Clippy, formatting
and contract hash checks pass. The shared-core broker fixtures qualify the
listed exchanges, not the unfinished live migration.
Full t333 closure and Sophia pin adoption remain separate gates. The checkpoints
below preserve earlier evidence and their original qualification limits.

## Broker discovery checkpoint

C and Rust broker peers now pass the shared-core export gate
`cargo test -p sophia-runtime --test broker_sdk -- --ignored`, with
`SOPHIA_BROKER_SDK_PEER` naming each independent executable. Both exercise native
startup, negotiation, single-row retirement and two-row disclosure responses,
acknowledgement and cleanup. The owner checks one atomic effect commit. The
socketpair fixture uses synthetic protection evidence; it does not prove protected
launch or foreign-process admission. Production selection remains legacy.

Tested executable SHA-256: C `524fe2f37617c536743dd2c05253a8f26f214d55460088b35b02a35eedd47cbb`;
Rust `9aff103202df64216cd94a1d1c6f84a6cf037bb3a9cf5319fde3df7143af4938`.
Sources remain uncommitted in the existing SDK worktrees and Sophia.


Broker response checkpoint (2026-10-16): both SDKs pass 12 independent
socket scenarios. New coverage preserves a pending label containing NUL, newline
and multibyte UTF-8 after journal consumption. Both SurfaceAdmitted and
SetDisclosure two-row responses reject a bad second row locally, then accept the
corrected pair with submission ID two. The peer receives one complete 448-byte
candidate, not separate row submissions. Strict Rust Clippy passes.

Next: t314's production broker export and runtime migration. Its acceptance
requires every existing broker path through files, no legacy production caller,
a compiled foreign-attach negative, and latency/CPU/copied-byte comparison before
legacy retirement. The owning plan is `jsschoen` section t314; existing Session
integration is `crates/sophia-session/src/live_session/metadata_broker.rs`.
The client fixtures do not prove those server properties. Changes remain
uncommitted, and the installed desktop and SDK pins remain unchanged.


Broker deadline, loss and retry checkpoint (2026-10-16): both SDKs now
pass nine independent socket schedules. New cases prove that an acknowledged
request still expires at its response deadline, loss after response submission
leaves custody Unknown, and explicit EAGAIN retry preserves every candidate and
control byte. The deadline test first drains the journal ack and prior cleanup,
so an unrelated handshake or ack timer cannot satisfy it. Strict Rust Clippy
passes. Next: retained-label and two-row integrated response coverage, then the
production broker migration and its protected-admission and measurement gates.
No production qualification is claimed; changes remain uncommitted.


Broker session failure checkpoint (2026-10-16): both independent socket
fixtures now pass six schedules: the full lifecycle, stale receipt epoch,
contradictory negotiated limits, refusal through terminal acknowledgement, short
ack reply and wrong receipt ID. Stale or uncorrelated receipts leave custody
unknown; errors after a valid receipt preserve Submitted. Both sessions stop
being ready on failure. C callers close the borrowed fd; Rust closes its owned
connection. Test peers accept EOF/reset and broken pipe during deliberate closure;
all protocol and custody assertions remain required. Strict Rust Clippy passes.

Next: response timeout and connection-loss schedules, immutable EAGAIN retry,
label retention and two-row integrated responses. Production export, protected
admission and comparative measurements remain open under t314. No production
qualification or publication is claimed; all changes remain uncommitted.


Paired broker session checkpoint (2026-10-16): C `sophia_br` now supplies
the integrated session counterpart to Rust `session::Session`. Both retain one
request in fixed storage independently of journal consumption, route all replies
through one connection driver, encode responses once, and wait for receipt ack
before transaction cleanup. C borrows the transport and requires fd closure on
terminal failure; Rust owns it. Neither reconnects or replays work.

Independent C and Rust socket fixtures pass negotiation, request retention,
a 448-byte response, acknowledgement and both transaction clunks. All 16 Rust
broker client tests and eight protocol tests pass. C's new session compiles under
strict warnings and passes its lifecycle test. These fixtures do not yet qualify
failure behavior or the production export. Next: matched refusal, loss, stale
epoch, deadline, malformed-event and two-row session schedules. Then t314 still
requires the production migration, protected admission and comparative measurements.
Changes remain uncommitted; SDK pins and the installed desktop are unchanged.


Rust broker session checkpoint (2026-10-16): `session::Session` now owns
the pipeline and routes startup, journal, ack and transaction replies. It copies
one pending request into fixed storage so journal consumption cannot invalidate
its label; response construction encodes borrowed rows once. Dispatch shares a
bounded I/O budget. A literal socket fixture passes negotiation, request delivery,
response submission, acknowledgement and both transaction clunks. Strict Clippy
passes. This is one happy-path fixture, not production qualification.

The C integrated session is still missing. Its independently tested startup,
journal, transaction and ordering components are ready to wire together next.
Do not claim broker session parity, publish this capability or adopt SDK pins
until C implements it and both pass equivalent fault schedules and production
export gates. Rust also still needs refusal, loss, stale epoch, response deadline,
malformed event and two-row integrated tests. t314's export, protected admission
and comparative measurements remain open.


Broker ordering checkpoint (2026-10-16): C `sophia_bp` and Rust
`phase::Ordering` now check handshake receipt ordering, negotiated limits,
one pending request, increasing request IDs and response receipt correlation.
Refusal keeps a fixed deadline from local observation; acknowledging only the
preceding receipt cannot extend it. The server owns publication-time enforcement.
Paired tests and strict Rust Clippy pass. This component assumes journal and
transaction validation; it does not own request data, socket I/O, response timers
or ack/cleanup pacing. Those remain work for the integrated session. Production
migration, protected admission and measurement gates remain open under t314.


Broker transaction drivers now stage Negotiate and complete 448-byte
ResponseSet records, submit explicitly and clunk both fids after receipt
acknowledgement. C `sophia_bt` and Rust `transaction::Transaction` use fixed
candidate storage and borrow operation bytes; they neither poll nor own the
transport. One session driver must match tags and close on terminal failure.
The original deadline includes cleanup. The assembly deadline starts when the
first write enters the transport and never extends with progress. EAGAIN waits
for explicit retry of the same control. Connection loss preserves known custody.

Thirteen Rust broker client tests and the independent C transaction suite pass,
including fragmented staging for both record sizes, retry, receipt/write ordering,
acknowledgement before release, assembly expiry, disconnect and failed or timed-out
cleanup. Strict Rust Clippy passes. These are component tests, not production
export evidence. Next: integrate startup, journal and transactions into the broker
session, then qualify against t314's production migration and admission gates.


Both SDKs now discover broker metadata on the caller-admitted root: Rust
`startup::Opening` and C `sophia_bo`. They retain the connection, read exact API
and 72-byte Limits records through EOF, and require successful clunks before
handoff. Fixed metadata storage and bounded dispatch avoid per-fragment role
allocations. The caller retains the admission authority channel; discovery does
not authenticate or negotiate. Rust owns and closes failed connections; C borrows
the wire and requires its caller to close the fd on failure.

Paired socket fixtures cover seven-byte fragments, one-byte I/O budgets,
incorrect version/epoch, directory replies, truncated/trailing records, failed
clunks, deadlines and clock regression. Rust's nine broker client tests, the C
startup suite and strict Rust Clippy pass. Transaction I/O, session integration,
t314's production export, protected admission and comparative measurements remain
open. No SDK pin or installed desktop changed.

Both SDKs now decode the designed metadata-broker API descriptor and Limits:
C `sophia_bf_decode_api/limits` and Rust `sophia-broker-protocol`.
These allocation-free decoders validate exact API/role identity, canonical
nonzero epochs, envelope identity, reserved fields and all advertised bounds.
C leaves outputs unchanged on failure. Independent literal, truncation,
overflow and boundary tests pass; strict Rust Clippy passes.

The KDL is copied from signed Sophia
`519f07fd1b6db3a665ae1e74368a3d2e234cbf9c`, SHA-256
`d0e224ee984a0ce3efe8aee209af1a9fff6bb2997362f1586a346e1fa7c78cb8`,
and is included in `spec/SHA256SUMS`. The markdown remains at the same source.
Both now also decode Negotiated, Refused, Submitted and all five BrokerRequest
kinds. C `sophia_bf_decode_event` and Rust `decode_event` borrow label text
from the input record, preserve absent/empty/control labels and accept zero
surface/candidate-generation values for the reducer's domain denials.
Envelope classes, fixed padding, strict booleans, UTF-8 and per-kind unused
fields are validated without allocation. Four Rust tests and independent C
discovery/event suites pass; C failure leaves its output unchanged.

Both now encode one atomic ResponseSet with fixed storage, validating all rows
before touching output. Request ID/kind are taken from the supplied pending
request; surface, candidate-generation, attention and permitted row/rejection
relationships are checked. Both two-row lowering shapes, absent/empty/control
labels, present-zero icons and grant target generations have independent tests.
A failed second row leaves the entire output unchanged. Six Rust tests and
three C broker suites pass; strict Clippy passes. Current owner state, retained
generation/grant and enqueue-capacity validation remain server responsibilities.

Both now encode Negotiate, submit and cumulative ack controls. Unsupported or
inverted revision ranges remain representable for server Refused. Encoders
reject zero identities, invalid declared lengths and short output buffers before
writing; bytes beyond the record stay untouched. Eight Rust tests and four
independent C broker suites pass, with strict Clippy clean.

Broker submission components now retain a fixed 448-byte candidate and 24-byte
submit control: Rust `sophia-broker-client::Submission` and C
`sophia_bs_submission`. Explicit same-ID retry, both receipt/write orders,
acknowledgement before release, short/failed writes, clock regression and
disconnect custody pass paired tests. Borrowed request/label inputs are encoded
at construction; no later caller lifetime or allocation is needed. Event values
now retain the validated epoch for receipt correlation. The C event struct is
unreleased and its layout changed; rebuild clients/bindings before adoption.

Four Rust custody tests plus eight protocol tests and the independent C custody
suite pass; strict Clippy passes. These components share no portal authority or
runtime dependency.

Both now retain fragmented/coalesced events in a fixed 512-byte journal:
Rust `Journal` and C `sophia_bj`. Event labels borrow the head until consume;
reads pause while it is held, and buffer compaction occurs only when needed.
Acknowledgements freeze their sequence/bytes until the write reply; later
consumption cannot alter them. Three Rust journal tests and the independent C
suite pass borrowed pointer lifetime, fragmentation, compaction, frozen acks,
gap/stale-epoch/malformed-text refusal, clock bounds and deadline expiry.
The local timer begins at observation, not server publication; the separate
Refused drain timer remains session work. No allocation or performance gain is
claimed beyond the fixed-storage implementation.

Startup, transaction I/O and full broker sessions remain absent.
t314 still requires protected admission, atomic
responses, broker migration and comparison measurements before retirement.

Matrix revision 48, 2026-10-16. Sophia t333 owns the complete parity gate.
This matrix maps shell event-loop and content-admission operations; it is not a complete role
inventory. Contract inputs are the pinned API-1 shell files and WM custody
rules in `spec/`, checked by `spec/SHA256SUMS`. C comparison source:
`sophia-desktop-sdk-c` `a90e235` (unreleased 0.10.0-dev; see below).

| Operation | Native C | Native Rust | Contract or remaining difference |
| --- | --- | --- | --- |
| Monitor the socket | `sophia_ss_poll_fd` | `ShellConnection: AsFd` | C retains caller ownership; Rust lends its owned descriptor. Never bypass SDK I/O. |
| Read/write interests | `sophia_ss_poll_events` | `ShellConnection::io_interest` | Read while live; write only for queued transport bytes. Recompute after progress. |
| Local work | Submit queues transport output; drain `sophia_ss_event`, consume and acknowledge obligations | `io_interest().immediate`; drive `poll_io` and drain typed receive methods | Do not sleep with undrained work. Rust also flags newly admitted work and exhausted drive rounds. |
| Nonblocking service | `sophia_ss_dispatch` | `ShellConnection::dispatch(now, byte_budget)` | Both bound the whole call to the supplied bytes and 32 syscall attempts per direction. Zero budget permits local processing without socket I/O. Rust's convenience `poll_io` retains its separate fixed limits. |
| Retry timer | `sophia_ss_timeout(now_ms)` | `wake_deadline()` with `dispatch(now, byte_budget)` | Both accept caller time and reject backward supplied times before changing state. Rust uses `Instant`; C uses monotonic milliseconds. |
| Retry identity | Same staged transaction, epoch and submission ID | Same staged transaction, epoch and submission ID | Neither retries in the refusal pass or replays across epochs. Both start at 4 ms and double to 256 ms; progress can permit an earlier retry. |
| Terminal connection | Poll interests become zero; inspect session state/error | `io_interest()` returns a terminal error | Stop monitoring; drain already received events and inspect custody. No automatic reconnect or replay. |
| Convenience wait | Caller composes poll, timer and dispatch | `wait_for_io` | Rust retains its existing convenience method. |

Rust checks: `external_loop_interest_tracks_queued_bytes_and_buffered_replies`,
`external_loop_drains_local_events_and_does_not_watch_idle_writability`,
`external_loop_stops_monitoring_a_closed_connection`, malformed-reply poison
controls, and the existing readiness/deadline tests. C counterparts:
`poll_reports_queued_output` in `shell_session_events_test.c` and
`eagain_waits_for_progress` in `shell_session_custody_test.c`. Both suites use
independent scripted peers. These checks do not establish production-export
qualification or physical desktop acceptance.

Dispatch controls add a Rust test that advances caller time through the full
4–256 ms schedule without sleeping, preserves staged transaction identity and
rejects backward time without progress. One-byte shell dispatches still reach
Submitted custody. Generic transport tests verify exact partial writes and
reads and that reusing a budget does not replenish it. C's matching partial
request control is `service_byte_budget_bounds_partial_requests`; its existing
`clock_rules` checks backward time and timestamp saturation.

Clock representations differ at their upper limit: C saturates deadlines at
`UINT64_MAX`; Rust rejects a supplied `Instant` that cannot represent the maximum
retry interval. Neither wraps time. With Rust's explicit driver, use `take_*`
receives; `poll_*` and `wait_for_io` drive the default clock and budget. Simulated
time must not be mixed with those convenience calls. These are documented API
differences, not evidence that every shell operation already has a counterpart.

## Content admission and reservations

| Operation | Native C | Native Rust | Contract or remaining difference |
| --- | --- | --- | --- |
| Atomic admission | `sophia_ss_submit` | `enqueue_content_batch` and existing single/group methods | All encoded units enter the bounded queue or none. Rust batches accept independent content records and whole candidates, each with its own transaction and ticket. Local admission is not server custody. |
| Reserve before choosing records | `sophia_ss_reserve` | `reserve_capacity(slots, bytes)` | One live token; dispatch and admissions using unreserved capacity can continue. Rust reserves from its bulk allowance, preserving control credits. |
| Commit delayed content | `sophia_ss_commit` | `commit_reserved_content_batch` and single/group methods | Both retain credits on invalid input/size and issue tickets only on successful admission. Rust still encodes/allocates at commit. |
| Prepare known records | Reserve then commit | `reserve_content_batch` and single/group methods | Rust's additional prepared handle exclusively borrows the connection; its commit needs no allocation, I/O or capacity check. |
| Cancel | `sophia_ss_cancel` | `cancel_reservation(&token)`; prepared handle `cancel`/Drop | Delayed-content tokens require explicit cancellation in both SDKs. Rust's prepared handle alone cancels on Drop. No admission or tickets. |
| Token identity | Session address, connection epoch and serial (0.10.0-dev) | Private shared identity checked against the connection | Wrong-connection and stale tokens cannot consume the live reservation. Do not move/reinitialize C session storage while a reservation is live. |
| Custody query | `sophia_ss_outcome` | `custody(Ticket)` | Local/admitted, in-flight, Submitted, refused errno, dropped-unsent and unknown map to the corresponding Rust values. Submitted is not semantic acceptance. |
| Old/unknown ticket | UNAVAILABLE for evicted, ARGUMENT for unissued | `custody_result` returns `Unavailable` or `Unissued` | Existing `custody` retains its `Option` API. Both keep the latest 256 tickets; unavailability is not an uncertain remote outcome. |

Rust scripted controls cover cancellation/drop without tickets or I/O, commit
to Queued and then Submitted, invalid groups, full queues, upload-slot reuse,
and a peer disappearing while the reservation is held. A compile-fail example
checks exclusive borrowing; an outbox test checks capacity acquisition before
commit and preservation of payload storage. C's `reservation` control covers
cancel, capacity isolation, invalid commits and closure while reserved.

Rust preparation uses the existing encoder and local checks. Runtime grant,
upload and semantic decisions still belong to the session. For lifecycle-aware
candidate registration, use the existing `enqueue_candidate_tracked`; a plain
content reservation does not update `ContentLifecycle`. Native lifecycle
helpers and batch coverage beyond content records remain open under t333.

`ContentBatchItem` describes one content record or whole candidate. A batch has
1–64 operations, subject to remaining byte/slot credits; each item produces one
ticket and its own server submission or upload write. Mixed batches use bulk
credits; only all-ACK batches use control capacity. Staged resource starts take
distinct free slots, and failure or cancellation commits none of their claims.
An atomic local admission is not a server-wide transaction: later refusal or
disconnect can settle each ticket differently.

Rust batch tests cover transaction/order preservation, invalid middle records,
count/byte/slot limits, delayed reservation preservation, control headroom,
resource-claim rollback and mixed custody after disconnect. C's
`admission_atomicity` already checks invalid middle records, byte/slot bounds,
queued versus in-flight custody and terminal classification; its full suite
passes at `a90e235`. This does not yet map every descriptor, catalog or native
input operation into a common batch API.

The delayed-content Rust token now survives connection service. Tests cover
reserved byte/slot isolation, control credits, unused-credit release, invalid
records, wrong-connection/stale tokens, cancellation and terminal release.
Allocation failure can still refuse a delayed-content commit while preserving
credits; use a prepared handle when the local side effect needs an infallible
commit. Rust takes nonzero bounded byte/slot counts. Both queues are configurable;
the accounting and optional Rust headroom are described below.

C regression `reservation_is_bound_to_its_session` failed on `cfa6561`: two
live sessions with the same reservation serial accepted one another's token.
The companion fix checks owner address and epoch as well as serial. This expands
the public reservation struct; 0.10.0-dev is unreleased and C consumers must
recompile before adoption. It changes no wire record or server authority.

Remaining inventory: every shell revision, operation, event, terminal outcome
and native lifecycle helper; generic transport, launch and
namespace ownership; WM/output clients missing in Rust; experimental lock client
missing in Rust; administration missing in both. Broker and portal coverage must
follow their accepted public contracts. No missing capability is supplied by an
IPC fallback. A codec alone does not establish client parity.

## Custody lookup and native lifecycle prerequisites

Rust `custody_result` distinguishes an evicted ticket from zero or a number
never issued by that connection. Retained `Custody::Unknown` remains a separate
outcome. Queries perform no I/O and work after disconnect. Tickets belong to
one connection; numeric tickets do not detect accidental use on another one.
Rust ledger and scripted disconnect checks match C's `old_and_unissued_tickets`
retention-boundary test and existing uncertain-custody controls.

## Native launcher transport

The Rust client now supports the pinned revision-7 launcher profile: exact
capability bits 5, 7, 8 and 11, with Limits. Other roles cannot use its native
record API or deliver native events. The public entry points map as follows:

| Operation | Native C | Native Rust |
| --- | --- | --- |
| Allocation, input ACK, activation | `sophia_ss_submit` with native records | `enqueue_native_launcher_tracked` |
| Whole native candidate | `sophia_ss_submit` with NativeCandidate | `enqueue_native_candidate_tracked` |
| Observe native/shared events and catalog | `sophia_ss_event`, consume; `sophia_ns_next` adds lifecycle policy | `take_native_launcher_observation`, transport only |
| Keyboard activation plus ACK | `sophia_ss_submit`; helper `sophia_ns_input_reserve/commit` | `reserve_native_input_response`, or `enqueue_native_input_response` |

Rust's prepared response validates matching event identities and distinct
transactions, then holds both records before a UI edit. Activation precedes
ACK. Commit admits both or neither locally, with independent custody tickets;
Drop cancels. It does not decide whether focus permits activation, infer an
input disposition, or enforce the input ACK deadline. The existing content
reservation handle is reused; it exclusively borrows the connection.

Use the native observation stream exclusively when ordering matters: native,
content and catalog receive methods otherwise remove items from the same inbox.
The catalog carries application entries without persistent identities. Existing
snapshot coalescing and bounded fetching still apply. Journal acknowledgements
confirm delivery, not application input acknowledgement or action completion.

Seven Rust scripted-peer checks cover exact negotiation, all native events,
catalog ordering, allocation/whole-candidate custody, cancellation, invalid
response identity, queue saturation, malformed input, foreign epochs and
separate custody after disconnect. C `f8ca212` passes its full suite, including
`sophia_shell_files_roles_test` and `native_session_test`'s `test_input` and
`test_activation`. C helper tests use a session stand-in; these paired checks
are not production-export or physical qualification.


## Native presentation and keyboard lifecycle

Rust now supplies `NativeLauncherLifecycle` for the presentation/input portion
of C's `sophia_ns`. `observe` applies ordered decoded observations; `input`
retains an input until its response commits. Opening IDs must rise, Prepared
never enables targets, focus must match the shown candidate and current revision,
and a later exact focus supersedes the previous lease without a fabricated
lease-order rule. Activation admission disarms the opening; close and disconnect
remove input authority. A late Presented for a closed opening cannot restore it.

`enqueue_native_candidate_with_lifecycle` admits the exact candidate and its
metadata together; failed admission registers nothing. `prepare_native_input`
checks eligibility and reserves the response before an edit. Drop cancels and
retains the input; commit records its tickets and consumes it without allocation
or I/O. `service` reconciles custody, counts refused ACKs once and disarms a final
connection. No automatic retry of application effects or reconnect occurs.

The input deadline is issuance microseconds divided by 1000 plus the timeout,
saturating at the clock representation limit, matching C. It is an early host
CLOCK_MONOTONIC scheduling deadline, not proof of timely server service.
`NativeLauncherObservation::Published` exposes catalog/output announcements
before fetching completes; old local scenes are disarmed at announcement, not
merely when the replacement object is installed. Exhaustive matches must add
this variant. Consume one ordered stream; feed a snapshot after accepting it
for scene construction. The reducer does not render or install UI state.

Nine Rust lifecycle controls and the existing transport checks pass, as do
workspace tests, strict Clippy and C's full suite at `62b62f6`. C counterparts
are `native_session_test`'s focus, input, activation, catalog-invalidation,
close and terminal controls. That C helper suite uses a session stand-in;
production-export acceptance is still pending.

## Native resources and pacing

Rust now tracks allocation and resource ownership and frame demands/permits:

| Operation | Native C | Native Rust |
| --- | --- | --- |
| Acquire/resize/release | `sophia_ns_allocate`, `sophia_ns_release` | `allocate_native`, `release_native_allocation` |
| Upload | `sophia_ns_upload_begin/end/cancel`, `sophia_ns_resource_cancel`, `sophia_ss_upload_chunk` | `begin_native_resource`, `upload_native_chunk`, `end_native_resource`, `cancel_native_resource` |
| Retire/release | `sophia_ns_retire`, resource events | `retire_native_resource`, `observe` |
| Demand/cancel | `sophia_ns_demand`, `sophia_ns_demand_cancel` | `demand_native_frame`, `cancel_native_demand` |
| Construct a scene | `sophia_ns_present` | `present_native_scene` |
| Reconcile time and custody | `sophia_ns_service` | `service_at` |

Both retain at most 16 resource identities and one upload. Rust copies each
caller chunk and returns a Stored custody ticket; C borrows its buffer until
upload progress finishes. Caller writes may split the declared layout chunks.
Rust validates layout against Limits and requires Stored before another write
or End. Failed payload is not replayed; cancel the resource. Both SDKs offer explicit-resource cancellation after a refused End. C's
`sophia_ns_resource_cancel` also delegates current-upload cancellation to the
existing transport wrapper. Neither helper releases the resource identity until
its terminal event. Independent tests cover refused End, cancellation and
identity retention; C also checks queue saturation and stale generations.

A rejected resize preserves the grant. Close removes local allocation use but
waits for server invalidation before replacing an orphaned grant. Resources
retain their own retirement/release lifetime. Submitted retirement is not release;
a refused retirement restores Accepted state. The Rust scene helper uses current
allocation/scale and accepted resources, fills context generations, checks byte
bounds and consumes a permit only on successful local admission. The lower-level
candidate API still registers metadata without resource/pacing policy.

Permit time is advisory demand enqueue plus TTL, with a caller margin. Local
expiry disarms use but keeps the demand outstanding until authoritative end;
neither SDK can prove timely arrival from that bound. Tests cover candidate
refusal before and after permit expiry. C's new regression failed on `c678319`
before repair: expiry first caused an indefinite wait for the same end. The fix
preserves that terminal observation and leaves newer demands untouched.

The Rust file wire also leaked upload slots after definitively refused Begins;
a six-refusal regression failed before repair. It now releases the pending
claim and drops queued payload for vanished bindings without blocking later
records. Full Rust tests, strict Clippy and C `make check` pass; 25 Rust native
checks include paired allocation, resource, pacing and refusal cases. Export
qualification, native capture and performance claims remain separate.

## Native pointer responses

C `sophia_ns_next`/`sophia_ns_action_ack` and Rust `observe`/`action`/
`prepare_native_action` retain invocation and dismissal until their exact ACK
is admitted. Cancellation owes no ACK. Both reject activation without current
presentation, focus, revision and catalog eligibility, and permit any displayed
row rather than only the keyboard selection. Both submit activation (cause 2)
before ACK, atomically for local admission, with independent remote outcomes.
Rust additionally exposes a prepared response: Drop preserves the action and
commit performs no I/O or allocation. C exposes immediate atomic submission;
it has no separate prepared pointer handle because no local text edit occurs.

Independent C and Rust tests cover the nonselected row. Existing C controls
cover busy admission, cancellation and one activation at a time. Rust controls
also check exact encoded ACK fields, stale presentation, unknown rows, revoked
focus, dismissal, cancellation, prepared cancellation, saturated pair admission
and both split-refusal orders. Full Rust workspace tests, strict Clippy, the
final 30-test native suite and C `make check` pass. Production exports remain
pending; these are SDK-level controls.

## Owning native facade and obligations

Rust `NativeLauncherSession` owns a fresh negotiated connection, retains the
current observation, assigns transaction IDs and exposes the native resource,
allocation, pacing and response operations. `next_event` applies a head once;
`done` cannot skip input or an Action owing an ACK. Response commit consumes it;
Drop leaves it available without consuming transaction IDs. C `sophia_ns` borrows
caller-owned `sophia_ss` storage and requires exclusive submission; its cancelled
input reservation may leave unused transaction IDs. Both models avoid reuse of
issued identities.

Rust integrates bounded dispatch, descriptor/interests, transport wake deadlines
and lifecycle service. C callers dispatch `sophia_ss` then service `sophia_ns`.
Rust `obligations` matches the C scheduling hints for input, action, allocation,
candidate and permit deadlines, missing snapshots/candidates, invalidation and
lost ACKs. Action receipt plus timeout is only a hint; repeated head reads must
not restart it. Neither SDK treats these hints as server authority. Rust's
Instant transport clock and host-monotonic millisecond lifecycle clock must each
advance monotonically. C uses host-monotonic milliseconds throughout.

Rust applies fetched catalog/facts generations as their observations are returned;
the caller installs their contents before presenting. C explicitly marks fetched
snapshot generations with `sophia_ns_catalog/facts`. Rust's facade takes ownership
on construction and drops the connection on construction failure; it requires
Limits still in the fresh connection's inbox. C initialization leaves borrowed
storage ownership with the caller.

Five Rust facade controls cover initialization, retained input, response Drop,
transaction exhaustion, clocks, disconnect and action hints. C independently
checks that rereading an Action does not restart its hint, alongside its existing
head and obligation controls. Full Rust workspace tests, strict Clippy, the final
35 native tests and C's full suite pass. Rust `presented` now exposes borrowed metadata/rows for the last Presented
candidate, matching C `sophia_ns_presented`; Prepared alone never supplies it,
and close/failure clears it. Neither accessor proves current focus eligibility.
Rust `apply_native_input` and owning-session `apply_input` match C
`sophia_ns_input_apply`: reserve before editable application code, invoke once,
then choose Consumed or Stale from its result. Uneditable input invokes no edit.
Rust pre-encodes both equal-size dispositions before the callback; selecting and
committing afterwards allocates nothing. C mutates its fixed record then commits.
A Rust callback panic fails the lifecycle closed and resumes unwinding, since
external edits cannot be replayed safely; C callbacks must return normally.

Independent tests cover a declined edit's Stale ACK. Rust additionally tests both
wire dispositions, full-queue suppression, uneditable input and caught panic;
C's existing tests cover reserve-before-edit and repeated heads. Full Rust tests,
strict Clippy and C tests pass; the native Rust suite now has 38 tests. The general batch API below closes delayed non-content admission; queue configuration is described below.
These gaps still block full t333 parity and production adoption.

## General role batches and delayed admission

Rust `ShellBatchItem` covers content records/whole candidates, native records
and whole native candidates, descriptor records, indicator activations, whole
catalog candidates and catalog activations. `enqueue_shell_batch`,
`reserve_shell_batch` and `commit_reserved_shell_batch` match C's immediate
`sophia_ss_submit` and delayed `sophia_ss_reserve`/`commit` across those records.
A one-item batch supplies delayed single-record admission. Prepared Drop cancels;
delayed validation/size failure preserves credits. Local admission is all-or-none,
with a ticket and independent remote outcome per item. Input is copied.

Mixed batches use bulk credits; all-control batches use control headroom.
Native and catalog negotiation checks still apply. Catalog candidates retain
their whole-record validation and chunk limit. These lower-level operations do
not register lifecycle state or prove eligibility/response correlation; use the
role helpers for those checks. A batch does not turn server effects into one
atomic transaction. C has the same caller responsibility for raw submissions.

New Rust controls cover delayed native pairs, prepared cancellation, retained
credits after invalid direction/size, descriptor pairs, catalog role rejection,
and whole catalog/activation records. C independently submits a delayed
descriptor pair through its real session peer. Full Rust workspace tests,
strict Clippy and C's full suite pass. Existing content batch controls still
cover staged upload rollback. Production-export qualification is pending.

## Configurable queue limits

C configures `queue_slots` (1–64) and `queue_bytes` (8 KiB–512 KiB) at session
initialization. Rust `QueueLimits` configures 1–64 total records and 1 byte–2 MiB,
with separate optional control-record and control-byte headroom. Every C queue
size is representable. Rust defaults remain 64 records/2 MiB with 32 records/
8 KiB reserved from bulk use. Zero headroom selects C's shared-pool policy.
Neither configuration changes server Limits or per-record codec bounds.

Rust `set_queue_limits` requires a live queue with no admitted units or delayed
credits; prepared handles prevent mutation through their exclusive borrow.
Invalid configuration and busy resizing preserve the old limits. Settings are
logical admission bounds and do not shrink retained allocator capacity. Configure
before transferring the connection into an owning native session.

Accounting differs with storage ownership: C copies a queued record into separate
transaction storage at handoff and frees its queue credits then; at most one
additional record is in flight. Rust transfers ownership without copying and
keeps that record charged until settlement. Thus a two-slot C queue can retain
two queued records plus one in flight, whereas Rust's two-record limit includes
that flight. This implementation difference is explicit; identical settings do
not promise identical admission timing or memory use.

Independent descriptor tests exercise two-record shared pools and document that
boundary. Rust controls cover byte/record headroom, delayed credits, invalid
limits and attempted resize while the wire owns bytes. Full Rust workspace
tests, strict Clippy and C `make check` pass. Production export checks remain
pending under t333.

## Shell session operation audit

| C operation | Rust counterpart | Ownership or control difference |
| --- | --- | --- |
| `sophia_ss_open_fd[_staging]`, `open_attached[_staging]` | `Connecting::direct/attached`; blocking `connect_files/from_attached` | C borrows fd/storage; Rust owns the pipeline. Both support caller-driven startup; see [startup controls](SHELL-STARTUP.md). |
| `state`, `limits`, `refusal` | construction result/`welcome`, Limits observation, typed errors | Rust returns admission refusal during connection rather than retaining a half-initialized session for querying. |
| `poll_events`, `dispatch` | `AsFd`, `io_interest`, `dispatch` | Whole-call budgets and monotonic-time rules above. |
| `record_bytes`, `submit` | `shell_file_*_bytes`, `enqueue_shell_batch` | Both size application candidates without allocating or reserving capacity; Rust validates a supplied header. See [sizing](SHELL-SIZING.md). |
| `reserve`, `commit`, `cancel` | capacity tokens and `commit_reserved_shell_batch` | Same all-or-none admission; Rust prepared handles additionally borrow exclusively and cancel on Drop. |
| `outcome` | `custody_result` | Retained unknown versus evicted/unissued distinctions above. |
| `event`, `consume` | `journal_event/consume_journal_event`, `set_automatic_event_delivery`; typed `take_*`; native `next_event/done` | Rust can retain a borrowed typed journal head until explicit consumption, or automatically deliver owned observations. Retained heads remain readable after local close. |
| `object`, `object_result` | `pending_snapshots/request_snapshot`, `prefetch_snapshot`, `snapshot_failure/take_snapshot_failure`, typed observations | Rust defaults to automatic fetch/retry; explicit fetches retain failures and allow retry. Both validate identity before releasing journal holds. See recovery and delivery limits above. |
| `ack_limit`, `ack`, generic `obligations` | `journal_progress`, `acknowledge_journal`, `set_automatic_journal_ack`, readiness and retry wake deadline | Rust defaults to automatic ACKs and counts SDK-retained events; C counts caller-consumed events. Both preserve snapshot barriers. Delivery ACK is separate from application input/action ACK. |
| `upload_begin/chunk/ready/pending/end/cancel` | resource records and Stored tickets; native upload helpers | C borrows chunks; Rust copies them and exposes custody. In-flight/queue accounting above. |
| `close` | `ShellConnection::close`, `NativeLauncherSession::close` | Local terminal classification without I/O. C leaves borrowed fd open; Rust keeps its owned fd until Drop. |

Local close preserves observed Submitted, classifies an issued uncertain submit
as Unknown and unsent tails as DroppedUnsent, releases delayed credits, blocks
new work and removes retry deadlines. Rust close is idempotent and retains
already-decoded observations. The native facade also disarms retained input and
lifecycle authority. Three Rust controls and C's existing staged/issued/retained
custody controls pass, alongside full workspace tests, strict Clippy and the full
C suite (43 Rust native tests).

Both SDKs now expose manual object scheduling, retained journal delivery and
allocation-free candidate sizing. Rust keeps automatic delivery as its default;
the controls and ownership differ as documented above. Codec coverage alone
does not prove full parity. The complete role audit and release/export gates
remain under t333.

## WM protocol foundation; session client pending

Rust `sophia-wm-protocol` extracts Sophia's neutral WM file codecs and passive
policy values at `1d071c144199548b55d8c42a961aa3e13b60ef74`. Its API/schema/docs
and golden corpus match the C SDK contract pins byte for byte. It imports no
Engine, X11, IPC or authority owner. Source paths and hashes are in the crate's
`EXTRACTION.md`; shared OutputId/TransactionId remain SDK identifiers.

The crate provides file envelope/section/submit/ACK validation; Limits and
capability negotiation; all profile handoff commands/completions; complete
snapshot/configuration/projection records and extension rows; every Cycle
cause; dirty/session-operation records; outcome and presentation receipts.
The 71 extracted controls cover literal envelopes, golden rows, capability gates,
truncation/reserved bytes, complete arrays and neutral semantic validation.
Full Rust workspace tests, strict Clippy and C's independent suite pass.

This is the codec foundation, not the missing WM session. Rust still needs C
`sophia_ws` equivalents for direct and already-authenticated attached startup,
caller-clock bounded dispatch/readiness, exact negotiation, retained event head,
submission/ACK custody barriers, immutable same-ID retries, fixed deadlines,
64-ticket retention, optional domain transaction allocation, Cycle-bound
snapshot identity/EOF checks, release and explicit close. No profile activation,
scene commit or application authority follows from codec-valid records. Paired
session and production-export tests are required before WM parity is complete.

Rust's developing `sophia-wm-client` now maps C's candidate record union to
`Candidate`: profile Prepared/Active/RolledBack, Configuration, Dirty,
Projection and SessionOperation. `encode` validates the selected capabilities
and supplied identities, then owns immutable bytes. Profile transaction IDs
remain server echoes; only configuration, projection and session operations
contribute a domain transaction. Five tests cover every variant, a literal
Dirty record, caller mutation, missing capabilities and invalid identities.
These are preparation APIs only: no admission, retry or custody is claimed.
The C implementation is unchanged; the session checklist above remains open.

The provisional Rust startup driver now opens API/Limits/events/submit/ACK on
a versioned pipeline, either attaching for supplied-admission fixtures or
continuing the authenticated root. It validates bounded objects through EOF,
refuses impossible offers and undersized control iounits, retains one deadline,
and shares a byte/syscall budget across at most 32 dispatch rounds. Five scripted
peer tests cover fragmentation, no reset/reattach, malformed files, clock rules,
expiry without sending and capability refusal. Strict Clippy passes. C's
`wm_session/bootstrap.c` remains the counterpart; no C implementation changed.
Rust startup is not negotiation: it issues no candidate and claims no custody.
Direct nonblocking version negotiation, final handoff-capacity checks, the
session driver and production-export tests remain required before release.

Rust's provisional journal now assembles all WM event kinds, validates
capability selection and scans transport controls behind a retained application
head. Consumption never acknowledges beyond that head. Malformed records stop
the scan; valid earlier controls remain established. Record/byte bounds, epoch
and sequence checks, and a fixed fragment deadline match C's
`wm_session/events.c`. Eleven independent literal-record tests cover retained
heads, custody ordering, malformed barriers, every fragment split, deadlines,
64-record retention and capability dependencies. The 21 WM client tests and
strict Clippy pass. The transport-control callback still needs the submission
driver's ticket validation; this is not an integrated custody or session gate.

Rust's provisional submission state machine now owns one immutable candidate,
keeps 64 custody results, allocates independent domain IDs and waits for the
Submitted ACK before the next transaction walk. It retains candidate bytes and
submission ID across EAGAIN, with a later-pass requirement and 4–256 ms backoff
or journal progress. Submitted wins either reply ordering; EALREADY requires
that observation. Disconnect preserves Submitted/refusal and distinguishes
unissued work from uncertain submission. Nine tests cover these rules, partial
writes, deadline bounds, history eviction and the journal-control connection.
All 30 WM client tests and strict Clippy pass. C's counterpart remains
`wm_session/submission.c`. The Rust operation driver must still connect this
state machine to actual pipeline tags, ordered reply draining and terminal
handling; scripted session/export qualification is not yet established.

Rust `Session` now joins startup, journal and submission on the actual 9P
pipeline. It owns request tags, processes journal replies before submission
replies, preserves final custody on EOF, services ACKs and exposes readiness,
deadlines, retained events, domain allocation and close. One dispatch shares
its budget across bounded rounds. Unix-socket peers exercise startup through
negotiation and application submission, both reply orders, EAGAIN, ACK barriers
and EOF after Submitted. All 32 WM client tests and strict Clippy pass.
This closes the driver connection gap above, not WM parity: snapshot fetching,
direct nonblocking version startup, handoff-capacity validation and complete
paired production-export qualification remain. C's existing driver is unchanged.

Rust now exposes `request_snapshot`, `snapshot_result` and `release_snapshot`
against C's `sophia_ws_snapshot` family. A request freezes the retained Cycle's
epoch, transaction and scene generation. The driver bounds bytes to 1 MiB,
checks the declared length and EOF, and waits for clunk before exposing decoded
rows. It allows one fetch/result, retries only pre-open EAGAIN with bounded
backoff and never extends the original deadline. Socket-peer scenarios cover
fragmented reads, Cycle consumption, pre-open retry, identity/length/EOF
mismatches, read/clunk refusal and expiry. All 35 WM client tests and strict
Clippy pass. Direct nonblocking version startup, handoff capacity checks and
paired export qualification remain; no production pin or C implementation changed.

Rust now supplies nonblocking `VersionHandshake` and WM `Connecting::direct` /
`attached`. Constructors perform no socket I/O; version, fixed-file startup
and role negotiation retain one caller deadline. Handoff verifies quiescence,
the exact sole root, eight request slots, sixteen fids, msize at most 64 KiB,
and input capacity of two negotiated messages plus 64 bytes for parallel reads
and scalar replies. C already exposes nonblocking version/session startup and
checks its caller-storage capacity; its storage model needs no matching layout.
Six generic handshake tests cover fragmented replies, errors, expiry and
unconsumed following bytes. WM socket tests now use the nonblocking entrypoint;
four added controls cover capacity, root ownership, deadline continuity and
invalid offers. The workspace suite, added tests and strict Clippy pass.
These close the startup gaps above. Complete WM operation/error coverage and
paired production-export qualification still gate release and pin adoption.

The [WM operation matrix](WM-PARITY.md) now maps every C session entrypoint,
ownership differences and remaining qualification. Rust adds the passive
obligations view and fixes Connecting's missing negotiation retry wake.
The socket regression advances to the advertised EAGAIN wake, not the much
later bootstrap deadline; terminal flags and snapshot readiness are checked.
Subsequent changes add startup introspection, paired fault checks, supplied
production exchange and protected admission/revocation; WM-PARITY.md records
their exact scope. Release and pin adoption remain open.

## Output protocol foundation

Rust now provides `sophia-output-protocol`: API-1 envelope, controls, Limits,
topology and proposal codecs with passive output values. It is extracted from
signed Sophia `ad2fa4962ba1e7626b601e1a5b6b0ab95324bfc3`; the extraction
manifest records 21 original source hashes and the adaptations. Its schema
and normative file contract match the C SDK byte for byte. All 30 extracted
tests, the Rust workspace suite and targeted strict Clippy pass.

This matches the C `sophia_output_files.h` codec surface, not the session
in `sophia_output_session.h`. Rust still needs startup, observe-required
negotiation/refusal, proposal custody/retry, automatically fetched topology
with exact publication/Qid identity, ordered events/ACK barriers, external-loop
service and terminal/recovery behavior. Output qualification remains open.
Candidate syntax must preserve semantically invalid values for the server's
topology owner to refuse; client parsing cannot assume authority.

The [output operation map](OUTPUT-PARITY.md) now tracks provisional Rust
fixed-file startup and the remaining session operations. Startup shares one
deadline, validates API/Limits through EOF, opens the role files and preserves
an authenticated root without reattaching. Seven socket tests and strict
Clippy pass. The provisional immutable submission lane now has nine tests for
custody, retry, deadlines and output-specific domain/refusal rules. C independently
checks out-of-order domains and EALREADY refusal through both constructors.
The provisional journal now validates negotiation/refusal and ordered events,
with seven literal-wire tests. C independently checks all six outcome kinds
and held-head ACK ordering. The provisional topology fetcher now checks publication identity through EOF
and clunk in five matched-reply tests. A developing session now joins the components, with three independent socket
tests for topology, final refusal ACK and unsent expiry. Complete proposal,
terminal and production tests remain. See output map revision 5 for limits.

Output map 6 adds the nonblocking connecting facade and integrated direct/attached
proposal retries, immutable bytes, out-of-order domains and ACK/clunk barriers.
The 35 Rust output tests and C suite pass; terminal and production gates remain.

Output map 7 adds paired terminal socket checks and the shared
[ownership/recovery guide](OUTPUT-CLIENT.md). These are supplied-admission
checks, not protected grant revocation or production-export qualification.

Output map 8 records both independent SDK peers passing the same production
output worker exchange. Admission and topology-validation outcome are supplied;
protected launch, Engine application and release qualification remain separate.

Output map 9 records independent C/Rust protected launch, admitted-root handoff
and idle revocation, retaining negotiation custody. In-flight proposal revocation
and release-wide qualification are not established by this result.

[Experimental lock parity](LOCK-PARITY.md) map 6 records the integrated Rust
Session and nonblocking Connecting facade, including explicit retry, retained
journal/object state, custody, 1–8 upload writes and draining cancellation.
Thirty-seven Rust tests and C's independent lock suite pass. Scripted socket
peers prove control progress and immediate cancellation readiness with held
upload replies. Production/protected-launch and release gates remain open;
the pinned lock contract is still draft.

Lock map 7 adds paired candidate C/Rust production upload/presentation and
protected idle-revocation gates. The new C obligations query matches Rust's
passive progress inspection without exposing private state. SDK reauthentication,
in-flight upload revocation and the release-wide negative-case audit remain open.
Lock map 8 adds paired protected revocation of an unfinished upload with a
settled prefix and queued tail. [Administration](ADMIN-PARITY.md) now pins the
accepted file contract and names the missing operations in both SDKs. Broker
files remain design-only under Sophia t314; portal requester and recipient
interfaces need separate typed coverage and must not share authority.

Admin map 2 adds equivalent allocation-free info decoding in C and Rust.
It validates service identity and advertised timeout fields only; neither
SDK yet has an administrative session client.

Admin map 3 adds borrowed invocation/status decoding and bounded invocation
encoding, including all ten outcomes and unchanged output on failure.
Catalog and administrative session support remain missing in both SDKs.

Admin map 4 adds bounded, allocation-free complete catalog decoding and
binary-search membership lookup in both SDKs. Administrative session support
and its production gates remain missing.

Admin map 5 adds matched-reply snapshot state machines in both SDKs, with
reusable storage and EOF/clunk barriers. The full administrative connection
driver and its production qualification remain missing.

Admin map 6 adds paired pipeline startup, bounded dispatch and same-connection
handoff through info/ctl. Command sessions and production qualification remain.

Admin map 7 adds paired immutable invocation custody and caller-driven status
scheduling, retaining uncertain and confirmed outcomes distinctly. Full session
integration and production gates remain.

Admin map 8 adds integrated same-connection sessions, retained catalogs,
monotonic command IDs, pending-only status snapshots and terminal custody.
Paired scripted socket exchanges pass; production qualification remains.

Admin map 9 adds nonblocking caller-connected socket facades and shared
ownership/recovery guidance. Startup deadlines and I/O budgets span phase
transitions. Production and release qualification remain open.

Admin map 10 adds paired production-export completion, stale-catalog refresh
and admission revocation after owner claim. Service loss and release-wide
qualification remain open.

Admin map 11 adds paired production service teardown before and after owner
claim, preserving client uncertainty and the server's queued/dispatched boundary.

Admin map 12 adds explicit fresh-service recovery with a different command,
preserved old uncertainty and isolation from an old ticket's late completion.

Admin map 13 adds paired abrupt service-process death before/after claim,
with retained uncertainty and no automatic retry.

## B3 upload window (working candidate)

Zero and oversized pixel Rwrite counts fail the connection in both SDKs without
resending. Rust retains Unknown custody for an issued unit whose invalid reply
establishes no accepted prefix; an oversized count cannot establish Stored on
terminal cleanup. Independent regression tests cover both invalid counts. C's
borrowed-buffer API exposes terminal failure rather than per-chunk custody.

Window regressions also cover terminal-event draining, head ESTALE, and partial
transport frames during refill. Rust tests cancellation with four issued and four
unissued chunks: confirmed writes remain Stored and unissued chunks become
DroppedUnsent after the peer's terminal status, not because enqueuing Cancel
preempts the outbox. An explicit Rust ResourceCancel follows already-queued
chunks of its resource. C refuses a caller cancellation while its borrowed upload buffer
is outstanding; a terminal resource event still fences and drains that window
before clunking and releasing the buffer. Neither shell SDK flushes upload tags.
Rust's slow-reader test forces kernel socket backpressure; C's one-byte dispatch
budget forces partial SDK output across calls. Both verify ordered window refill.

The shell client keeps up to four adjacent same-resource canonical upload units
in an inline window. Each retains its payload charge and custody ticket. Replies
match by tag but settle in issue order; a returned credit permits the next chunk
without waiting for the entire burst. Records and resource changes stop gathering.
Fragmented prefixes are confirmed sequentially; the final fragment can overlap
following units. Every write respects iounit and the canonical unit boundary.

Short writes and EAGAIN drain invalidated tails before resuming at the confirmed
cursor. Only EAGAIN backs off. Fencing prohibits recovery, and terminal status
drains outstanding replies before clunk. End waits for all upload units to settle.
Both SDKs close the writer after draining a permanent head refusal, cancelling
that resource before End. Rust previously kept the binding after refusing one unit;
later chunks now cannot resume that abandoned binding.
Five request credits remain reserved for control work. The fixed window adds no
per-burst allocation and does not change the caller API.

The paired C candidate windows a borrowed multi-chunk buffer, rather than Rust's
separate outbox units. A one-chunk C call usually has depth one. C can window
fragments and retries after event/ACK progress or time; Rust retries on time.
C retains its unconsumed-event refill gate and confirmed-ACK transaction barrier.
These scheduling differences remain explicit. C adds a low-level upload_retry
call; its high-level session owns the independent timer. Its client/session
layouts and default request storage grow, requiring consumer rebuilds.

Independent window/recovery tests pass locally; exact signed B3 revisions, export
adoption and native performance remain pending. Counts come from scripted peers
and instrumented captures, not a new production metrics API.

# Portal SDK parity

Revision 23, 2026-10-16. Recipient components are developing; there is no complete
portal client or parity claim.

The unchanged recipient contract in `spec/portal-client-files.md` comes from
Sophia `9b57fd040`, SHA-256
`d5e2bceed3ecbd56c119ecd87547f9437ee43a813b591df1f638f404681a5b97`.
Its installation and qualification statements describe that source snapshot,
not SDK evidence.

Both SDKs decode the exact 64-byte frame record into passive fixed-size values.
They validate version, format, nonzero identities, u32 dimensions, canonical
stride and the 64 MiB image bound without allocating or touching pixels.
C leaves output unchanged on failure. Independent tests cover truncated and
trailing records, the exact limit, oversized and overflowing dimensions,
invalid fields and a one-pixel image. Both pass, as does strict Rust Clippy.

Both SDKs now encode the exact 24-byte capture request and decode the complete
32-byte capture-status snapshot without allocation. All eight states remain
distinct. Idle requires zero identities; other states require nonzero serial
and output. Rust returns an inline byte array; C writes into caller storage,
leaving output unchanged on error and preserving bytes beyond the record.
Independent literal and malformed-boundary tests pass, including every truncated
status prefix, trailing bytes, invalid version/state and identity combinations.
The two Rust portal tests, both C portal tests and strict Rust Clippy pass.
These codecs neither enforce serial ordering nor confer frame authority.

Both now decode exact version-1 api and status snapshots without allocation.
Request capability and transfer state remain distinct passive values. Decoders
reject unknown capabilities, invalid epochs, truncation and trailing bytes.
The maximum api/status records are 101/43 bytes. Independent discovery tests
and strict Rust Clippy pass; C preserves caller output on failure.

Remaining recipient work includes authenticated startup, retained-object frame and transfer reads,
watch/cancellation, custody, revocation and external-loop APIs. Reads should
use caller-controlled storage and bounded chunks; do not create another full
image copy. Decoded metadata alone never authorizes pixels or proves presentation.
Production gates and foreign-language bindings remain required.

The trusted portal-requester is a separate authority and contract; no recipient
API may approve transfers or claim executor custody. Requester coverage and the
broker migration prerequisite remain explicit t333 work.

Provisional metadata fetchers now emit Walk/Open/Read/Clunk operations for api,
status and capture-status. They expose results only after EOF, decoding and
successful clunk. Reads obey msize, iounit and per-file limits, with a one-byte
EOF probe at the bound. Caller-monotonic absolute deadlines cannot be extended
by partial progress. Refusals preserve wire errno and forbid automatic retry;
the driver must match replies and close on terminal failure.

Rust retains at most 101 bytes inline; C borrows caller storage. These fetchers
allocate no storage themselves, though Rust's supplied transport Reply may own
allocated data. They adapt each SDK's administrative snapshot driver without
importing administrative authority or dependencies. Independent tests cover
all three files, fragmentation, EOF/clunk barriers, overflow, malformed EOF,
short iounit, refusal, expiry and clock regression. Three Rust component tests,
C's snapshot test and strict Rust Clippy pass. They do not authenticate or own
the connection; startup, frame streaming and production gates remain.

Metadata startup now accepts the sole attached root on a quiescent role
pipeline, fetches api through EOF/clunk and returns the same root and pipeline.
The caller must supply the root from completed launch authentication and keep
the relay alive separately; root ownership is checked but proves no authority.
No version, attach or reconnect occurs. One caller-monotonic deadline covers
startup. Rust shares a bounded I/O budget; C services its pipeline once per
dispatch. Failure drops Rust's role pipeline; C requires its caller to close it.
C startup state must not move because its fetch borrows inline scratch.

Independent socket peers pass one-byte dispatch, retained-root handoff, invalid
root, unsupported api version, directory replies, clunk refusal and expiry.
Five Rust client tests, C's startup test and strict Rust Clippy pass. Their
scripted initial attach stands in for completed authentication; these tests do
not qualify the real launch exchange. Authentication composition, recipient
session operations, retained frame reads and production gates remain.

Independently built C and Rust peers now pass Sophia's opt-in
`external_sdk_portal_startup_uses_real_namespace_authentication` gate.
The production namespace launcher and Factotum admit each protected peer;
namespace discovery selects the portal endpoint, and the SDK authenticates
before handing its existing root to metadata startup. Each observes the
unavailable request capability and retains only the root after api clunk.
The original launch relay remains open until peer exit. Strict Rust Clippy
passes. This qualifies startup composition, not frame delivery, capture
custody, revocation or physical presentation. No SDK pin adoption is implied.

The frame range scheduler now keeps four requests in fixed storage. Counts
respect msize minus reply framing, iounit and caller chunk limits. Out-of-order
replies place bytes at their reserved offsets; short reads schedule only the
unread suffix. Tokens belong to one window and must be matched to its wire tags.
If admission is busy, retain the reserved request and submit it once.
Zero-length, oversized, unknown and duplicate replies fail the window closed.
The caller closes it on timeout, cancellation, revocation or transport failure.

Independent tests cover reversed short reads with exact byte coverage, four-slot
pressure, framing/iounit limits, terminal errors and a full 64 MiB range without
allocating image storage. Both tests and strict Rust Clippy pass. Payload range
completion is not image completion: the eventual session must verify EOF,
reread the retained metadata fid to reject replacement, and clunk successfully.
No private qid arithmetic is required. This scheduler copies no pixels; wire
integration, frame storage ownership and production delivery remain unfinished.

Both now integrate retained-frame reads with the four-request window. They
retain frame-info and frame fids, require exact payload and EOF, reread the
original metadata fid, and require both clunks before exposing a completed
image. No private qid arithmetic or automatic reconnect is used. An explicit
second read reuses the root and image storage without fetching api again.

Rust owns the role pipeline and reusable Vec, growing it once if needed after
metadata validation. It preserves initialized storage between captures, avoiding
another zero-fill for an unchanged size. C borrows the pipeline and caller image
storage, rejects insufficient capacity and allocates nothing. Both place each
reply directly in the final image buffer; there is no second full-image copy.
Rust transports still allocate reply data. Rust drops its pipeline on failure;
C requires the caller to close its borrowed descriptor. Partial storage is
scratch, never a completed image. Retain the launch relay separately.

Independent socket tests pass short reads, one-byte dispatch, buffer/connection
reuse, stale or changed metadata, extra pixels, early termination, clunk refusal,
deadline expiry and cancellation. C additionally tests insufficient capacity.
All nine Rust client tests and strict Clippy pass; C's frame-client test passes.
These are scripted peers, not production frame-delivery or revocation evidence.
Capture requests and the remaining recipient operations are still unfinished.

Both independently built peers now pass production authenticated delivery of
a supplied 256×128 RGBA8 frame. The trusted fixture submits the capture request
and the real coordinator installs the frame; each SDK authenticates, reads it,
checks output/generation/presentation identities and every pixel, and retains
only the role root afterwards. Both external startup/delivery gates and strict
Rust Clippy pass. This qualifies the SDK/export boundary with supplied policy
and pixels, not SDK request custody, mid-read revocation, native presentation,
physical installation or performance.

Both peers also pass the production mid-read revocation gate. Each pauses after
accepting some pixels; the fixture advances the lock fence before reading
continues. Both terminate without a completed image, and deadline expiry cannot
pass the test. The new received-byte accessor reports scratch progress only.
Bytes already buffered before revocation may still arrive; this gate does not
claim they can be recalled. All three external portal gates, the existing
portal-dispatch regressions and strict Rust Clippy pass. Capture-request custody
and the remaining recipient operations still need implementation.

Capture-custody components now distinguish queued, issued, accepted, settled,
refused, dropped-unsent and unknown requests. They keep the 24-byte write inline
and allocate nothing. Pipeline admission marks issued; only an exact Rwrite
marks accepted. A complete Rlerror records refusal; short or lost replies forbid
replay. Closing locally cannot cancel work already admitted by the server.

Only fresh EOF/clunk-verified capture-status snapshots may settle a request.
Serial and output must match. Queued/capturing remain pending, while ready,
denied, unavailable, pressure and cancelled remain distinct terminal outcomes.
Ready grants no frame authority. A caller-monotonic deadline of at most ten
seconds covers the whole observation, without extension from partial progress.
Pending status schedules another snapshot no sooner than the next millisecond;
settled or failed requests schedule none.

Rust `capture::Pending` and C `sophia_pc_*` pass independent tests of all
outcomes, refusal, short replies, loss/cancellation, mismatched status, malformed
records, clock regression and fixed deadlines. All twelve Rust client tests
and strict Clippy pass; C's capture-client test passes. These are component
tests: serial allocation, write limits, wire driving and production request
qualification remain unfinished. Existing authenticated frame/revocation gates
do not qualify this new request path.

The request driver now composes custody with the authenticated role transport:
Rust `capture_request::Request` and C `sophia_pcr_*`. It checks the advertised
capability and caller-supplied serial high-water mark, opens capture write-only,
and admits one complete offset-zero write within msize/iounit limits. It closes
that fid before opening fresh status snapshots. Results require EOF and clunk;
pending status uses the caller's timer, without sleeping inside dispatch.
Failure closes Rust's owned pipeline; C callers must close their borrowed fd.
Neither driver retries or reconnects.

Successful handoff preserves the root and returns the serial for the next
explicit request. The caller must maintain the recipient owner's high-water
mark across reconnects and reserve each serial before use. Rust returns its
owned Opened value; C exclusively borrows an immovable opening and restores it
on finish. C's driver and metadata scratch allocate nothing. Rust uses inline
metadata storage but its transport still allocates replies.

Independent socket tests cover exact bytes, one-byte dispatch, fragmented
status, repeated explicit requests, refusal, short writes, disconnect, wrong
status identity, terminal-status clunk refusal, small iounit, directory replies,
unsupported capability, old serials, expiry and cancellation. The fifteen Rust
client tests (including the added request cases), C request test and strict
Clippy pass.

Both peers pass Sophia's production request-to-frame gate: real namespace
authentication, SDK request submission, coordinator settlement, frame metadata
and every pixel verified, then root-only ownership. All four external portal
gates pass independently in C and Rust. The fixture supplies policy and pixels;
this is neither native capture acceptance nor measured performance evidence.
Production request refusal, cancellation/revocation and reconnect audits remain,
alongside transfer/watch, trusted requester and release work.

Both peers now pass production capture refusal and lock-cancellation gates.
Locked admission returns EACCES, an occupied queue EAGAIN, and an old serial
ESTALE; each SDK records Refused with the exact errno and no outcome. Advancing
the execution fence cancels queued work and work already taken by its owner.
Both SDKs report Cancelled after a verified status snapshot and retain only the
root. Unlock does not revive the old work: the fixture rejects its late Ready
completion and confirms no frame was installed.

All nine external portal gates pass independently in both languages; strict
Rust Clippy also passes. These checks use the real request queue and export
with supplied policy. They do not qualify native scheduling, local cancellation
after wire admission, reconnect recovery, or the remaining transfer/watch APIs.

The snapshot operation driver now supports opaque retained transfers, capped at
65,536 bytes. Rust `Fetch::transfer` borrows a mutable caller slice; C uses
`SOPHIA_PS_TRANSFER` with its existing borrowed storage. Both require at least
64 KiB, leave unused bytes untouched and allocate no payload buffer. Rust's
Fetch now carries a storage lifetime; metadata constructors still store their
small records inline. Transport reply allocation is unchanged.

The driver reads bounded chunks on one retained fid, probes EOF at the limit
and exposes bytes only after successful clunk. It does not decode payloads or
infer a relationship to a separately walked watch fid. Independent tests pass
empty, one-byte and exact-limit payloads, fragmented replies, undersized storage,
overflow, stale reads and clunk refusal. All seventeen Rust client tests,
C transfer/snapshot/request tests and strict Clippy pass. Rebuilt peers pass
the nine existing production gates as regression checks; these do not exercise
the new transfer path. Transfer transport composition and production delivery,
watch and the remaining recovery audits still need implementation.

Rust `transfer::Transfer` and C `sophia_pt_*` now drive the retained transfer
reader on an exclusive authenticated root. Both borrow caller payload storage,
use bounded dispatch and one absolute deadline, and return the same role after
EOF/clunk without copying the payload. Rust drops its owned role on failure;
C callers close their borrowed fd. C borrows an immovable opening exclusively
and restores it on successful finish. Scratch progress is not a completed result.

Both independent peers pass production delivery, absent-object and mid-read
revocation gates. Delivery verifies the original buffer pointer, unchanged
unused bytes and root-only ownership. Absence preserves ENOENT. Revocation
after payload receipt but before completion returns ESTALE without exposing a
result; buffered bytes are not claimed to be recalled. All twelve external
portal gates, seventeen Rust component tests and strict Clippy pass. The
production payload is seven supplied bytes; the 64 KiB boundary remains
component evidence. Watch, transfer recovery/local cancellation and release
qualification remain open.

Rust `watch::Watch` and C `sophia_pw_*` now retain a watch fid and issue one
nonempty offset-zero read. They neither poll the file repeatedly nor reopen it
automatically. ESTALE on the retained object is followed by clunk; only then is
revocation reported and the original root returned. A separately fetched
transfer is not assumed to share the watch's object identity.

Local cancellation closes Rust's owned role; C marks the operation closed and
requires its caller to close the borrowed fd. Expiry follows the same failure
path. Neither reports revocation or preserves the connection through a flush.
Each driver owns the role exclusively, so composing concurrent watch and other
recipient operations remains a session-integration gap.

Both peers pass production lock, replacement, source-generation, absence,
local-cancellation and deadline cases. Revocation returns root-only ownership;
absence preserves ENOENT; cancellation/expiry remain distinct from revocation.
All eighteen external portal gates and strict Clippy pass. These do not measure
idle cost or qualify malformed watch replies, clunk refusal, flush-based
cancellation or reconnect recovery; those audits and release work remain.

Independent scripted peers now qualify malformed watch replies and explicit
root reuse. Empty and payload-bearing Rread, unrelated remote errors, directory
walk replies, disconnect and failed clunk all leave revocation unavailable.
ESTALE during open or the pending read completes only after clunk. Two explicit
watches reuse the original root with one version/attach exchange, including
one-byte dispatch and clock-regression checks. Both watch tests and strict Rust
Clippy pass; no production behavior changed.

This closes the malformed-reply/clunk gap for the standalone watch driver.
It does not qualify reconnect after role loss or a shared recipient session.
The next integration must route watch and foreground replies by their own
tags on one pipeline, preserve independent object identities and bounded
storage, and provide explicit cancellation without losing unrelated work.

The developing shared session now runs one watch alongside one metadata or
transfer snapshot on a single pipeline. Rust `session::Session` and C
`sophia_pcs_*` route replies by separate tags, retain watch events until
released and use bounded dispatch. Transfer bytes stay in caller storage;
Rust returns the full borrowed buffer and payload length for reuse.

Watch cancellation drains already decoded replies, sends Tflush for a pending
read and clunks the retained fid. A revocation reply preceding Rflush takes
precedence over cancellation. Foreground work remains on the same connection.
Original deadlines bound cleanup; expiry or a terminal protocol/transport
failure still ends the role. No operation is replayed after loss.

Independent production peers pass status access with a pending watch,
cancellation while a transfer continues, and an old watch's revocation beside
a replacement object's transfer. Both finish with root-only ownership.
All twenty external portal gates, nineteen Rust component tests and strict
Clippy pass. Deterministic flush/reply races and shared-session failure tests
remain; production scenarios do not qualify every ordering. Shared capture
requests and frame reads are still missing, as are the remaining recovery,
trusted requester/broker and release gates.


Independent shared-session peers now force five cases: Rflush alone, watch
ESTALE before Rflush, failed watch clunk after that revocation, an error reply
to Tflush, and disconnect during flush. One-byte dispatch preserves foreground
status work in the two successful cases; explicit releases leave only the
original root. Terminal failures hide both the watch event and snapshot result.
Each case checks one version/attach, one watch and one flush with the exact
old tag. Backwards caller time is refused without poisoning the session.

These tests pass in native C and Rust. Production code is unchanged. They do
not yet force a reply already decoded when cancellation is requested, malformed
watch reads during flush, or every pre-read cancellation stage. Those cases,
shared frame/capture composition and the remaining release gates stay open.


The shared-session suite now adds five cases. Empty and payload-bearing watch
reads during flush fail immediately, even without Rflush. Cancellation before
walk dispatch or while the peer holds the open reply clunks the watch without
issuing its read or a flush, while foreground status work continues.

A peer handshake also sends status and revocation in one batch before local
cancellation. Rust's exact read budget leaves the decoded revocation unconsumed;
C drains both replies in that dispatch. Both retain Revoked and complete status
without Tflush. A Rust negative control that flushes immediately in cancel_watch
fails with "tag not outstanding"; restoring the source passes. These are native
scripted-peer checks with supplied admission, not new production-authentication
or performance evidence. Both suites and strict Rust Clippy pass.

Next integrate capture requests and frame reads into the shared scheduler.
Reconnect/recovery, trusted requester/broker and publication remain open.


## Shared capture scheduling

One capture request can now run beside a retained watch. It occupies the single
foreground slot until its result is released; snapshots and transfers return
Busy meanwhile. Both drivers reuse their exclusive capture operation state,
including atomic writes, custody, immutable status snapshots and paced polling.
The session remains the sole socket driver with one shared dispatch budget.
Capture records and status scratch remain inline; no frame storage is involved.

| Capability | Rust Session | C session |
| --- | --- | --- |
| Reserve request | `capture` | `sophia_pcs_capture` |
| Inspect custody after failure | `capture_custody` | `sophia_pcs_capture_custody` |
| Retained historical outcome and serial | `capture_result` | `sophia_pcs_capture_result` |
| Release foreground slot | `release_capture` | `sophia_pcs_release_capture` |
| Earliest wake, including paced status | `deadline` | `sophia_pcs_deadline` |

The caller reserves increasing durable serials across reconnects. Ready remains
historical status, not permission to read pixels. Session failure hides results
but preserves custody; neither driver replays a request. C's caller-owned session
layout grows and must be recompiled. There is no released ABI update yet.

Independent tests cover cancellation of a watch during accepted capture,
fragmented status, paced Queued-to-Ready observation, two explicit requests,
Busy admission and seven failure modes with matching custody. Both independent
peers pass all 21 production portal gates, including a new request replacing a
watched capture object. Strict Rust Clippy and the affected C tests pass.
Shared frame reads, broader recovery, trusted requester/broker, foreign bindings
and publication remain open. This is not physical or measured performance evidence.


## Shared frame scheduling

A frame read now occupies the same foreground slot as a snapshot or capture
request, beside one retained watch. Both SDKs reuse the exclusive frame state:
retain metadata and pixel fids, read through the existing four-request window,
check exact EOF, recheck the original metadata, and clunk both before exposing
an image. Six request slots reserve four pixel reads plus watch/flush; four
fids retain root, watch, metadata and pixels. Rust also validates input capacity
for four bounded Rreads and two scalar replies, using the existing 60 KiB
payload cap. The default pipeline bounds suffice.

| Capability | Rust Session | C session |
| --- | --- | --- |
| Start retained frame | `frame` | `sophia_pcs_frame` |
| Inspect scratch progress | `frame_received` | `sophia_pcs_frame_received` |
| Borrow verified result | `frame_result` | `sophia_pcs_frame_result` |
| Release result | `take_frame` moves Vec and metadata | `sophia_pcs_release_frame`; caller already owns pixels |
| Recover failed scratch | `take_frame_scratch` after terminal failure/close | Caller retains storage; close role fd before reuse |

Rust reuses the supplied Vec and grows it at most once after metadata validation.
C requires sufficient disjoint caller storage and allocates nothing. Neither
adds a second image copy. Partial or failed storage remains scratch; results
stay hidden after any terminal session error. This grows the unreleased C
session layout again; recompile its users.

Independent peers require four pixel reads before replying, deliver that first
batch in reverse order with short reads, cancel a watch during a partial image,
and reuse the same image address for a second frame. Both run at the minimum
six request/four-fid capacity and cover stale recheck, changed metadata, excess
pixels, premature EOF and failed clunk; C also checks insufficient storage and
overlap. All 22 Rust portal tests, the affected strict C tests and strict Rust
Clippy pass. Both external peers pass all 22 production portal gates, including
frame completion despite watch cancellation.

Recipient composition is now implemented. Remaining work includes the trusted
portal-requester API (a separate admitted role), explicit recovery qualification,
the broker export/client dependency, foreign bindings and signed publication.
No physical acceptance or measured performance improvement is claimed here.


## Trusted requester record foundation

The `requester` Rust protocol module and C `sophia_portal_requester_files.h`
now provide matching passive codecs for the separate trusted requester role:
canonical api discovery; Limits; Negotiate, TransferRequest, PayloadBegin,
PayloadEnd and TransferCancel candidates; Negotiated, Refused, Submitted,
Decision and TransferOutcome events; and exact submit/ack controls.

Transfer IDs, source generation and owner-clock deadlines may be zero in a
request so the lifecycle can answer Denied. Unsupported negotiation ranges
remain representable for Refused. All seven transfer kinds are typed. Optional
UTF-8 MIME preserves absent versus present-empty and is bounded to 255 bytes.
Decoders reject malformed envelopes, padding, enums, lengths and limit bounds;
Denied cannot carry hidden grant fields. Encoders use fixed scratch storage
and leave caller output unchanged on failure. No payload allocation is added.

The pinned requester document, KDL and referenced broker custody document come
unchanged from signed Sophia `519f07fd1b6db3a665ae1e74368a3d2e234cbf9c`:

| File | SHA-256 |
| --- | --- |
| `sophia-portal-files.md` | `81d8983fd05bc0efd0cf0d36cc82d2d7d89467b1fa105d758024e11008aa568b` |
| `sophia-portal-files-v1.kdl` | `eb14ff3474b297e44409326119f22ed5ff048bb558452106d3c85ffc494244dd` |
| `sophia-broker-files.md` | `01ef46dab00a05ef2e097f5b57d180c7be749f84ee3f43e031feaa5656bad2bc` |

The KDL's old design-only preamble predates the requester implementation;
the accompanying requester document describes the implemented private channel.
The broker document remains design-only. Copying it provides the referenced
custody rules, not evidence of a broker export or client.

Independent native tests check literal layouts, domain-refusal representation,
MIME boundaries, all candidate/event kinds, truncation, padding, bounds and
unchanged failure outputs. C also tests invalid UTF-8 sequences; Rust accepts
borrowed `str`. The focused tests, strict Rust Clippy, C UBSan trap-mode run and
both spec manifests pass. These are codec checks only. Requester startup,
negotiation, retained journal/ack, submission custody, upload, terminal outcomes
and production-export qualification remain to be implemented in the SDKs.

## Requester submission and journal state

Rust `requester::{Submission, Journal}` and C
`sophia_portal_requester_state.h` now provide equivalent fixed-storage state.
Candidates remain immutable; EAGAIN permits explicit same-ID retry without
extending the deadline. A correlated receipt establishes Submitted custody even
before the write reply. Connection loss preserves known custody and marks an
issued operation without a receipt Unknown.

The journal accepts partial and coalesced records in 512 bytes, retains one
application event, and freezes each cumulative acknowledgement until its exact
write reply. Reads and consumption do not extend the acknowledgement deadline.
The local timer starts with observed bytes; the server still enforces its own
publication-based deadline. These components neither admit a connection nor
perform I/O. The transport owner must close on terminal errors.

Six Rust tests and an independent C suite cover reply ordering, retry, uncertain
delivery, fragmented/coalesced events, frozen acknowledgements, deadline expiry,
sequence gaps, short replies and clock regression. Strict Rust Clippy and the
C warnings-as-errors build pass. Startup, negotiation, transport integration,
grants/uploads and production requester gates remain open; this is not a
complete requester client or a performance result.

## Requester discovery startup

Rust `requester::startup::Opening` and C `sophia_pqo` take exclusive use of a
caller-admitted trusted requester root. They read exact `api` and `limits`
snapshots, require matching epochs and successful clunks, then hand the same
connection/root to the session. Discovery does not establish admission or
complete negotiation. The caller retains the trusted authority channel.

Both drivers use fixed 128-byte metadata storage, honor iounit, bound each
dispatch and keep one deadline across both files. Independent scripted peers
pass fragmented reads with one-byte I/O budgets, root preservation, no repeated
attach/version, malformed versions/qids, clunk errors, mismatched epochs,
truncated/oversized limits, clock regression and expiry. Rust Clippy and the
strict C build pass. Requester negotiation, operation transport and production
gates remain open; these fixtures do not qualify trusted admission.

Rust's `transaction::Transaction` and C's `sophia_pqt` produce bounded staging,
submit and cleanup operations for the sole transport driver. Independent tests
cover explicit retry, receipt/ack-gated release, deadlines and uncertain delivery.
Both preserve Submitted custody when cleanup fails and leave EAGAIN retry to the
caller. Rust's eleven requester state/startup/transaction tests pass; C's matching
transaction suite passes both normally and with UBSan trap instrumentation.
The ordinary UBSan build could not link the unavailable runtime; trap mode
requires no runtime. Integrated journal/session driving and production gates
remain missing; no complete requester capability is claimed.

Rust's developing `requester::session::Session` and C's `sophia_pqr` integrate
the handshake: one I/O owner drives the retained journal, explicit acks and
transaction cleanup. Independent scripted peers pass successful negotiation,
receipt-before-write ordering, refusal, wrong receipt and short ack at the
minimum three request slots and five fids. Rust uses at least msize + 22 bytes
of input capacity. C also tests discovery-to-session handoff and invalidates the
consumed opening. Readiness requires negotiation ack and transaction cleanup.
Rust drops the role connection on terminal failure; C requires the caller to
close its fd. Subsequent request/grant/upload operations and production requester
gates remain absent; these gaps block complete parity and release.

## Request and grant correlation

Rust `requester::transfer::Transfer` and C `sophia_pqf` now retain one request's
expected grant without allocation. Allowed must match transfer ID, namespaces,
kind, source generation, Limits' broker generation and owner-clock deadline.
Zero-ID/generation/deadline requests remain representable for domain Denied.
Terminal outcomes are single-use; Completed requires a previously issued End.
Disconnect hides the grant but preserves the last known state, including an
End whose execution is uncertain. It does not manufacture Revoked or Completed.

Three Rust tests and an independent C suite pass every grant-field mismatch,
domain denial, disconnect, premature completion and repeated terminal outcomes.
Strict Rust Clippy and the strict C build pass. These are passive correlation
components; session request admission, grant routing and upload ownership still
need integration and production qualification. A generic Allowed does not
promise that the selected transfer kind has an implemented executor.

## Session request integration

Both sessions now admit typed requests with increasing submission IDs and
connection-scoped request handles. They correlate Decision and terminal events
before exposing state, preserve an original grant when a duplicate request is
denied, and retain terminal results until their journal records are acknowledged
and the caller releases them. Fixed storage holds 64 grants plus one pending or
denied request; retained terminal results occupy slots until released.

Independent socket peers pass allowed/denied requests, duplicate denial,
revocation, broker-generation mismatch and premature Completed rejection.
Rust's four session tests and C's eleven session schedules pass at minimum
transport limits; strict Clippy and the C build pass. Begin/upload/End
and production requester qualification remain open. Earlier component-only
limits above describe their checkpoints, not the current session coverage.

Both sessions now submit Cancel for an Allowed grant before End issuance, with
explicit retry and retained custody. Submitted does not imply Revoked; only the
correlated outcome changes that observation. Repeated local cancellation refuses.
Request handles include SDK issuer identity as well as wire epoch and submission:
a unique process-local Rust issuer or C's immovable session address. Matching
wire epochs do not let another live session's handle select a transfer. C session
storage must not be reinitialized while its handles remain live. Independent
tests cover cancellation and foreign-handle refusal; no production claim yet.

## Upload writer component

Rust `requester::upload::Upload` and C `sophia_pqu` implement one upload/0
writer after correlated Begin custody. They honor iounit and msize, account only
exact successful writes, and retain the open writer after the final byte.
Explicit cleanup follows End custody or terminal settlement; a clunk alone is
not a transfer outcome. A premature clunk can abandon and revoke the upload.

Rust takes the caller's Vec and returns its original storage; C borrows immutable
caller storage. No separate payload allocation or copy occurs in this component.
Independent tests pass empty, one-byte and 64-KiB payloads, bounded writes,
storage identity, partial walk, short writes, ESTALE, deadlines and clunk failure.
Strict Clippy and C builds pass. Begin/End session integration, in-flight
revocation/cancellation and production qualification remain open.

Rust now integrates Begin/upload/End in the session. Its fifth session test
passes empty and 64-KiB payloads, exact End length, writer retention through End,
Completed routing and original-buffer return. Upload requires four request slots
and six fids. `transaction_ready` separates command admission from negotiated
session readiness. Begin consumes its Vec even on admission error, as documented;
this ownership limitation remains subject to the release audit.

Both standalone writers support terminal settlement while a write is pending:
drain its reply, accept ESTALE for the ended binding, then clunk without another
write. Independent component tests pass.

Both sessions now integrate Begin/upload/End and transaction readiness. C's
borrowed payload remains valid through writer cleanup; Rust returns the original
Vec. Independent socket tests pass empty and 64-KiB uploads and cancellation
while the completed writer is held open. Cancel preserves Allowed until the
correlated Revoked event; cleanup then releases the upload storage. Rust's six
session tests and C's fifteen schedules pass. Upload needs four request slots
and six fids in either SDK.

Both sessions also pass revocation during the first pending payload write. The
peer withholds that reply until the Revoked record is acknowledged, then returns
ESTALE. It rejects another payload write and verifies writer clunk before close.
The caller recovers its storage and retains Revoked. Rust's seven session tests
and C's sixteen schedules pass.

A separate control returns ESTALE with no terminal event. Both sessions fail
closed, preserve the last Allowed observation, hide its grant and release
payload storage after connection closure. They neither replay nor fabricate
Revoked. Rust's eight session tests and C's seventeen schedules pass. This
qualifies safe failure, not connection-preserving recovery when ESTALE precedes
a later terminal event; that recovery behavior remains for the ownership audit.

Cancellation during an outstanding write, disconnect around End
custody and production requester gates remain open. These scripted-peer results
do not establish complete portal parity or a measured performance improvement.

## Production requester checkpoint

Both independent requester peers now pass Sophia's
`external_sdk_requester_production_lifecycle` gate against the real
`PortalFileExport`. Each runs completion, executor failure, denial and
cancellation with empty and 64-KiB payloads. Two further modes close before End
and lose the outcome after the executor takes End, for twelve cases per SDK.
The first executes nothing; the second executes exactly once but leaves the
client at disconnected EndIssued, never a fabricated Completed or Revoked.
A thirteenth case cancels a partially acknowledged 64-KiB upload, with a
one-byte dispatch budget making the intermediate state observable. Both peers
assert that acknowledged bytes are greater than zero and less than the payload,
then require Revoked and zero executions. This covers partial-upload
cancellation, not a controlled ordering of a withheld write reply.
Both return/release payload storage. The harness waits for transport retirement
and rejects any late execution after peer exit. It admits only the supplied
socket, verifies exact execution bytes and counts, and requires no execution
for denial or cancellation. It uses owner-supplied test authority, not a desktop
or recipient frontend; real application execution is outside this gate.

Rust peer SHA-256:
`6d4cbc96f611bdd946d7b322c3599f6ee93f3557076de832ca8a00cb28390db9`.
C peer SHA-256:
`955e6ea95b278fb2e0a02b891156f7ff520d8d4cb0e829ea77a4ce64633449c0`.
These are uncommitted builds over Rust base `4c18605` and C base `2d719d5`;
they are not release identities. The Sophia harness is uncommitted over
`519f07fd1`. Rust peer Clippy and Sophia's 17 ordinary portal-file tests pass.
Production race/recovery qualification, the cross-role audit and publication
remain open; the earlier statement that no requester production evidence exists
is superseded by this checkpoint.

## Requester API comparison

These entry points use the pinned requester revision 1. C names below have the
`sophia_pqr_` prefix; Rust methods belong to `requester::session::Session`.

| Operation | C | Rust | Ownership and evidence |
| --- | --- | --- | --- |
| Discovery and negotiation | `sophia_pqo_*`, `init` | `startup::Opening`, `new` | Supplied admitted root; neither discovers authority from attach strings. Production peers exercise both. |
| Readiness and bounded service | `ready`, `transaction_ready`, `dispatch`, `deadline`, `wants_write` | `ready`, `transaction_ready`, `dispatch`, `deadline`, `io_interest`, `fd` | C retains the caller's wire/fd. Both take caller-monotonic milliseconds and byte budgets. |
| Journal retention | `event`, `consume`, `acknowledge` | Same method names | Consumption is local; acknowledgement releases server retention, not semantic custody. |
| Requests and results | `request`, `transfer`, `release_transfer` | Same method names | Connection-scoped handles, exact grant correlation, acknowledged terminal results before release. C sessions must not move or be reinitialized with live handles. |
| Upload lifecycle | `begin`, `upload_written`, `upload_ready`, `end`, `release_upload` | `begin`, `upload_written`, `upload_ready`, `end`, `take_upload` | C borrows immutable bytes; Rust owns the original Vec. Both retain storage through cleanup. Rust consumes the Vec on local admission error: unresolved release-audit limitation. |
| Cancellation | `cancel` | `cancel` | Before End issuance only; neither Submitted nor local cancellation invents Revoked. Held and partial upload production cases pass. |
| Explicit retry | `retry`; transaction custody in retained state | `retry`, `custody` | Same immutable submission after EAGAIN; no reconnect replay. Independent session tests qualify End retry with empty/64-KiB uploads. |
| Local close | `close`, then caller closes fd | `close` closes owned pipeline | Preserve known observations, hide grants, release storage; real pre-End and post-custody disconnect cases pass. |

Both session peers now refuse the first End with EAGAIN, wait for explicit
retry, compare the complete staged candidate and submit control byte for byte,
then publish one Completed. Sixteen intervening dispatch calls leave custody
Queued and the transfer conservatively EndIssued. Nine Rust session tests and
nineteen C schedules pass. This does not claim that EndIssued proves execution,
or provide cancellation/abandonment of a refused End on a live connection.

Rust now also offers `begin_buffer(&mut Vec)`: local rejection preserves bytes,
pointer and capacity; admission takes the original storage without a copy or
allocation. After admission, recover it through `take_upload`, including after
a transport error. The original consuming `begin` remains a convenience API.
C's existing borrowed Begin already preserves caller storage on rejection.
Independent session tests cover deadline rejection, successful reuse and a
second Begin refused while busy. Both production peers now exercise rejected
Begin followed by successful use of the same storage; thirteen cases each pass.
Strict Rust Clippy passes all portal-client targets. This resolves the earlier
failed-admission storage gap without requiring identical memory models.

The table does not close the recovery audit: terminal-error ordering still
needs explicit release decisions and
paired evidence. Broker coverage and foreign-binding qualification remain
separate t333 requirements.

## Foreign binding and recovery audit

C's `make check-binding` compiles and runs
`src/tests/requester_binding_smoke.nim` using imported public C declarations
and the two native static libraries. Empty, one-byte and 64-KiB payloads pass:
caller-owned state has the header's layout, writes borrow the original buffer,
held storage survives until clunk, and rejected deadlines leave caller bytes
intact. No Rust runtime, Sophia checkout or C adapter is needed. This meets the
thin foreign-binding smoke requirement; it is not a full Nim role client.
The full C `make check` and both Rust portal crates' tests also pass.

Requester recovery is explicit: neither SDK reconnects or replays. ESTALE before
a correlated outcome closes the session and leaves the last observation
unchanged; ESTALE after terminal settlement drains the outstanding operation
and cleans up the writer. A disconnected EndIssued is unknown execution, even
when the owner later succeeds. These paired behaviors obey the pinned rule that
disconnect cannot initiate execution or replay. They deliberately do not promise
connection-preserving recovery from arbitrary remote errors. C callers must
close the supplied fd before releasing borrowed payload storage on failure.

End EAGAIN can be retried with its retained identity. Neither session currently
offers live cancellation of that refused End; close remains available without
inventing a remote outcome. Preserve this limit in the final role audit rather
than treating EndIssued as proof of execution.
